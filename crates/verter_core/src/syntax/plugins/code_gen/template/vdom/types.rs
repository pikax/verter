use crate::utils::vue::PatchFlag;

/// Kind of child node — used by close-phase to decide separator strategy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ChildKind {
    Text,
    Interpolation,
    Element,
    Comment,
    /// Whitespace-only text containing newlines — deferred to the close phase.
    ///
    /// Vue's condense mode removes this when:
    /// - It's the first or last child, OR
    /// - Both adjacent siblings are elements or comments, OR
    /// - One adjacent is a comment and the other is an element.
    /// - Both adjacent are elements (the newline triggers removal).
    ///
    /// Between an element and an interpolation, it becomes a single space.
    WhitespaceNewline,
    /// Whitespace-only text WITHOUT newlines — also deferred to the close phase.
    ///
    /// Vue's condense mode removes this when:
    /// - It's the first or last child, OR
    /// - Both adjacent are comments, OR
    /// - One adjacent is a comment and the other is an element.
    ///
    /// Between elements (without newline), or involving interpolation: kept as space.
    WhitespaceSpace,
}

impl ChildKind {
    /// Content prefix that the close phase must prepend for this child kind.
    ///
    /// Text children need an opening `"` quote; interpolation needs `_toDisplayString`.
    /// Elements and comments use `overwrite` for their own prefix, so no extra prefix is needed.
    ///
    /// This exists because `prepend_left` at the same position is FIFO — if the child
    /// handler and close phase both call `prepend_left` at the same position, the child
    /// handler's content appears first. So the close phase must emit the child's content
    /// prefix as part of its own single `prepend_left` call.
    pub(crate) fn content_prefix(&self) -> &'static str {
        match self {
            ChildKind::Text => "\"",
            ChildKind::Interpolation => "_toDisplayString",
            ChildKind::Element | ChildKind::Comment => "",
            // Should never reach close phase — resolve_whitespace_candidates converts/removes these.
            ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace => "\"",
        }
    }
}

/// Recorded during child processing for close-phase separator decisions.
///
/// # Ordering Invariant
///
/// The close phase combines `scope_prefix` + `content_prefix()` + separator
/// into a single `prepend_left(start, ...)` call. This is the ONLY
/// `prepend_left` permitted at `self.start`. If any other code calls
/// `prepend_left` at the same position, the FIFO ordering will break.
#[derive(Debug)]
pub(crate) struct ChildInfo<'alloc> {
    /// Start position in source — used for retroactive separator insertion via prepend_left.
    pub start: u32,
    /// End position in source — used by the close phase for _createTextVNode closing.
    /// For text: position after text content (where closing `"` is appended).
    /// For interpolation: position after `}}` (where closing `)` overwrites to).
    /// For elements/comments: 0 (unused in _createTextVNode wrapping).
    pub end: u32,
    /// What kind of child this is.
    pub kind: ChildKind,
    /// Scope open prefix text (e.g. `"(show) ? "` for v-if, renderList wrapper for v-for).
    /// Emitted by the close phase as part of the separator prepend_left call, ensuring correct
    /// ordering: separator THEN scope prefix THEN child content.
    pub scope_prefix: &'alloc str,
    /// Whether this child is a `<template #name>` that defines a named slot.
    /// Named slot children emit their own `name: _withCtx(...)` string and don't
    /// need wrapping by the parent. Non-named-slot children inside a component with
    /// named slots must be wrapped in `default: _withCtx(() => [...])`.
    pub is_named_slot: bool,
}

/// Stored scope close token — emitted after the element VNode call closes.
#[derive(Debug)]
pub(crate) enum ScopeClose {
    /// `) : _createCommentVNode("v-if", true)`
    IfTernary,
    /// `) : _createCommentVNode("v-if", true)`
    ElseIfTernary,
    /// `)`
    Else,
    /// `}), 128 /* KEYED_FRAGMENT */))` or `}), 256 /* UNKEYED_FRAGMENT */))`
    /// `is_keyed` is true when the v-for element has a `:key` prop.
    For { is_keyed: bool },
}

