//! AST node types and pre-computed codegen metadata.
//!
//! Defines the concrete node types stored in the [`TemplateAst`] arena:
//! [`ElementNode`] (boxed), [`TextNode`], [`CommentNode`], and
//! [`InterpolationNode`]. Each element carries pre-computed
//! [`ChildrenFlag`]/[`PropFlag`] bitsets and a [`ChildrenMode`] enum so
//! codegen can branch without re-scanning children or props.

use smallvec::SmallVec;

use crate::parser::types::RootNodeTemplate;
use crate::types::{NodeId, NodeProp, NodeTag};

/// A node in the arena (either an element or a leaf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstNode {
    pub kind: AstNodeKind,

    /// None => the node is a root child
    pub parent: Option<NodeId>,

    /// Position in the parent's children vec (or in root.content.children if parent == None)
    pub index_in_parent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstNodeKind {
    /// Boxed to keep the enum small (40 bytes vs 416 bytes per arena slot).
    /// ElementNode is ~392 bytes while leaf variants are 8-12 bytes; boxing
    /// reduces Vec<AstNode> reallocation cost by ~10x and improves construction
    /// throughput by ~24% at 1000 elements (see `box_element_bench`).
    Element(Box<ElementNode>),
    Text(TextNode),
    Comment(CommentNode),
    Interpolation(InterpolationNode),
}

/// Which branch of a `v-if` / `v-else-if` / `v-else` chain this element belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementNodeConditionKind {
    If,
    ElseIf,
    Else,
}

/// A cached `v-if` / `v-else-if` / `v-else` directive on an element.
///
/// Stored directly on [`ElementNode::v_condition`] so codegen can inspect
/// the condition branch without searching through the props list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementNodeCondition {
    pub kind: ElementNodeConditionKind,
    /// The original directive prop (carries value span for the expression).
    pub prop: NodeProp,
}

/// Tag classification for codegen branching.
///
/// Determined by the syntax layer from tag name bytes during element open.
/// Eliminates per-element tag-name re-scanning in codegen.
#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum TagType {
    /// Plain HTML element (lowercase, known HTML tag).
    Element = 0,
    /// Vue component (PascalCase, contains dash, or unknown tag).
    Component = 1,
    /// `<slot>` outlet.
    SlotOutlet = 2,
    /// `<template>` wrapper (inside template content, not SFC root).
    Template = 3,
}

impl TagType {
    /// Plain HTML element.
    #[inline(always)]
    pub const fn is_element(self) -> bool {
        matches!(self, TagType::Element)
    }

    /// Vue component (PascalCase, contains dash, or unknown tag).
    #[inline(always)]
    pub const fn is_component(self) -> bool {
        matches!(self, TagType::Component)
    }

    /// `<slot>` outlet.
    #[inline(always)]
    pub const fn is_slot_outlet(self) -> bool {
        matches!(self, TagType::SlotOutlet)
    }

    /// `<template>` wrapper.
    #[inline(always)]
    pub const fn is_template(self) -> bool {
        matches!(self, TagType::Template)
    }

