//! AST node types and pre-computed codegen metadata.
//!
//! Defines the concrete node types stored in the [`TemplateAst`] arena:
//! [`ElementNode`] (boxed), [`TextNode`], [`CommentNode`], and
//! [`InterpolationNode`]. Each element carries pre-computed
//! [`ChildrenFlag`]/[`PropFlag`] bitsets and a [`ChildrenMode`] enum so
//! codegen can branch without re-scanning children or props.

use smallvec::SmallVec;

use crate::new_impl::{
    syntax::types::RootNodeTemplate,
    types::{NodeId, NodeProp, NodeTag},
};

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

pub const HAS_TEXT: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasText as u16);
pub const HAS_INTERPOLATION: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasInterpolation as u16);
pub const HAS_ELEMENT: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasElement as u16);
pub const HAS_COMMENT: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasComment as u16);
pub const SINGLE_CHILD: ChildrenFlag = ChildrenFlag(ChildrenFlags::SingleChild as u16);
pub const HAS_V_IF: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasVIf as u16);
pub const HAS_V_FOR: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasVFor as u16);
pub const HAS_CHILD_WITH_V_SLOT: ChildrenFlag =
    ChildrenFlag(ChildrenFlags::HasChildWithVSlot as u16);
pub const HAS_DYNAMIC_SLOT_CHILD: ChildrenFlag =
    ChildrenFlag(ChildrenFlags::HasDynamicSlotChild as u16);
pub const HAS_CHILD_WITH_KEY: ChildrenFlag = ChildrenFlag(ChildrenFlags::HasChildWithKey as u16);

// ======================== Prop flags ========================

/// Element-local prop characteristics for codegen optimization.
///
/// Each variant is an independent bit. Set during bytes-only prop
/// classification in the syntax layer. The codegen reads these to
/// derive PatchFlags and block decisions without re-scanning props.
#[repr(u16)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
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
    /// Complement of `STATIC_ONLY_MASK` within the 14-bit flag space.
    pub const NEEDS_OXC_MASK: u16 = !Self::STATIC_ONLY_MASK & 0x3FFF;

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

// ---- top-level PropFlag constants (mirrors ChildrenFlag pattern) ----

pub const P_HAS_DYNAMIC_KEY: PropFlag = PropFlag(PropFlags::HasDynamicKey as u16);
pub const P_HAS_DYNAMIC_CLASS: PropFlag = PropFlag(PropFlags::HasDynamicClass as u16);
pub const P_HAS_DYNAMIC_STYLE: PropFlag = PropFlag(PropFlags::HasDynamicStyle as u16);
pub const P_HAS_REF: PropFlag = PropFlag(PropFlags::HasRef as u16);
pub const P_HAS_EVENT_LISTENER: PropFlag = PropFlag(PropFlags::HasEventListener as u16);
pub const P_HAS_CUSTOM_DIRECTIVE: PropFlag = PropFlag(PropFlags::HasCustomDirective as u16);
pub const P_HAS_STATIC_CLASS: PropFlag = PropFlag(PropFlags::HasStaticClass as u16);
pub const P_HAS_STATIC_STYLE: PropFlag = PropFlag(PropFlags::HasStaticStyle as u16);
pub const P_HAS_BIND_SPREAD: PropFlag = PropFlag(PropFlags::HasBindSpread as u16);
pub const P_HAS_ON_SPREAD: PropFlag = PropFlag(PropFlags::HasOnSpread as u16);
pub const P_HAS_MODEL: PropFlag = PropFlag(PropFlags::HasModel as u16);
pub const P_HAS_SHOW: PropFlag = PropFlag(PropFlags::HasShow as u16);
pub const P_HAS_V_HTML: PropFlag = PropFlag(PropFlags::HasVHtml as u16);
pub const P_HAS_V_TEXT: PropFlag = PropFlag(PropFlags::HasVText as u16);

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
mod children_flag_tests {
    use super::*;

    #[test]
    fn empty_flag() {
        let f = ChildrenFlag::empty();
        assert_eq!(f.0, 0);
        assert!(!f.has_children());
        assert!(!f.is_text_only());
        assert!(!f.has_dynamic());
        assert!(!f.needs_array());
    }