/// A runtime directive entry for `_withDirectives(vnode, [[dir, val, arg, mods], ...])`.
///
/// Each entry corresponds to one directive on the element.
#[derive(Debug)]
pub(crate) struct DirectiveEntry<'alloc> {
    /// The directive identifier (e.g. `_vModelText`, `_vShow`, `_directive_focus`)
    pub directive: &'alloc str,
    /// The bound value expression (e.g. `_ctx.msg`), or empty if none.
    pub value: &'alloc str,
    /// The argument string (e.g. `"arg"`), or empty if none.
    pub arg: &'alloc str,
    /// Modifier object (e.g. `{ trim: true, number: true }`), or empty if none.
    pub modifiers: &'alloc str,
}

#[derive(Debug)]
pub(crate) struct StateStack<'alloc> {
    pub id: u32,

    /// Child nodes recorded during processing — close phase uses this to decide
    /// separators (concatenation vs array), TEXT patch flag, etc.
    pub children: Vec<ChildInfo<'alloc>>,

    pub cache_id: Option<u16>,

    // -- Element codegen fields (populated during element open) --
    /// Whether this element is a component (vs native element).
    pub is_component: bool,

    /// Whether this element is a `<slot/>` outlet (renders via `_renderSlot`).
    pub is_slot_outlet: bool,

    /// Position of `<` of the open tag — used for withDirectives prepend.
    pub open_tag_start: u32,

    /// Position after `>` of the open tag — used as fallback emit position for self-closing.
    pub open_tag_end: u32,

    /// Accumulated patch flag from props processing.
    pub patch_flag: PatchFlag,

    /// Dynamic prop names for the PROPS patch flag.
    pub dynamic_props: Vec<&'alloc str>,

    /// Scope closes to emit after the element VNode call.
    pub pending_scope_closes: Vec<ScopeClose>,

    /// Whether this element is a block root (uses _openBlock + _createElementBlock).
    /// True for: direct children of <template>, v-if/v-for branch elements.
    pub is_block_root: bool,

    /// Pending v-if/v-else-if close positions where comment fallback should be emitted.
    ///
    /// # Deferred Emission Contract
    ///
    /// When `process_scope_closes()` encounters `ScopeClose::IfTernary` or
    /// `ScopeClose::ElseIfTernary`, it appends ` : ` at `close_pos` but does NOT
    /// emit `_createCommentVNode(...)`. Instead, the caller pushes `close_pos`
    /// here. Two things can happen next:
    ///
    /// 1. A v-else-if/v-else sibling follows: `handle_element_start()` pops the
    ///    last entry (consumed by the else branch).
    /// 2. No else follows: the parent's `handle_element_closed()` or
    ///    `handle_template_closed()` emits `_createCommentVNode("v-if", true)`
    ///    at each remaining position.
    ///
    /// This two-phase approach is necessary because at the time an element with
    /// v-if closes, we don't yet know whether v-else-if/v-else follows.
    pub pending_vif_fallbacks: Vec<u32>,

    /// Counter for v-if branch keys within this parent's scope.
    /// Each new v-if chain starts at 0, incremented for each v-if/v-else-if/v-else branch.
    pub vif_key_counter: u32,

    /// v-if branch key to inject into this element's props (set by directives module).
    /// When Some(N), the element gets `{ key: N }` injected into its props.
    pub vif_branch_key: Option<u32>,

    // -- Static hoisting fields --
    /// Whether all props on this element are static (Value, ClassValue, StyleValue only).
    /// Used to determine if props can be hoisted.
    pub has_all_static_props: bool,

    /// Whether this element has any props at all.
    pub has_props: bool,

    // -- Slot fields --
    /// Slot parameters text (from v-slot="params"). When Some, component children
    /// are wrapped in `{ slotName: _withCtx((params) => [...]), _: 1 }`.
    /// None means no v-slot directive → children are passed as normal args.
    pub slot_params: Option<&'alloc str>,
    /// Slot name (from v-slot:name). None → "default".
    /// For dynamic slots (`v-slot:[expr]`), stored as the expression text and
    /// `slot_is_dynamic` is true.
    pub slot_name: Option<&'alloc str>,
    /// Whether the slot name is dynamic (`v-slot:[expr]`).
    pub slot_is_dynamic: bool,

    /// Whether this `<template>` element defines a named slot inside a component parent.
    /// When true, the template does NOT generate a VNode — its children become a slot
    /// entry in the parent component's slots object.
    pub is_named_slot_template: bool,

    /// v-if scope prefix for named slot templates (e.g. `"(!isMobile) ? "`).
    /// When a `<template v-if="cond" #name>` is encountered, the v-if condition
    /// must be emitted INSIDE the `_withCtx(() => [...])` callback, not wrapping
    /// the slot key-value pair. This field stores the scope prefix so the close
    /// phase can incorporate it inside the callback.
    pub named_slot_vif_prefix: &'alloc str,

    /// Whether this named slot template's scope closes should be handled internally
    /// (inside the _withCtx callback) rather than externally by the parent.
    /// Set to true when `named_slot_vif_prefix` is non-empty.
    pub named_slot_has_vif: bool,

    /// Whether this component has `<template #name>` children defining named slots.
    /// When true, children are wrapped in `{ ... _: 1 }` instead of `[...]`.
    pub has_named_slot_children: bool,

    /// Whether any named slot child uses a dynamic name (`v-slot:[expr]`).
    /// Determines slot flag: false → `_: 1` (STABLE), true → `_: 2` (DYNAMIC).
    pub any_dynamic_slots: bool,

    // -- Directive fields --
    /// Runtime directives that need `_withDirectives()` wrapping.
    /// Populated during element open for v-model (native), v-show, and custom directives.
    /// The close phase emits `_withDirectives(vnode, [...])`.
    pub runtime_directives: Vec<DirectiveEntry<'alloc>>,
}