    /// Not a plain HTML element (Component, SlotOutlet, or Template).
    #[inline(always)]
    pub const fn is_special(self) -> bool {
        !self.is_element()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementNode {
    pub tag_open: NodeTag,
    pub tag_close: Option<NodeTag>,

    /// Tag classification for codegen branching (Element / Component / SlotOutlet / Template).
    /// Set by the syntax layer during element open from tag name bytes.
    pub tag_type: TagType,

    /// Whether this element was written as a self-closing tag (`<br />`).
    pub is_self_closing: bool,

    pub props: Vec<NodeProp>,

    /// The content region between open and close tags.
    ///
    /// `None` means the element is self-closing (`<br />`, `<input />`) or was
    /// never opened for content (e.g., `OpenTagEnd` was never reached). This
    /// distinction is important for codegen: self-closing elements must NOT emit
    /// a closing tag or children array, and certain elements (like `<input>`)
    /// are invalid with children in HTML spec. Flattening this into the struct
    /// (e.g., always having `children: Vec<NodeId>`) would lose the ability to
    /// distinguish "no content region" from "empty content region" (`<div></div>`).
    pub content: Option<ElementContent>,

    // helpers to make it easier to handle v-if / v-for / v-slot / v-once without needing to search through props for the relevant directives.
    // Populated by the syntax layer during prop insertion (bytes-only classification).
    // First occurrence wins; duplicates emit a warning diagnostic.
    pub v_condition: Option<ElementNodeCondition>,
    pub v_for: Option<NodeProp>,
    pub v_slot: Option<NodeProp>,
    pub v_once: Option<NodeProp>,
    pub v_ref: Option<NodeProp>,

    /// Element-local prop characteristics for codegen optimization.
    /// Set by the syntax layer during prop classification.
    pub prop_flag: PropFlag,

    /// Pre-computed children characteristics for codegen optimization.
    /// Computed by the builder when the element is closed.
    pub children_flag: ChildrenFlag,

    /// Pre-computed children mode for fast codegen branching.
    pub children_mode: ChildrenMode,

    /// Whether this element and all its descendants are fully static.
    ///
    /// An element is fully static when:
    /// 1. It is a plain HTML element (`tag_type.is_element()`)
    /// 2. No structural directives (`v-if`, `v-for`, `v-slot`, `v-once`, `ref`)
    /// 3. No dynamic props (no bits in `PropFlag::NEEDS_OXC_MASK`)
    /// 4. No interpolation or structural children
    /// 5. All child elements are also fully static (recursive)
    ///
    /// Used by VDOM codegen for static subtree hoisting: fully-static subtrees
    /// are emitted as `_createStaticVNode("<html>", N)` instead of individual
    /// `_createElementVNode()` calls.
    pub is_fully_static: bool,
}

impl ElementNode {
    /// Returns `true` if the element has no attributes and no directives (cached or otherwise).
    #[inline]
    pub fn is_plain(&self) -> bool {
        self.props.is_empty()
            && self.v_condition.is_none()
            && self.v_for.is_none()
            && self.v_slot.is_none()
            && self.v_once.is_none()
            && self.v_ref.is_none()
    }

    // ---- tag_type forwarding ----

    /// Vue component (PascalCase, contains dash, or unknown tag).
    #[inline(always)]
    pub const fn is_component(&self) -> bool {
        self.tag_type.is_component()
    }

    /// `<slot>` outlet.
    #[inline(always)]
    pub const fn is_slot_outlet(&self) -> bool {
        self.tag_type.is_slot_outlet()
    }

    /// `<template>` wrapper.
    #[inline(always)]
    pub const fn is_template(&self) -> bool {
        self.tag_type.is_template()
    }

    // ---- prop_flag forwarding ----

    /// Has any class prop (static or dynamic).
    #[inline(always)]
    pub const fn has_class(&self) -> bool {
        self.prop_flag.has_class()
    }

    /// Has any style prop (static or dynamic).
    #[inline(always)]
    pub const fn has_style(&self) -> bool {
        self.prop_flag.has_style()
    }

    /// Has a `v-bind` or `v-on` spread (no arg).
    #[inline(always)]
    pub const fn has_spread(&self) -> bool {
        self.prop_flag.has_spread()
    }

    /// Has both static and dynamic class — codegen must merge them.
    #[inline(always)]
    pub const fn needs_class_merge(&self) -> bool {
        self.prop_flag.needs_class_merge()
    }

    /// Has both static and dynamic style — codegen must merge them.
    #[inline(always)]
    pub const fn needs_style_merge(&self) -> bool {
        self.prop_flag.needs_style_merge()
    }

    /// Returns `true` if this element requires OXC expression parsing.
    ///
    /// An element needs OXC parsing if it has any structural directive
    /// (`v-if`, `v-for`, `v-slot`, `v-once`) or any directive prop
    /// (`:class`, `@click`, `v-model`, etc.). Elements with only static
    /// attributes (`class="..."`, `style="..."`, `ref="..."`) do not.
    #[inline]
    pub const fn needs_expression_parsing(&self) -> bool {
        self.v_condition.is_some()
            || self.v_for.is_some()
            || self.v_slot.is_some()
            || self.v_once.is_some()
            || self.prop_flag.needs_oxc_parsing()
    }
}

/// Pre-computed children category for branch-friendly codegen.
///
/// Derived from [`ChildrenFlag`] when an element is closed. Codegen can
/// `match` on this enum to pick the optimal rendering strategy without
/// inspecting individual children at emit time.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ChildrenMode {
    /// No children at all (self-closing or empty content).
    Empty,
    /// Only HTML comment children (no visible output).
    CommentsOnly,
    /// Text-only children with no interpolations (fully static).
    TextOnlyStatic,
    /// Text children with at least one `{{ }}` interpolation (needs TEXT patch flag).
    TextOnlyDynamic,
    /// Exactly one element child (no text siblings). Enables direct child codegen.
    SingleElement,
    /// Multiple element children (may need fragment wrapping).
    MultiElement,
    /// Mix of text/interpolation and element children (array wrapping required).
    Mixed,
}

// TODO(new_impl): deferred metadata that cannot be reliably precomputed from
// child-node kinds alone at AST close-time:
// - keyed/unkeyed fragment strategy (requires key analysis on child props)
// - dynamic/stable slot patching semantics (requires slot codegen context)
// - static hoist/stringification eligibility (requires expression constness)
// - whitespace-policy-aware significance (depends on template options)

// ======================== Children flags ========================

/// Named children flag variants for `ElementNode`.
///
/// Each variant is an independent bit that can be combined via
/// [`ChildrenFlag::add`]. The codegen uses these to choose fast paths
/// (text concatenation vs. array wrapping, TEXT patch flag, etc.).
#[repr(u16)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ChildrenFlags {
    /// Has at least one `Text` child.
    HasText = 1,