    #[test]
    fn build_flags_fluent() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation);

        assert!(f.has(ChildrenFlags::HasText));
        assert!(f.has(ChildrenFlags::HasInterpolation));
        assert!(!f.has(ChildrenFlags::HasElement));
    }

    #[test]
    fn text_only() {
        let f = ChildrenFlags::HasText.into_flag();
        assert!(f.is_text_only());
        assert!(f.has_children());
        assert!(!f.needs_array());
        assert!(!f.has_dynamic());
    }

    #[test]
    fn text_with_interpolation() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation);
        assert!(f.is_text_only());
        assert!(f.has_dynamic());
        assert!(!f.needs_array());
    }

    #[test]
    fn interpolation_only_is_text_only() {
        let f = ChildrenFlags::HasInterpolation.into_flag();
        assert!(f.is_text_only());
        assert!(f.has_dynamic());
    }

    #[test]
    fn element_children_need_array() {
        let f = ChildrenFlags::HasElement.into_flag();
        assert!(f.needs_array());
        assert!(!f.is_text_only());
    }

    #[test]
    fn mixed_text_and_element() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasElement);
        assert!(!f.is_text_only());
        assert!(f.needs_array());
        assert!(f.has_children());
    }

    #[test]
    fn single_child_flag() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::SingleChild);
        assert!(f.has(ChildrenFlags::SingleChild));
        assert!(f.needs_array());
    }

    #[test]
    fn structural_directive_flags() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::HasVIf)
            .add(ChildrenFlags::HasVFor);
        assert!(f.has(ChildrenFlags::HasVIf));
        assert!(f.has(ChildrenFlags::HasVFor));
    }

    #[test]
    fn remove_flag() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasComment)
            .remove(ChildrenFlags::HasComment);
        assert!(f.has(ChildrenFlags::HasText));
        assert!(!f.has(ChildrenFlags::HasComment));
    }

    #[test]
    fn union_flags() {
        let a = HAS_TEXT.union(HAS_INTERPOLATION);
        let b = HAS_ELEMENT.union(HAS_V_IF);
        let combined = a.union(b);

        assert!(combined.has(ChildrenFlags::HasText));
        assert!(combined.has(ChildrenFlags::HasInterpolation));
        assert!(combined.has(ChildrenFlags::HasElement));
        assert!(combined.has(ChildrenFlags::HasVIf));
    }

    #[test]
    fn constants_work() {
        assert!(HAS_TEXT.has(ChildrenFlags::HasText));
        assert!(HAS_INTERPOLATION.has(ChildrenFlags::HasInterpolation));
        assert!(HAS_ELEMENT.has(ChildrenFlags::HasElement));
        assert!(HAS_COMMENT.has(ChildrenFlags::HasComment));
        assert!(SINGLE_CHILD.has(ChildrenFlags::SingleChild));
        assert!(HAS_V_IF.has(ChildrenFlags::HasVIf));
        assert!(HAS_V_FOR.has(ChildrenFlags::HasVFor));
    }

    #[test]
    fn clear_resets_all() {
        let f = HAS_TEXT.union(HAS_ELEMENT).union(HAS_V_IF).clear();
        assert_eq!(f.0, 0);
        assert!(!f.has_children());
    }

    #[test]
    fn comment_only_has_children_but_not_text_only() {
        let f = ChildrenFlags::HasComment.into_flag();
        assert!(f.has_children());
        assert!(!f.is_text_only());
        assert!(!f.needs_array());
    }

    #[test]
    fn grouped_masks_work() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation)
            .add(ChildrenFlags::HasVIf);

        assert!(f.has_any(ChildrenFlag::TEXT_LIKE_MASK));
        assert!(
            f.has_all((ChildrenFlags::HasText as u16) | (ChildrenFlags::HasInterpolation as u16))
        );
        assert!(f.has_structural());
        assert!(f.has_dynamic());
    }

    #[test]
    fn mode_derivation() {
        assert_eq!(ChildrenFlag::empty().mode(), ChildrenMode::Empty);

        let comments_only = ChildrenFlags::HasComment.into_flag();
        assert_eq!(comments_only.mode(), ChildrenMode::CommentsOnly);

        let text_static = ChildrenFlags::HasText.into_flag();
        assert_eq!(text_static.mode(), ChildrenMode::TextOnlyStatic);

        let text_dynamic = ChildrenFlags::HasInterpolation.into_flag();
        assert_eq!(text_dynamic.mode(), ChildrenMode::TextOnlyDynamic);

        let single_element = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::SingleChild);
        assert_eq!(single_element.mode(), ChildrenMode::SingleElement);

        let multi_element = ChildrenFlags::HasElement.into_flag();
        assert_eq!(multi_element.mode(), ChildrenMode::MultiElement);

        let mixed = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasElement);
        assert_eq!(mixed.mode(), ChildrenMode::Mixed);
    }

    // @ai-generated - Tests SingleElement mode when element + comment (comment not significant)
    #[test]
    fn mode_single_element_with_comment() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::SingleChild)
            .add(ChildrenFlags::HasComment);
        assert_eq!(f.mode(), ChildrenMode::SingleElement);
    }

    // @ai-generated - Tests MultiElement mode with structural flags
    #[test]
    fn mode_multi_element_with_structural() {
        let f = ChildrenFlags::HasElement
            .into_flag()
            .add(ChildrenFlags::HasVIf)
            .add(ChildrenFlags::HasVFor);
        assert_eq!(f.mode(), ChildrenMode::MultiElement);
        assert!(f.has_structural());
    }

    // @ai-generated - Tests TextOnlyDynamic with both HasText and HasInterpolation
    #[test]
    fn mode_text_only_dynamic_both_flags() {
        let f = ChildrenFlags::HasText
            .into_flag()
            .add(ChildrenFlags::HasInterpolation);
        assert_eq!(f.mode(), ChildrenMode::TextOnlyDynamic);
        assert!(f.is_text_only());
        assert!(f.has_dynamic());
    }
}

