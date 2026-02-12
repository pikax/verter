use crate::utils::vue::PatchFlag;

/// Kind of child node — used by close-phase to decide separator strategy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ChildKind {
    Text,
    Interpolation,
    Element,
    Comment,
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
pub(crate) struct ChildInfo {
    /// Start position in source — used for retroactive separator insertion via prepend_left.
    pub start: u32,
    /// What kind of child this is.
    pub kind: ChildKind,
    /// Scope open prefix text (e.g. `"(show) ? "` for v-if, renderList wrapper for v-for).
    /// Emitted by the close phase as part of the separator prepend_left call, ensuring correct
    /// ordering: separator THEN scope prefix THEN child content.
    pub scope_prefix: String,
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
pub(crate) struct DirectiveEntry {
    /// The directive identifier (e.g. `_vModelText`, `_vShow`, `_directive_focus`)
    pub directive: String,
    /// The bound value expression (e.g. `_ctx.msg`), or empty if none.
    pub value: String,
    /// The argument string (e.g. `"arg"`), or empty if none.
    pub arg: String,
    /// Modifier object (e.g. `{ trim: true, number: true }`), or empty if none.
    pub modifiers: String,
}

#[derive(Debug)]
pub(crate) struct StateStack {
    pub id: u32,

    /// Child nodes recorded during processing — close phase uses this to decide
    /// separators (concatenation vs array), TEXT patch flag, etc.
    pub children: Vec<ChildInfo>,

    pub cache_id: Option<u16>,

    // -- Element codegen fields (populated during element open) --
    /// Whether this element is a component (vs native element).
    pub is_component: bool,

    /// Position of `<` of the open tag — used for withDirectives prepend.
    pub open_tag_start: u32,

    /// Position after `>` of the open tag — used as fallback emit position for self-closing.
    pub open_tag_end: u32,

    /// Accumulated patch flag from props processing.
    pub patch_flag: PatchFlag,

    /// Dynamic prop names for the PROPS patch flag.
    pub dynamic_props: Vec<String>,

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
    pub slot_params: Option<String>,
    /// Slot name (from v-slot:name). None → "default".
    /// For dynamic slots (`v-slot:[expr]`), stored as the expression text and
    /// `slot_is_dynamic` is true.
    pub slot_name: Option<String>,
    /// Whether the slot name is dynamic (`v-slot:[expr]`).
    pub slot_is_dynamic: bool,

    // -- Directive fields --
    /// Runtime directives that need `_withDirectives()` wrapping.
    /// Populated during element open for v-model (native), v-show, and custom directives.
    /// The close phase emits `_withDirectives(vnode, [...])`.
    pub runtime_directives: Vec<DirectiveEntry>,
}

impl StateStack {
    pub fn new() -> Self {
        Self {
            id: 0,
            children: Vec::new(),
            cache_id: None,

            is_component: false,
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
            runtime_directives: Vec::new(),
        }
    }

    pub fn create_child(&mut self, element_id: u32) -> Self {
        Self {
            id: element_id,
            children: Vec::new(),
            cache_id: None,

            is_component: false,
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
            runtime_directives: Vec::new(),
        }
    }
}