    /// Has at least one `Interpolation` child (dynamic content).
    HasInterpolation = 1 << 1,

    /// Has at least one `Element` child.
    HasElement = 1 << 2,

    /// Has at least one `Comment` child.
    HasComment = 1 << 3,

    /// Exactly one significant (non-comment) child.
    SingleChild = 1 << 4,

    /// A child element has `v-if`.
    HasVIf = 1 << 5,

    /// A child element has `v-for`.
    HasVFor = 1 << 6,

    /// A child element has `v-slot`.
    HasChildWithVSlot = 1 << 7,

    /// A child element has a dynamic `v-slot:[expr]`.
    HasDynamicSlotChild = 1 << 8,

    /// A child element has a `:key` binding (from PropFlags::HasDynamicKey).
    HasChildWithKey = 1 << 9,
}

impl ChildrenFlags {
    /// Convert a single flag into a [`ChildrenFlag`] wrapper.
    #[inline(always)]
    pub const fn into_flag(self) -> ChildrenFlag {
        ChildrenFlag(self as u16)
    }

    /// Returns the canonical name for debugging.
    #[inline(always)]
    pub const fn name(self) -> &'static str {
        match self {
            ChildrenFlags::HasText => "HAS_TEXT",
            ChildrenFlags::HasInterpolation => "HAS_INTERPOLATION",
            ChildrenFlags::HasElement => "HAS_ELEMENT",
            ChildrenFlags::HasComment => "HAS_COMMENT",
            ChildrenFlags::SingleChild => "SINGLE_CHILD",
            ChildrenFlags::HasVIf => "HAS_V_IF",
            ChildrenFlags::HasVFor => "HAS_V_FOR",
            ChildrenFlags::HasChildWithVSlot => "HAS_CHILD_WITH_V_SLOT",
            ChildrenFlags::HasDynamicSlotChild => "HAS_DYNAMIC_SLOT_CHILD",
            ChildrenFlags::HasChildWithKey => "HAS_CHILD_WITH_KEY",
        }
    }
}

/// Runtime wrapper for children flags on an `ElementNode`.
///
/// Packs combinable boolean flags into a single `u16`.
/// Follows the same enum + wrapper pattern as
/// [`PatchFlags`](crate::utils::vue::patch_flags::PatchFlags) /
/// [`PatchFlag`](crate::utils::vue::patch_flags::PatchFlag).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ChildrenFlag(pub u16);