#[cfg(test)]
mod prop_flag_tests {
    use super::*;

    // @ai-generated - Tests empty PropFlag
    #[test]
    fn empty_flag() {
        let f = PropFlag::empty();
        assert!(f.is_empty());
        assert!(!f.has(PropFlags::HasDynamicKey));
        assert!(!f.has_any(0xFFFF));
    }

    // @ai-generated - Tests adding and checking individual flags
    #[test]
    fn add_and_has_individual_flags() {
        let all_flags = [
            PropFlags::HasDynamicKey,
            PropFlags::HasDynamicClass,
            PropFlags::HasDynamicStyle,
            PropFlags::HasRef,
            PropFlags::HasEventListener,
            PropFlags::HasCustomDirective,
            PropFlags::HasStaticClass,
            PropFlags::HasStaticStyle,
            PropFlags::HasBindSpread,
            PropFlags::HasOnSpread,
            PropFlags::HasModel,
            PropFlags::HasShow,
            PropFlags::HasVHtml,
            PropFlags::HasVText,
        ];

        for &flag in &all_flags {
            let f = PropFlag::empty().add(flag);
            assert!(!f.is_empty(), "flag {:?} should make non-empty", flag);
            assert!(f.has(flag), "flag {:?} should be present", flag);
        }
    }

    // @ai-generated - Tests combining multiple flags
    #[test]
    fn combined_flags() {
        let f = PropFlag::empty()
            .add(PropFlags::HasDynamicKey)
            .add(PropFlags::HasRef)
            .add(PropFlags::HasEventListener);

        assert!(f.has(PropFlags::HasDynamicKey));
        assert!(f.has(PropFlags::HasRef));
        assert!(f.has(PropFlags::HasEventListener));
        assert!(!f.has(PropFlags::HasDynamicClass));
        assert!(!f.has(PropFlags::HasModel));
        assert!(!f.is_empty());
    }

    // @ai-generated - Tests has_any with a mask
    #[test]
    fn has_any_mask() {
        let f = PropFlag::empty().add(PropFlags::HasDynamicStyle);
        let mask = (PropFlags::HasDynamicClass as u16) | (PropFlags::HasDynamicStyle as u16);
        assert!(f.has_any(mask));

        let other_mask = (PropFlags::HasRef as u16) | (PropFlags::HasModel as u16);
        assert!(!f.has_any(other_mask));
    }

    // @ai-generated - Tests into_flag conversion
    #[test]
    fn into_flag_conversion() {
        let f = PropFlags::HasVHtml.into_flag();
        assert!(f.has(PropFlags::HasVHtml));
        assert!(!f.has(PropFlags::HasVText));
    }