impl Default for StateStack<'_> {
    fn default() -> Self {
        Self {
            id: 0,
            children: Vec::new(),
            cache_id: None,

            is_component: false,
            is_slot_outlet: false,
            open_tag_start: 0,
            open_tag_end: 0,
            patch_flag: PatchFlag::empty(),
            dynamic_props: Vec::new(),
            pending_scope_closes: Vec::new(),
            is_block_root: false,
            pending_vif_fallbacks: Vec::new(),
            vif_key_counter: 0,
            vif_branch_key: None,
            has_all_static_props: true,
            has_props: false,

            slot_params: None,
            slot_name: None,
            slot_is_dynamic: false,
            is_named_slot_template: false,
            named_slot_vif_prefix: "",
            named_slot_has_vif: false,
            has_named_slot_children: false,
            any_dynamic_slots: false,
            runtime_directives: Vec::new(),
        }
    }
}

impl StateStack<'_> {
    /// Reset all fields to defaults while retaining Vec capacities.
    ///
    /// Used by the StateStack pool to avoid re-allocating inner Vecs.
    pub fn reset(&mut self, element_id: u32) {
        self.id = element_id;
        self.children.clear();
        self.cache_id = None;
        self.is_component = false;
        self.is_slot_outlet = false;
        self.open_tag_start = 0;
        self.open_tag_end = 0;
        self.patch_flag = PatchFlag::empty();
        self.dynamic_props.clear();
        self.pending_scope_closes.clear();
        self.is_block_root = false;
        self.pending_vif_fallbacks.clear();
        self.vif_key_counter = 0;
        self.vif_branch_key = None;
        self.has_all_static_props = true;
        self.has_props = false;
        self.slot_params = None;
        self.slot_name = None;
        self.slot_is_dynamic = false;
        self.is_named_slot_template = false;
        self.named_slot_vif_prefix = "";
        self.named_slot_has_vif = false;
        self.has_named_slot_children = false;
        self.any_dynamic_slots = false;
        self.runtime_directives.clear();
    }
}