impl ChildrenFlag {
    pub const TEXT_LIKE_MASK: u16 =
        (ChildrenFlags::HasText as u16) | (ChildrenFlags::HasInterpolation as u16);
    pub const NODE_MASK: u16 = Self::TEXT_LIKE_MASK
        | (ChildrenFlags::HasElement as u16)
        | (ChildrenFlags::HasComment as u16);
    pub const STRUCTURAL_MASK: u16 =
        (ChildrenFlags::HasVIf as u16) | (ChildrenFlags::HasVFor as u16);

    /// An empty flag (no children).
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create from a raw `u16` value.
    #[inline(always)]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns true if any flag is set (i.e., the element has children).
    #[inline(always)]
    pub const fn has_children(self) -> bool {
        self.0 != 0
    }

    /// Bitwise membership check.
    #[inline(always)]
    pub const fn contains(self, flag: ChildrenFlags) -> bool {
        (self.0 & (flag as u16)) != 0
    }

    /// Returns true if any bits from `mask` are set.
    #[inline(always)]
    pub const fn has_any(self, mask: u16) -> bool {
        (self.0 & mask) != 0
    }

    /// Returns true if all bits from `mask` are set.
    #[inline(always)]
    pub const fn has_all(self, mask: u16) -> bool {
        (self.0 & mask) == mask
    }

    /// Alias for [`contains`](Self::contains).
    #[inline(always)]
    pub const fn has(self, flag: ChildrenFlags) -> bool {
        self.contains(flag)
    }

    /// Add a flag.
    #[inline(always)]
    pub const fn add(self, flag: ChildrenFlags) -> Self {
        Self(self.0 | (flag as u16))
    }

    /// Alias for [`add`](Self::add).
    #[inline(always)]
    pub const fn with(self, flag: ChildrenFlags) -> Self {
        self.add(flag)
    }

    /// Remove a flag.
    #[inline(always)]
    pub const fn remove(self, flag: ChildrenFlags) -> Self {
        Self(self.0 & !(flag as u16))
    }

    /// Alias for [`remove`](Self::remove).
    #[inline(always)]
    pub const fn without(self, flag: ChildrenFlags) -> Self {
        self.remove(flag)
    }

    /// Combine two `ChildrenFlag` values (bitwise OR).
    #[inline(always)]
    pub const fn union(self, other: ChildrenFlag) -> Self {
        Self(self.0 | other.0)
    }

    /// Clear all flags.
    #[inline(always)]
    pub const fn clear(self) -> Self {
        Self(0)
    }

    // ---- convenience helpers for codegen ----

    /// All children are text-like (Text / Interpolation) with no elements.
    /// The codegen can use text concatenation instead of array wrapping.
    #[inline(always)]
    pub const fn is_text_only(self) -> bool {
        // Has at least text or interpolation, but no element children.
        let text_like = self.has_any(Self::TEXT_LIKE_MASK);
        let no_elements = !self.contains(ChildrenFlags::HasElement);
        text_like && no_elements
    }

    /// Has dynamic content (at least one interpolation).
    /// When combined with `is_text_only`, the TEXT patch flag must be set.
    #[inline(always)]
    pub const fn has_dynamic(self) -> bool {
        self.contains(ChildrenFlags::HasInterpolation)
    }

    /// Has any structural control flow marker among children.
    #[inline(always)]
    pub const fn has_structural(self) -> bool {
        self.has_any(Self::STRUCTURAL_MASK)
    }

    /// Returns true if children are comments only (no significant children).
    /// This is the flag-level check; [`Self::mode()`] derives [`ChildrenMode::CommentsOnly`]
    /// from the same condition.
    #[inline(always)]
    pub const fn is_comments_only(self) -> bool {
        self.contains(ChildrenFlags::HasComment)
            && !self.has_any(Self::TEXT_LIKE_MASK | (ChildrenFlags::HasElement as u16))
    }