    // @ai-generated - Tests new API parity methods: contains, has_all, with, remove, without, union, clear
    #[test]
    fn contains_alias() {
        let f = PropFlag::empty().add(PropFlags::HasRef);
        assert!(f.contains(PropFlags::HasRef));
        assert!(!f.contains(PropFlags::HasModel));
    }

    #[test]
    fn has_all_mask() {
        let f = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(f.has_all(PropFlag::CLASS_MASK));
        assert!(!f.has_all(PropFlag::STYLE_MASK));
    }

    #[test]
    fn with_alias() {
        let f = PropFlag::empty()
            .with(PropFlags::HasDynamicKey)
            .with(PropFlags::HasRef);
        assert!(f.has(PropFlags::HasDynamicKey));
        assert!(f.has(PropFlags::HasRef));
    }

    #[test]
    fn remove_flag() {
        let f = PropFlag::empty()
            .add(PropFlags::HasRef)
            .add(PropFlags::HasModel)
            .remove(PropFlags::HasModel);
        assert!(f.has(PropFlags::HasRef));
        assert!(!f.has(PropFlags::HasModel));
    }

    #[test]
    fn without_alias() {
        let f = PropFlag::empty()
            .add(PropFlags::HasShow)
            .without(PropFlags::HasShow);
        assert!(f.is_empty());
    }

    #[test]
    fn union_flags() {
        let a = P_HAS_DYNAMIC_CLASS.union(P_HAS_STATIC_CLASS);
        let b = P_HAS_REF.union(P_HAS_MODEL);
        let combined = a.union(b);

        assert!(combined.has(PropFlags::HasDynamicClass));
        assert!(combined.has(PropFlags::HasStaticClass));
        assert!(combined.has(PropFlags::HasRef));
        assert!(combined.has(PropFlags::HasModel));
    }

    #[test]
    fn clear_resets_all() {
        let f = P_HAS_DYNAMIC_KEY
            .union(P_HAS_REF)
            .union(P_HAS_MODEL)
            .clear();
        assert_eq!(f.0, 0);
        assert!(f.is_empty());
    }

    #[test]
    fn new_from_raw() {
        let raw = (PropFlags::HasRef as u16) | (PropFlags::HasModel as u16);
        let f = PropFlag::new(raw);
        assert!(f.has(PropFlags::HasRef));
        assert!(f.has(PropFlags::HasModel));
        assert!(!f.has(PropFlags::HasShow));
    }

    // @ai-generated - Tests mask constants
    #[test]
    fn class_mask() {
        let f = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(f.has_any(PropFlag::CLASS_MASK));
        assert!(f.has_all(PropFlag::CLASS_MASK));

        let only_static = PropFlag::empty().add(PropFlags::HasStaticClass);
        assert!(only_static.has_any(PropFlag::CLASS_MASK));
        assert!(!only_static.has_all(PropFlag::CLASS_MASK));
    }

    #[test]
    fn style_mask() {
        let f = PropFlag::empty().add(PropFlags::HasDynamicStyle);
        assert!(f.has_any(PropFlag::STYLE_MASK));
        assert!(!f.has_all(PropFlag::STYLE_MASK));
    }

    #[test]
    fn spread_mask() {
        let f = PropFlag::empty()
            .add(PropFlags::HasBindSpread)
            .add(PropFlags::HasOnSpread);
        assert!(f.has_any(PropFlag::SPREAD_MASK));
        assert!(f.has_all(PropFlag::SPREAD_MASK));
    }

    #[test]
    fn directive_mask() {
        let f = PropFlag::empty().add(PropFlags::HasModel);
        assert!(f.has_any(PropFlag::DIRECTIVE_MASK));

        let f2 = PropFlag::empty().add(PropFlags::HasRef);
        assert!(!f2.has_any(PropFlag::DIRECTIVE_MASK));
    }

    // @ai-generated - Tests convenience helpers
    #[test]
    fn has_class_helper() {
        assert!(!PropFlag::empty().has_class());
        assert!(PropFlag::empty().add(PropFlags::HasStaticClass).has_class());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicClass)
            .has_class());
    }

    #[test]
    fn has_style_helper() {
        assert!(!PropFlag::empty().has_style());
        assert!(PropFlag::empty().add(PropFlags::HasStaticStyle).has_style());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicStyle)
            .has_style());
    }

    #[test]
    fn has_spread_helper() {
        assert!(!PropFlag::empty().has_spread());
        assert!(PropFlag::empty().add(PropFlags::HasBindSpread).has_spread());
        assert!(PropFlag::empty().add(PropFlags::HasOnSpread).has_spread());
    }

    #[test]
    fn needs_class_merge_helper() {
        assert!(!PropFlag::empty().needs_class_merge());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .needs_class_merge());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasDynamicClass)
            .needs_class_merge());
        assert!(PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass)
            .needs_class_merge());
    }

    #[test]
    fn needs_style_merge_helper() {
        assert!(!PropFlag::empty().needs_style_merge());
        assert!(PropFlag::empty()
            .add(PropFlags::HasStaticStyle)
            .add(PropFlags::HasDynamicStyle)
            .needs_style_merge());
    }

    #[test]
    fn has_directive_helper() {
        assert!(!PropFlag::empty().has_directive());
        assert!(PropFlag::empty()
            .add(PropFlags::HasCustomDirective)
            .has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasModel).has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasShow).has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasVHtml).has_directive());
        assert!(PropFlag::empty().add(PropFlags::HasVText).has_directive());
        // Non-directive flags should NOT trigger has_directive
        assert!(!PropFlag::empty().add(PropFlags::HasRef).has_directive());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasEventListener)
            .has_directive());
    }

    // @ai-generated - Tests top-level constants
    #[test]
    fn top_level_constants() {
        assert!(P_HAS_DYNAMIC_KEY.has(PropFlags::HasDynamicKey));
        assert!(P_HAS_DYNAMIC_CLASS.has(PropFlags::HasDynamicClass));
        assert!(P_HAS_DYNAMIC_STYLE.has(PropFlags::HasDynamicStyle));
        assert!(P_HAS_REF.has(PropFlags::HasRef));
        assert!(P_HAS_EVENT_LISTENER.has(PropFlags::HasEventListener));
        assert!(P_HAS_CUSTOM_DIRECTIVE.has(PropFlags::HasCustomDirective));
        assert!(P_HAS_STATIC_CLASS.has(PropFlags::HasStaticClass));
        assert!(P_HAS_STATIC_STYLE.has(PropFlags::HasStaticStyle));
        assert!(P_HAS_BIND_SPREAD.has(PropFlags::HasBindSpread));
        assert!(P_HAS_ON_SPREAD.has(PropFlags::HasOnSpread));
        assert!(P_HAS_MODEL.has(PropFlags::HasModel));
        assert!(P_HAS_SHOW.has(PropFlags::HasShow));
        assert!(P_HAS_V_HTML.has(PropFlags::HasVHtml));
        assert!(P_HAS_V_TEXT.has(PropFlags::HasVText));
    }

    // @ai-generated - Tests PropFlags::name()
    #[test]
    fn prop_flags_name() {
        assert_eq!(PropFlags::HasDynamicKey.name(), "HAS_DYNAMIC_KEY");
        assert_eq!(PropFlags::HasDynamicClass.name(), "HAS_DYNAMIC_CLASS");
        assert_eq!(PropFlags::HasDynamicStyle.name(), "HAS_DYNAMIC_STYLE");
        assert_eq!(PropFlags::HasRef.name(), "HAS_REF");
        assert_eq!(PropFlags::HasEventListener.name(), "HAS_EVENT_LISTENER");
        assert_eq!(PropFlags::HasCustomDirective.name(), "HAS_CUSTOM_DIRECTIVE");
        assert_eq!(PropFlags::HasStaticClass.name(), "HAS_STATIC_CLASS");
        assert_eq!(PropFlags::HasStaticStyle.name(), "HAS_STATIC_STYLE");
        assert_eq!(PropFlags::HasBindSpread.name(), "HAS_BIND_SPREAD");
        assert_eq!(PropFlags::HasOnSpread.name(), "HAS_ON_SPREAD");
        assert_eq!(PropFlags::HasModel.name(), "HAS_MODEL");
        assert_eq!(PropFlags::HasShow.name(), "HAS_SHOW");
        assert_eq!(PropFlags::HasVHtml.name(), "HAS_V_HTML");
        assert_eq!(PropFlags::HasVText.name(), "HAS_V_TEXT");
    }
}