    /// Returns the precomputable children mode for branch-friendly codegen.
    #[inline(always)]
    pub const fn mode(self) -> ChildrenMode {
        if !self.has_children() {
            return ChildrenMode::Empty;
        }

        let has_element = self.contains(ChildrenFlags::HasElement);
        let has_text_like = self.has_any(Self::TEXT_LIKE_MASK);

        if has_element {
            if has_text_like {
                return ChildrenMode::Mixed;
            }
            if self.contains(ChildrenFlags::SingleChild) {
                return ChildrenMode::SingleElement;
            }
            return ChildrenMode::MultiElement;
        }

        if has_text_like {
            if self.has_dynamic() {
                return ChildrenMode::TextOnlyDynamic;
            }
            return ChildrenMode::TextOnlyStatic;
        }

        if self.contains(ChildrenFlags::HasComment) {
            return ChildrenMode::CommentsOnly;
        }

        ChildrenMode::Empty
    }

    /// Children include elements — the codegen must use array wrapping.
    #[inline(always)]
    pub const fn needs_array(self) -> bool {
        self.contains(ChildrenFlags::HasElement)
    }
}

// ======================== Prop flags ========================

/// Element-local prop characteristics for codegen optimization.
///
/// Each variant is an independent bit. Set during bytes-only prop
/// classification in the syntax layer. The codegen reads these to
/// derive PatchFlags and block decisions without re-scanning props.
#[repr(u16)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum PropFlags {
    /// Element has a `:key` or `v-bind:key` binding.
    HasDynamicKey = 1,

    /// Element has a `:class` or `v-bind:class` binding.
    HasDynamicClass = 1 << 1,

    /// Element has a `:style` or `v-bind:style` binding.
    HasDynamicStyle = 1 << 2,

    /// Element has a `ref` attribute.
    HasRef = 1 << 3,

    /// Element has an `@` or `v-on:` event listener.
    HasEventListener = 1 << 4,

    /// Element has a non-built-in `v-*` directive.
    HasCustomDirective = 1 << 5,

    /// Element has a static `class="..."` attribute.
    HasStaticClass = 1 << 6,

    /// Element has a static `style="..."` attribute.
    HasStaticStyle = 1 << 7,

    /// Element has `v-bind="obj"` (no arg — spread).
    HasBindSpread = 1 << 8,

    /// Element has `v-on="obj"` (no arg — spread).
    HasOnSpread = 1 << 9,

    /// Element has `v-model` directive.
    HasModel = 1 << 10,

    /// Element has `v-show` directive.
    HasShow = 1 << 11,

    /// Element has `v-html` directive.
    HasVHtml = 1 << 12,

    /// Element has `v-text` directive.
    HasVText = 1 << 13,

    /// Element has a generic dynamic binding (`:title`, `:id`, `:disabled`, etc.)
    /// — any `v-bind:arg` where arg is not `key`, `class`, or `style`.
    HasDynamicBinding = 1 << 14,
}

impl PropFlags {
    #[inline(always)]
    pub const fn into_flag(self) -> PropFlag {
        PropFlag(self as u16)
    }

    /// Returns the canonical name for debugging.
    #[inline(always)]
    pub const fn name(self) -> &'static str {
        match self {
            PropFlags::HasDynamicKey => "HAS_DYNAMIC_KEY",
            PropFlags::HasDynamicClass => "HAS_DYNAMIC_CLASS",
            PropFlags::HasDynamicStyle => "HAS_DYNAMIC_STYLE",
            PropFlags::HasRef => "HAS_REF",
            PropFlags::HasEventListener => "HAS_EVENT_LISTENER",
            PropFlags::HasCustomDirective => "HAS_CUSTOM_DIRECTIVE",
            PropFlags::HasStaticClass => "HAS_STATIC_CLASS",
            PropFlags::HasStaticStyle => "HAS_STATIC_STYLE",
            PropFlags::HasBindSpread => "HAS_BIND_SPREAD",
            PropFlags::HasOnSpread => "HAS_ON_SPREAD",
            PropFlags::HasModel => "HAS_MODEL",
            PropFlags::HasShow => "HAS_SHOW",
            PropFlags::HasVHtml => "HAS_V_HTML",
            PropFlags::HasVText => "HAS_V_TEXT",
            PropFlags::HasDynamicBinding => "HAS_DYNAMIC_BINDING",
        }
    }
}