#[cfg(test)]
mod element_node_tests {
    use super::*;
    use crate::new_impl::types::NodeTag;
    use smallvec::SmallVec;

    fn make_plain_element() -> ElementNode {
        ElementNode {
            tag_open: NodeTag {
                start: 0,
                end: 5,
                name_end: 4,
            },
            tag_close: None,
            tag_type: TagType::Element,
            is_self_closing: true,
            props: Vec::new(),
            content: None,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        }
    }

    // @ai-generated - Tests is_plain() with no props or directives
    #[test]
    fn is_plain_empty_element() {
        let el = make_plain_element();
        assert!(el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false when props exist
    #[test]
    fn is_plain_with_props() {
        let mut el = make_plain_element();
        el.props.push(crate::new_impl::types::NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_condition
    #[test]
    fn is_plain_with_v_condition() {
        let mut el = make_plain_element();
        el.v_condition = Some(ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: crate::new_impl::types::NodeProp {
                start: 0,
                name_end: 4,
                is_directive: true,
                arg_start: None,
                arg_end: None,
                is_dynamic: None,
                value_start: None,
                value_end: None,
                modifiers: SmallVec::new(),
            },
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_for
    #[test]
    fn is_plain_with_v_for() {
        let mut el = make_plain_element();
        el.v_for = Some(crate::new_impl::types::NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_slot
    #[test]
    fn is_plain_with_v_slot() {
        let mut el = make_plain_element();
        el.v_slot = Some(crate::new_impl::types::NodeProp {
            start: 0,
            name_end: 6,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_once
    #[test]
    fn is_plain_with_v_once() {
        let mut el = make_plain_element();
        el.v_once = Some(crate::new_impl::types::NodeProp {
            start: 0,
            name_end: 6,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests is_plain() returns false with v_ref
    #[test]
    fn is_plain_with_v_ref() {
        let mut el = make_plain_element();
        el.v_ref = Some(crate::new_impl::types::NodeProp {
            start: 0,
            name_end: 3,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(5),
            value_end: Some(8),
            modifiers: SmallVec::new(),
        });
        assert!(!el.is_plain());
    }

    // @ai-generated - Tests needs_expression_parsing() returns false for empty element
    #[test]
    fn needs_expression_parsing_empty_element() {
        let el = make_plain_element();
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Static class only does not need OXC parsing
    #[test]
    fn needs_expression_parsing_static_class_only() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasStaticClass);
        el.props.push(crate::new_impl::types::NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(12),
            value_end: Some(15),
            modifiers: SmallVec::new(),
        });
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Static class + static style does not need OXC parsing
    #[test]
    fn needs_expression_parsing_static_class_and_style() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasStaticStyle);
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Static ref does not need OXC parsing
    #[test]
    fn needs_expression_parsing_static_ref() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasRef);
        assert!(!el.needs_expression_parsing());
    }

    // @ai-generated - Dynamic class needs OXC parsing
    #[test]
    fn needs_expression_parsing_dynamic_class() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasDynamicClass);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - Event listener needs OXC parsing
    #[test]
    fn needs_expression_parsing_event_listener() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasEventListener);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - v-model needs OXC parsing
    #[test]
    fn needs_expression_parsing_v_model() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty().add(PropFlags::HasModel);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - v-if needs OXC parsing (via cached directive)
    #[test]
    fn needs_expression_parsing_v_if() {
        let mut el = make_plain_element();
        el.v_condition = Some(ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: crate::new_impl::types::NodeProp {
                start: 0,
                name_end: 4,
                is_directive: true,
                arg_start: None,
                arg_end: None,
                is_dynamic: None,
                value_start: Some(6),
                value_end: Some(10),
                modifiers: SmallVec::new(),
            },
        });
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - v-for needs OXC parsing (via cached directive)
    #[test]
    fn needs_expression_parsing_v_for() {
        let mut el = make_plain_element();
        el.v_for = Some(crate::new_impl::types::NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(7),
            value_end: Some(20),
            modifiers: SmallVec::new(),
        });
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - Static class + dynamic class needs OXC parsing
    #[test]
    fn needs_expression_parsing_mixed_static_dynamic() {
        let mut el = make_plain_element();
        el.prop_flag = PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(el.needs_expression_parsing());
    }

    // @ai-generated - Tests PropFlag::needs_oxc_parsing mask exhaustively
    #[test]
    fn prop_flag_needs_oxc_parsing() {
        // Static-only flags should NOT need OXC
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .needs_oxc_parsing());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticStyle)
            .needs_oxc_parsing());
        assert!(!PropFlag::empty().add(PropFlags::HasRef).needs_oxc_parsing());
        assert!(!PropFlag::empty()
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasStaticStyle)
            .add(PropFlags::HasRef)
            .needs_oxc_parsing());

        // Dynamic flags SHOULD need OXC
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicKey)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicClass)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasDynamicStyle)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasEventListener)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasCustomDirective)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasBindSpread)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasOnSpread)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasModel)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasShow)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasVHtml)
            .needs_oxc_parsing());
        assert!(PropFlag::empty()
            .add(PropFlags::HasVText)
            .needs_oxc_parsing());
    }

    // @ai-generated - Tests is_component forwarding
    #[test]
    fn is_component_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.is_component());
        el.tag_type = TagType::Component;
        assert!(el.is_component());
    }

    // @ai-generated - Tests is_slot_outlet forwarding
    #[test]
    fn is_slot_outlet_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.is_slot_outlet());
        el.tag_type = TagType::SlotOutlet;
        assert!(el.is_slot_outlet());
    }

    // @ai-generated - Tests is_template forwarding
    #[test]
    fn is_template_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.is_template());
        el.tag_type = TagType::Template;
        assert!(el.is_template());
    }

    // @ai-generated - Tests has_class forwarding
    #[test]
    fn has_class_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.has_class());
        el.prop_flag = el.prop_flag.add(PropFlags::HasStaticClass);
        assert!(el.has_class());
    }

    // @ai-generated - Tests has_style forwarding
    #[test]
    fn has_style_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.has_style());
        el.prop_flag = el.prop_flag.add(PropFlags::HasDynamicStyle);
        assert!(el.has_style());
    }

    // @ai-generated - Tests has_spread forwarding
    #[test]
    fn has_spread_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.has_spread());
        el.prop_flag = el.prop_flag.add(PropFlags::HasBindSpread);
        assert!(el.has_spread());
    }

    // @ai-generated - Tests needs_class_merge forwarding
    #[test]
    fn needs_class_merge_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.needs_class_merge());
        el.prop_flag = el
            .prop_flag
            .add(PropFlags::HasStaticClass)
            .add(PropFlags::HasDynamicClass);
        assert!(el.needs_class_merge());
    }

    // @ai-generated - Tests needs_style_merge forwarding
    #[test]
    fn needs_style_merge_forwarding() {
        let mut el = make_plain_element();
        assert!(!el.needs_style_merge());
        el.prop_flag = el
            .prop_flag
            .add(PropFlags::HasStaticStyle)
            .add(PropFlags::HasDynamicStyle);
        assert!(el.needs_style_merge());
    }
}

#[cfg(test)]
mod tag_type_tests {
    use super::*;

    // @ai-generated - Tests TagType convenience methods
    #[test]
    fn is_element() {
        assert!(TagType::Element.is_element());
        assert!(!TagType::Component.is_element());
        assert!(!TagType::SlotOutlet.is_element());
        assert!(!TagType::Template.is_element());
    }

    #[test]
    fn is_component() {
        assert!(!TagType::Element.is_component());
        assert!(TagType::Component.is_component());
        assert!(!TagType::SlotOutlet.is_component());
        assert!(!TagType::Template.is_component());
    }

    #[test]
    fn is_slot_outlet() {
        assert!(!TagType::Element.is_slot_outlet());
        assert!(!TagType::Component.is_slot_outlet());
        assert!(TagType::SlotOutlet.is_slot_outlet());
        assert!(!TagType::Template.is_slot_outlet());
    }

    #[test]
    fn is_template() {
        assert!(!TagType::Element.is_template());
        assert!(!TagType::Component.is_template());
        assert!(!TagType::SlotOutlet.is_template());
        assert!(TagType::Template.is_template());
    }

    #[test]
    fn is_special() {
        assert!(!TagType::Element.is_special());
        assert!(TagType::Component.is_special());
        assert!(TagType::SlotOutlet.is_special());
        assert!(TagType::Template.is_special());
    }
}