/// Runtime wrapper for element-local prop flags.
///
/// Same pattern as [`ChildrenFlag`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PropFlag(pub u16);

impl PropFlag {
    pub const CLASS_MASK: u16 =
        (PropFlags::HasStaticClass as u16) | (PropFlags::HasDynamicClass as u16);
    pub const STYLE_MASK: u16 =
        (PropFlags::HasStaticStyle as u16) | (PropFlags::HasDynamicStyle as u16);
    pub const SPREAD_MASK: u16 =
        (PropFlags::HasBindSpread as u16) | (PropFlags::HasOnSpread as u16);
    pub const DIRECTIVE_MASK: u16 = (PropFlags::HasCustomDirective as u16)
        | (PropFlags::HasModel as u16)
        | (PropFlags::HasShow as u16)
        | (PropFlags::HasVHtml as u16)
        | (PropFlags::HasVText as u16);
    /// Mask for flags set by non-directive attributes (`class="..."`, `style="..."`, `ref="..."`).
    /// These do NOT require OXC expression parsing.
    pub const STATIC_ONLY_MASK: u16 = (PropFlags::HasStaticClass as u16)
        | (PropFlags::HasStaticStyle as u16)
        | (PropFlags::HasRef as u16);
    /// Mask for flags that DO require OXC expression parsing.
    /// Complement of `STATIC_ONLY_MASK` within the 15-bit flag space.
    pub const NEEDS_OXC_MASK: u16 = !Self::STATIC_ONLY_MASK & 0x7FFF;

    /// An empty flag (no prop characteristics).
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create from a raw `u16` value.
    #[inline(always)]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// True if no prop flags are set (element is fully static prop-wise).
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Bitwise membership check.
    #[inline(always)]
    pub const fn contains(self, flag: PropFlags) -> bool {
        (self.0 & (flag as u16)) != 0
    }

    /// Returns true if any bits from `mask` are set.
    #[inline(always)]
    pub const fn has_any(self, mask: u16) -> bool {
        (self.0 & mask) != 0
    }

    /// Returns true if all bits from `mask` are set.
    #[inline(always)]
    pub const fn has_all(self, mask: u16) -> bool {
        (self.0 & mask) == mask
    }

    /// Alias for [`contains`](Self::contains).
    #[inline(always)]
    pub const fn has(self, flag: PropFlags) -> bool {
        self.contains(flag)
    }

    /// Add a flag.
    #[inline(always)]
    pub const fn add(self, flag: PropFlags) -> Self {
        Self(self.0 | (flag as u16))
    }

    /// Alias for [`add`](Self::add).
    #[inline(always)]
    pub const fn with(self, flag: PropFlags) -> Self {
        self.add(flag)
    }

    /// Remove a flag.
    #[inline(always)]
    pub const fn remove(self, flag: PropFlags) -> Self {
        Self(self.0 & !(flag as u16))
    }

    /// Alias for [`remove`](Self::remove).
    #[inline(always)]
    pub const fn without(self, flag: PropFlags) -> Self {
        self.remove(flag)
    }

    /// Combine two `PropFlag` values (bitwise OR).
    #[inline(always)]
    pub const fn union(self, other: PropFlag) -> Self {
        Self(self.0 | other.0)
    }

    /// Clear all flags.
    #[inline(always)]
    pub const fn clear(self) -> Self {
        Self(0)
    }

    // ---- convenience helpers for codegen ----

    /// Has any class prop (static `class="..."` or dynamic `:class`).
    #[inline(always)]
    pub const fn has_class(self) -> bool {
        self.has_any(Self::CLASS_MASK)
    }

    /// Has any style prop (static `style="..."` or dynamic `:style`).
    #[inline(always)]
    pub const fn has_style(self) -> bool {
        self.has_any(Self::STYLE_MASK)
    }

    /// Has a `v-bind` or `v-on` spread (no arg).
    #[inline(always)]
    pub const fn has_spread(self) -> bool {
        self.has_any(Self::SPREAD_MASK)
    }

    /// Has both static and dynamic class — codegen must merge them.
    #[inline(always)]
    pub const fn needs_class_merge(self) -> bool {
        self.has_all(Self::CLASS_MASK)
    }

    /// Has both static and dynamic style — codegen must merge them.
    #[inline(always)]
    pub const fn needs_style_merge(self) -> bool {
        self.has_all(Self::STYLE_MASK)
    }

    /// Has any directive-type prop (custom directive, v-model, v-show, v-html, v-text).
    #[inline(always)]
    pub const fn has_directive(self) -> bool {
        self.has_any(Self::DIRECTIVE_MASK)
    }

    /// Returns `true` if any prop flag indicates a directive that needs OXC parsing.
    ///
    /// `false` for flags set only by non-directive attributes (`class`, `style`, `ref`).
    #[inline(always)]
    pub const fn needs_oxc_parsing(self) -> bool {
        self.has_any(Self::NEEDS_OXC_MASK)
    }
}

/// A v-if / v-else-if / v-else chain discovered among sibling elements.
///
/// Stores the indices into the parent's `children` array for each chain member.
/// Members are consecutive elements with v-if → v-else-if* → v-else?, separated
/// only by whitespace-only text or comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalChain {
    /// Indices into the parent's children vec for each chain member.
    pub member_indices: SmallVec<[usize; 3]>,
}

/// The content region between an element's open and close tags.
///
/// Tracks the byte range of the inner content and the ordered list of child
/// node IDs (elements, text, comments, interpolations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementContent {
    /// Byte offset immediately after the open tag's `>`.
    pub start: u32,
    /// Byte offset of the close tag's `<` (or synthetic end on EOF).
    pub end: u32,
    /// Ordered child node IDs (arena indices).
    /// SmallVec<[NodeId; 4]> avoids heap allocation for ≤4 children, which covers
    /// ~78% of elements in real-world Vue projects (measured across 10k+ .vue files).
    pub children: SmallVec<[NodeId; 4]>,
    /// Pre-computed v-if/v-else-if/v-else chains among direct children.
    /// Each chain records the child indices of its members.
    pub v_if_chains: SmallVec<[ConditionalChain; 1]>,
}

// ======================== Leaf node payloads ========================

/// A raw text segment in the template (e.g., `hello world`).
///
/// Spans `[start..end)` in the source. Adjacent text nodes may exist when
/// separated by entity-decoded segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextNode {
    pub start: u32,
    pub end: u32,
    /// Whether this text segment contains an HTML entity (e.g., `&amp;`).
    /// Codegen may need to emit the decoded value rather than raw source.
    pub is_entity: bool,
    /// Whether this text segment is all whitespace (spaces, tabs, newlines).
    /// For entity text, this checks the decoded content.
    /// Used by chain adjacency scanning to skip formatting whitespace between
    /// `v-if`/`v-else-if`/`v-else` chain members.
    pub is_whitespace_only: bool,
}

/// An HTML comment (`<!-- ... -->`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentNode {
    /// Byte offset of `<` in `<!--`.
    pub start: u32,
    /// Byte offset past `>` in `-->`.
    pub end: u32,
    /// Byte offset of the first content character (after `<!--`).
    pub content_start: u32,
    /// Byte offset past the last content character (before `-->`).
    pub content_end: u32,
}

/// A mustache interpolation (`{{ expr }}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpolationNode {
    /// Byte offset of the opening `{` of `{{`.
    pub start: u32,
    /// Byte offset past the closing `}` of `}}`.
    pub end: u32,
    /// Byte offset of the first expression character (after `{{` + whitespace).
    pub inner_start: u32,
    /// Byte offset past the last expression character (before `}}`).
    pub inner_end: u32,
}

// ====================================================================

/// The complete template AST produced by [`super::builder::TemplateAstBuilder`].
///
/// Contains a flat arena of nodes (`nodes`) and the root template metadata.
/// Use [`NodeId`] indices to access nodes, and the navigation methods
/// (`parent`, `children`, `siblings`, `dfs`) for tree traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAst {
    /// Flat arena of all AST nodes. Indexed by [`NodeId`].
    pub nodes: Vec<AstNode>,
    /// Root template block metadata (tag positions, lang, attributes, children).
    pub root: RootNodeTemplate,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod types_tests;
