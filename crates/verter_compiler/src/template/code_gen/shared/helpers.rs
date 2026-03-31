//! Runtime helper name constants and patch flag constants.
//!
//! These are `&'static str` constants for zero-allocation codegen output.

// ======================== Runtime helpers (VDOM) ========================

pub const CREATE_ELEMENT_VNODE: &str = "_createElementVNode";
pub const CREATE_ELEMENT_BLOCK: &str = "_createElementBlock";
pub const CREATE_VNODE: &str = "_createVNode";
pub const CREATE_BLOCK: &str = "_createBlock";
pub const CREATE_COMMENT_VNODE: &str = "_createCommentVNode";
pub const CREATE_TEXT_VNODE: &str = "_createTextVNode";
pub const OPEN_BLOCK: &str = "_openBlock";
pub const FRAGMENT: &str = "_Fragment";
pub const TO_DISPLAY_STRING: &str = "_toDisplayString";
pub const RENDER_LIST: &str = "_renderList";
pub const WITH_CTX: &str = "_withCtx";
pub const WITH_DIRECTIVES: &str = "_withDirectives";
pub const WITH_MODIFIERS: &str = "_withModifiers";
pub const WITH_KEYS: &str = "_withKeys";
pub const RESOLVE_COMPONENT: &str = "_resolveComponent";
pub const RENDER_SLOT: &str = "_renderSlot";
pub const CREATE_SLOTS: &str = "_createSlots";
pub const MERGE_PROPS: &str = "_mergeProps";
pub const RESOLVE_DIRECTIVE: &str = "_resolveDirective";
pub const SET_BLOCK_TRACKING: &str = "_setBlockTracking";
pub const NORMALIZE_CLASS: &str = "_normalizeClass";
pub const NORMALIZE_STYLE: &str = "_normalizeStyle";
pub const RESOLVE_DYNAMIC_COMPONENT: &str = "_resolveDynamicComponent";
pub const V_MODEL_TEXT: &str = "_vModelText";
pub const V_MODEL_CHECKBOX: &str = "_vModelCheckbox";
pub const V_MODEL_RADIO: &str = "_vModelRadio";
pub const V_MODEL_SELECT: &str = "_vModelSelect";
pub const V_MODEL_DYNAMIC: &str = "_vModelDynamic";
pub const V_SHOW: &str = "_vShow";
pub const CREATE_STATIC_VNODE: &str = "_createStaticVNode";
pub const NORMALIZE_PROPS: &str = "_normalizeProps";
pub const GUARD_REACTIVE_PROPS: &str = "_guardReactiveProps";
pub const TO_HANDLERS: &str = "_toHandlers";

// ======================== Vue built-in components ========================
// These are imported directly from "vue" (e.g., `Suspense as _Suspense`)
// instead of using `_resolveComponent()`.

pub const SUSPENSE: &str = "_Suspense";
pub const TELEPORT: &str = "_Teleport";
pub const KEEP_ALIVE: &str = "_KeepAlive";
pub const BASE_TRANSITION: &str = "_BaseTransition";
pub const TRANSITION: &str = "_Transition";
pub const TRANSITION_GROUP: &str = "_TransitionGroup";

// ======================== Runtime helpers (Vapor) ========================

pub const TEMPLATE: &str = "_template";
pub const TXT: &str = "_txt";
pub const SET_TEXT: &str = "_setText";
pub const SET_CLASS: &str = "_setClass";
pub const SET_STYLE: &str = "_setStyle";
pub const SET_PROP: &str = "_setProp";
pub const SET_ATTR: &str = "_setAttr";
pub const SET_HTML: &str = "_setHtml";
pub const SET_DYNAMIC_PROPS: &str = "_setDynamicProps";
pub const CHILD: &str = "_child";
pub const NEXT: &str = "_next";
pub const RENDER_EFFECT: &str = "_renderEffect";
pub const DELEGATE_EVENTS: &str = "_delegateEvents";
pub const ON: &str = "_on";
pub const CREATE_INVOKER: &str = "_createInvoker";
pub const CREATE_IF: &str = "_createIf";
pub const CREATE_FOR: &str = "_createFor";
pub const CREATE_SLOT: &str = "_createSlot";
pub const CREATE_COMPONENT: &str = "_createComponent";
pub const VAPOR_TO_DISPLAY_STRING: &str = "_toDisplayString";
pub const WITH_MEMO: &str = "_withMemo";
pub const APPLY_V_SHOW: &str = "_applyVShow";
pub const APPLY_TEXT_MODEL: &str = "_applyTextModel";
pub const APPLY_CHECKBOX_MODEL: &str = "_applyCheckboxModel";
pub const APPLY_RADIO_MODEL: &str = "_applyRadioModel";
pub const APPLY_SELECT_MODEL: &str = "_applySelectModel";
pub const CREATE_COMPONENT_WITH_FALLBACK: &str = "_createComponentWithFallback";
pub const CREATE_TEMPLATE_REF_SETTER: &str = "_createTemplateRefSetter";
pub const SET_INSERTION_STATE: &str = "_setInsertionState";

// ======================== Runtime helpers (SSR) ========================
// Imported from "vue/server-renderer"

pub const SSR_RENDER_ATTRS: &str = "_ssrRenderAttrs";
pub const SSR_RENDER_LIST: &str = "_ssrRenderList";
pub const SSR_RENDER_COMPONENT: &str = "_ssrRenderComponent";
pub const SSR_RENDER_SLOT: &str = "_ssrRenderSlot";
pub const SSR_INTERPOLATE: &str = "_ssrInterpolate";
pub const SSR_RENDER_ATTR: &str = "_ssrRenderAttr";
pub const SSR_GET_DYNAMIC_MODEL_PROPS: &str = "_ssrGetDynamicModelProps";
pub const SSR_RENDER_TELEPORT: &str = "_ssrRenderTeleport";
pub const SSR_RENDER_VNODE: &str = "_ssrRenderVNode";
pub const SSR_RENDER_CLASS: &str = "_ssrRenderClass";
pub const SSR_RENDER_STYLE: &str = "_ssrRenderStyle";
pub const SSR_INCLUDE_BOOLEAN_ATTR: &str = "_ssrIncludeBooleanAttr";
pub const SSR_RENDER_SUSPENSE: &str = "_ssrRenderSuspense";
pub const SSR_GET_DIRECTIVE_PROPS: &str = "_ssrGetDirectiveProps";
pub const SSR_LOOSE_CONTAIN: &str = "_ssrLooseContain";
pub const SSR_LOOSE_EQUAL: &str = "_ssrLooseEqual";

// ======================== Helper import bitflags ========================

/// VDOM runtime helper identifier. Each variant is a distinct bit in a `u64`.
#[repr(u64)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum VdomHelper {
    CreateElementVNode = 1,
    CreateElementBlock = 1 << 1,
    CreateVNode = 1 << 2,
    CreateBlock = 1 << 3,
    CreateCommentVNode = 1 << 4,
    CreateTextVNode = 1 << 5,
    OpenBlock = 1 << 6,
    Fragment = 1 << 7,
    ToDisplayString = 1 << 8,
    RenderList = 1 << 9,
    WithCtx = 1 << 10,
    WithDirectives = 1 << 11,
    WithModifiers = 1 << 12,
    WithKeys = 1 << 13,
    ResolveComponent = 1 << 14,
    ResolveDirective = 1 << 15,
    SetBlockTracking = 1 << 16,
    VModelText = 1 << 17,
    VModelCheckbox = 1 << 18,
    VModelRadio = 1 << 19,
    VModelSelect = 1 << 20,
    VModelDynamic = 1 << 21,
    VShow = 1 << 22,
    RenderSlot = 1 << 23,
    CreateSlots = 1 << 24,
    MergeProps = 1 << 25,
    NormalizeClass = 1 << 26,
    NormalizeStyle = 1 << 27,
    ResolveDynamicComponent = 1 << 28,
    CreateStaticVNode = 1 << 29,
    NormalizeProps = 1 << 30,
    GuardReactiveProps = 1 << 31,
    ToHandlers = 1 << 32,
}

impl VdomHelper {
    /// The runtime helper name string (e.g. `"_createElementVNode"`).
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreateElementVNode => CREATE_ELEMENT_VNODE,
            Self::CreateElementBlock => CREATE_ELEMENT_BLOCK,
            Self::CreateVNode => CREATE_VNODE,
            Self::CreateBlock => CREATE_BLOCK,
            Self::CreateCommentVNode => CREATE_COMMENT_VNODE,
            Self::CreateTextVNode => CREATE_TEXT_VNODE,
            Self::OpenBlock => OPEN_BLOCK,
            Self::Fragment => FRAGMENT,
            Self::ToDisplayString => TO_DISPLAY_STRING,
            Self::RenderList => RENDER_LIST,
            Self::WithCtx => WITH_CTX,
            Self::WithDirectives => WITH_DIRECTIVES,
            Self::WithModifiers => WITH_MODIFIERS,
            Self::WithKeys => WITH_KEYS,
            Self::ResolveComponent => RESOLVE_COMPONENT,
            Self::ResolveDirective => RESOLVE_DIRECTIVE,
            Self::SetBlockTracking => SET_BLOCK_TRACKING,
            Self::VModelText => V_MODEL_TEXT,
            Self::VModelCheckbox => V_MODEL_CHECKBOX,
            Self::VModelRadio => V_MODEL_RADIO,
            Self::VModelSelect => V_MODEL_SELECT,
            Self::VModelDynamic => V_MODEL_DYNAMIC,
            Self::VShow => V_SHOW,
            Self::RenderSlot => RENDER_SLOT,
            Self::CreateSlots => CREATE_SLOTS,
            Self::MergeProps => MERGE_PROPS,
            Self::NormalizeClass => NORMALIZE_CLASS,
            Self::NormalizeStyle => NORMALIZE_STYLE,
            Self::ResolveDynamicComponent => RESOLVE_DYNAMIC_COMPONENT,
            Self::CreateStaticVNode => CREATE_STATIC_VNODE,
            Self::NormalizeProps => NORMALIZE_PROPS,
            Self::GuardReactiveProps => GUARD_REACTIVE_PROPS,
            Self::ToHandlers => TO_HANDLERS,
        }
    }
}

/// Ordered lookup table for `VdomHelperFlags::to_imports()`.
const ALL_VDOM: [VdomHelper; 33] = [
    VdomHelper::CreateElementVNode,
    VdomHelper::CreateElementBlock,
    VdomHelper::CreateVNode,
    VdomHelper::CreateBlock,
    VdomHelper::CreateCommentVNode,
    VdomHelper::CreateTextVNode,
    VdomHelper::OpenBlock,
    VdomHelper::Fragment,
    VdomHelper::ToDisplayString,
    VdomHelper::RenderList,
    VdomHelper::WithCtx,
    VdomHelper::WithDirectives,
    VdomHelper::WithModifiers,
    VdomHelper::WithKeys,
    VdomHelper::ResolveComponent,
    VdomHelper::ResolveDirective,
    VdomHelper::SetBlockTracking,
    VdomHelper::VModelText,
    VdomHelper::VModelCheckbox,
    VdomHelper::VModelRadio,
    VdomHelper::VModelSelect,
    VdomHelper::VModelDynamic,
    VdomHelper::VShow,
    VdomHelper::RenderSlot,
    VdomHelper::CreateSlots,
    VdomHelper::MergeProps,
    VdomHelper::NormalizeClass,
    VdomHelper::NormalizeStyle,
    VdomHelper::ResolveDynamicComponent,
    VdomHelper::CreateStaticVNode,
    VdomHelper::NormalizeProps,
    VdomHelper::GuardReactiveProps,
    VdomHelper::ToHandlers,
];

/// Bitflag set of VDOM runtime helpers. Wraps a `u64` with O(1) add/has.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VdomHelperFlags(pub u64);

impl VdomHelperFlags {
    /// Empty set.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// True if no helpers are recorded.
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check membership.
    #[cfg(test)]
    #[inline(always)]
    pub const fn has(self, h: VdomHelper) -> bool {
        (self.0 & (h as u64)) != 0
    }

    /// Add a helper (returns new value).
    #[inline(always)]
    pub const fn add(self, h: VdomHelper) -> Self {
        Self(self.0 | (h as u64))
    }

    /// Merge two flag sets.
    #[cfg(test)]
    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Convert to a `Vec` of helper name strings. Uses `trailing_zeros` scan.
    pub fn to_imports(self) -> Vec<&'static str> {
        if self.0 == 0 {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut bits = self.0;
        while bits != 0 {
            let idx = bits.trailing_zeros() as usize;
            result.push(ALL_VDOM[idx].name());
            bits &= bits - 1; // clear lowest set bit
        }
        result
    }
}

/// Vapor runtime helper identifier. Each variant is a distinct bit in a `u32`.
#[repr(u32)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum VaporHelper {
    Template = 1,
    Txt = 1 << 1,
    SetText = 1 << 2,
    SetClass = 1 << 3,
    SetStyle = 1 << 4,
    SetProp = 1 << 5,
    SetAttr = 1 << 6,
    SetHtml = 1 << 7,
    SetDynamicProps = 1 << 8,
    Child = 1 << 9,
    Next = 1 << 10,
    RenderEffect = 1 << 11,
    DelegateEvents = 1 << 12,
    On = 1 << 13,
    CreateInvoker = 1 << 14,
    CreateIf = 1 << 15,
    CreateFor = 1 << 16,
    CreateSlot = 1 << 17,
    CreateComponent = 1 << 18,
    ToDisplayString = 1 << 19,
    WithMemo = 1 << 20,
    ApplyVShow = 1 << 21,
    ApplyTextModel = 1 << 22,
    ApplyCheckboxModel = 1 << 23,
    ApplyRadioModel = 1 << 24,
    ApplySelectModel = 1 << 25,
    CreateComponentWithFallback = 1 << 26,
    ResolveComponent = 1 << 27,
    CreateTemplateRefSetter = 1 << 28,
    SetInsertionState = 1 << 29,
    WithModifiers = 1 << 30,
    WithKeys = 1u32 << 31,
}

impl VaporHelper {
    /// The runtime helper name string (e.g. `"_template"`).
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Template => TEMPLATE,
            Self::Txt => TXT,
            Self::SetText => SET_TEXT,
            Self::SetClass => SET_CLASS,
            Self::SetStyle => SET_STYLE,
            Self::SetProp => SET_PROP,
            Self::SetAttr => SET_ATTR,
            Self::SetHtml => SET_HTML,
            Self::SetDynamicProps => SET_DYNAMIC_PROPS,
            Self::Child => CHILD,
            Self::Next => NEXT,
            Self::RenderEffect => RENDER_EFFECT,
            Self::DelegateEvents => DELEGATE_EVENTS,
            Self::On => ON,
            Self::CreateInvoker => CREATE_INVOKER,
            Self::CreateIf => CREATE_IF,
            Self::CreateFor => CREATE_FOR,
            Self::CreateSlot => CREATE_SLOT,
            Self::CreateComponent => CREATE_COMPONENT,
            Self::ToDisplayString => VAPOR_TO_DISPLAY_STRING,
            Self::WithMemo => WITH_MEMO,
            Self::ApplyVShow => APPLY_V_SHOW,
            Self::ApplyTextModel => APPLY_TEXT_MODEL,
            Self::ApplyCheckboxModel => APPLY_CHECKBOX_MODEL,
            Self::ApplyRadioModel => APPLY_RADIO_MODEL,
            Self::ApplySelectModel => APPLY_SELECT_MODEL,
            Self::CreateComponentWithFallback => CREATE_COMPONENT_WITH_FALLBACK,
            Self::ResolveComponent => RESOLVE_COMPONENT,
            Self::CreateTemplateRefSetter => CREATE_TEMPLATE_REF_SETTER,
            Self::SetInsertionState => SET_INSERTION_STATE,
            Self::WithModifiers => WITH_MODIFIERS,
            Self::WithKeys => WITH_KEYS,
        }
    }
}

/// Ordered lookup table for `VaporHelperFlags::to_imports()`.
const ALL_VAPOR: [VaporHelper; 32] = [
    VaporHelper::Template,
    VaporHelper::Txt,
    VaporHelper::SetText,
    VaporHelper::SetClass,
    VaporHelper::SetStyle,
    VaporHelper::SetProp,
    VaporHelper::SetAttr,
    VaporHelper::SetHtml,
    VaporHelper::SetDynamicProps,
    VaporHelper::Child,
    VaporHelper::Next,
    VaporHelper::RenderEffect,
    VaporHelper::DelegateEvents,
    VaporHelper::On,
    VaporHelper::CreateInvoker,
    VaporHelper::CreateIf,
    VaporHelper::CreateFor,
    VaporHelper::CreateSlot,
    VaporHelper::CreateComponent,
    VaporHelper::ToDisplayString,
    VaporHelper::WithMemo,
    VaporHelper::ApplyVShow,
    VaporHelper::ApplyTextModel,
    VaporHelper::ApplyCheckboxModel,
    VaporHelper::ApplyRadioModel,
    VaporHelper::ApplySelectModel,
    VaporHelper::CreateComponentWithFallback,
    VaporHelper::ResolveComponent,
    VaporHelper::CreateTemplateRefSetter,
    VaporHelper::SetInsertionState,
    VaporHelper::WithModifiers,
    VaporHelper::WithKeys,
];

/// Bitflag set of Vapor runtime helpers. Wraps a `u32` with O(1) add/has.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VaporHelperFlags(pub u32);

impl VaporHelperFlags {
    /// Empty set.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// True if no helpers are recorded.
    #[cfg(test)]
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check membership.
    #[cfg(test)]
    #[inline(always)]
    pub const fn has(self, h: VaporHelper) -> bool {
        (self.0 & (h as u32)) != 0
    }

    /// Add a helper (returns new value).
    #[inline(always)]
    pub const fn add(self, h: VaporHelper) -> Self {
        Self(self.0 | (h as u32))
    }

    /// Merge two flag sets.
    #[cfg(test)]
    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Convert to a `Vec` of helper name strings. Uses `trailing_zeros` scan.
    pub fn to_imports(self) -> Vec<&'static str> {
        if self.0 == 0 {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut bits = self.0;
        while bits != 0 {
            let idx = bits.trailing_zeros() as usize;
            result.push(ALL_VAPOR[idx].name());
            bits &= bits - 1; // clear lowest set bit
        }
        result
    }
}

// ======================== Built-in component import bitflags ========================

/// Bitflag set of Vue built-in components that need direct import from "vue".
///
/// These components (Suspense, Teleport, KeepAlive, etc.) must be imported
/// directly from "vue" rather than resolved via `_resolveComponent()`.
/// Uses a separate `u8` bitfield to avoid exhausting the `VdomHelper` `u32`.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BuiltinComponentFlags(pub u8);

impl BuiltinComponentFlags {
    const SUSPENSE_BIT: u8 = 1;
    const TELEPORT_BIT: u8 = 1 << 1;
    const KEEP_ALIVE_BIT: u8 = 1 << 2;
    const BASE_TRANSITION_BIT: u8 = 1 << 3;
    const TRANSITION_BIT: u8 = 1 << 4;
    const TRANSITION_GROUP_BIT: u8 = 1 << 5;

    /// Empty set.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// True if no built-in components are recorded.
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Add a built-in component flag.
    #[inline(always)]
    pub const fn add(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }

    /// Convert to a `Vec` of helper name strings for the import statement.
    pub fn to_imports(self) -> Vec<&'static str> {
        if self.0 == 0 {
            return Vec::new();
        }
        let mut result = Vec::new();
        if self.0 & Self::SUSPENSE_BIT != 0 {
            result.push(SUSPENSE);
        }
        if self.0 & Self::TELEPORT_BIT != 0 {
            result.push(TELEPORT);
        }
        if self.0 & Self::KEEP_ALIVE_BIT != 0 {
            result.push(KEEP_ALIVE);
        }
        if self.0 & Self::BASE_TRANSITION_BIT != 0 {
            result.push(BASE_TRANSITION);
        }
        if self.0 & Self::TRANSITION_BIT != 0 {
            result.push(TRANSITION);
        }
        if self.0 & Self::TRANSITION_GROUP_BIT != 0 {
            result.push(TRANSITION_GROUP);
        }
        result
    }
}

// ======================== SSR helper import bitflags ========================

/// SSR runtime helper identifier. Each variant is a distinct bit in a `u32`.
/// These are imported from `"vue/server-renderer"`.
#[repr(u32)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum SsrHelper {
    RenderAttrs = 1,
    RenderList = 1 << 1,
    RenderComponent = 1 << 2,
    RenderSlot = 1 << 3,
    Interpolate = 1 << 4,
    RenderAttr = 1 << 5,
    GetDynamicModelProps = 1 << 6,
    RenderTeleport = 1 << 7,
    RenderVNode = 1 << 8,
    RenderClass = 1 << 9,
    RenderStyle = 1 << 10,
    IncludeBooleanAttr = 1 << 11,
    RenderSuspense = 1 << 12,
    GetDirectiveProps = 1 << 13,
    LooseContain = 1 << 14,
    LooseEqual = 1 << 15,
}

impl SsrHelper {
    /// The runtime helper name string (e.g. `"_ssrRenderAttrs"`).
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RenderAttrs => SSR_RENDER_ATTRS,
            Self::RenderList => SSR_RENDER_LIST,
            Self::RenderComponent => SSR_RENDER_COMPONENT,
            Self::RenderSlot => SSR_RENDER_SLOT,
            Self::Interpolate => SSR_INTERPOLATE,
            Self::RenderAttr => SSR_RENDER_ATTR,
            Self::GetDynamicModelProps => SSR_GET_DYNAMIC_MODEL_PROPS,
            Self::RenderTeleport => SSR_RENDER_TELEPORT,
            Self::RenderVNode => SSR_RENDER_VNODE,
            Self::RenderClass => SSR_RENDER_CLASS,
            Self::RenderStyle => SSR_RENDER_STYLE,
            Self::IncludeBooleanAttr => SSR_INCLUDE_BOOLEAN_ATTR,
            Self::RenderSuspense => SSR_RENDER_SUSPENSE,
            Self::GetDirectiveProps => SSR_GET_DIRECTIVE_PROPS,
            Self::LooseContain => SSR_LOOSE_CONTAIN,
            Self::LooseEqual => SSR_LOOSE_EQUAL,
        }
    }
}

/// Ordered lookup table for `SsrHelperFlags::to_imports()`.
const ALL_SSR: [SsrHelper; 16] = [
    SsrHelper::RenderAttrs,
    SsrHelper::RenderList,
    SsrHelper::RenderComponent,
    SsrHelper::RenderSlot,
    SsrHelper::Interpolate,
    SsrHelper::RenderAttr,
    SsrHelper::GetDynamicModelProps,
    SsrHelper::RenderTeleport,
    SsrHelper::RenderVNode,
    SsrHelper::RenderClass,
    SsrHelper::RenderStyle,
    SsrHelper::IncludeBooleanAttr,
    SsrHelper::RenderSuspense,
    SsrHelper::GetDirectiveProps,
    SsrHelper::LooseContain,
    SsrHelper::LooseEqual,
];

/// Bitflag set of SSR runtime helpers. Wraps a `u32` with O(1) add/has.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SsrHelperFlags(pub u32);

impl SsrHelperFlags {
    /// Empty set.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// True if no helpers are recorded.
    #[inline(always)]
    #[allow(dead_code)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check membership.
    #[cfg(test)]
    #[inline(always)]
    #[allow(dead_code)]
    pub const fn has(self, h: SsrHelper) -> bool {
        (self.0 & (h as u32)) != 0
    }

    /// Add a helper (returns new value).
    #[inline(always)]
    pub const fn add(self, h: SsrHelper) -> Self {
        Self(self.0 | (h as u32))
    }

    /// Convert to a `Vec` of helper name strings. Uses `trailing_zeros` scan.
    pub fn to_imports(self) -> Vec<&'static str> {
        if self.0 == 0 {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut bits = self.0;
        while bits != 0 {
            let idx = bits.trailing_zeros() as usize;
            result.push(ALL_SSR[idx].name());
            bits &= bits - 1; // clear lowest set bit
        }
        result
    }
}

/// Check if a tag name is a Vue built-in component.
///
/// Returns `Some((flag_bit, helper_name))` if the tag is a built-in component,
/// where `flag_bit` is the `BuiltinComponentFlags` bit and `helper_name` is
/// the prefixed import name (e.g., `"_Suspense"`).
///
/// Handles both PascalCase (`<Suspense>`) and lowercase/kebab-case
/// (`<suspense>`, `<keep-alive>`) variants.
pub fn is_builtin_component(tag: &str) -> Option<(u8, &'static str)> {
    match tag {
        "Teleport" | "teleport" => Some((BuiltinComponentFlags::TELEPORT_BIT, TELEPORT)),
        "Suspense" | "suspense" => Some((BuiltinComponentFlags::SUSPENSE_BIT, SUSPENSE)),
        "KeepAlive" | "keep-alive" => Some((BuiltinComponentFlags::KEEP_ALIVE_BIT, KEEP_ALIVE)),
        "BaseTransition" | "base-transition" => {
            Some((BuiltinComponentFlags::BASE_TRANSITION_BIT, BASE_TRANSITION))
        }
        "Transition" | "transition" => Some((BuiltinComponentFlags::TRANSITION_BIT, TRANSITION)),
        "TransitionGroup" | "transition-group" => Some((
            BuiltinComponentFlags::TRANSITION_GROUP_BIT,
            TRANSITION_GROUP,
        )),
        _ => None,
    }
}

// ======================== String case conversion ========================

/// Convert a kebab-case or camelCase string to PascalCase.
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

// ======================== u32 push helper ========================

/// Push a `u32` as decimal digits directly into a string buffer.
/// Uses direct digit computation instead of `write!()` or `to_string()`.
#[inline]
pub fn push_u32(buf: &mut String, n: u32) {
    // Fast paths for common small values (covers most node IDs/indices)
    if n < 10 {
        buf.push((b'0' + n as u8) as char);
        return;
    }
    if n < 100 {
        buf.push((b'0' + (n / 10) as u8) as char);
        buf.push((b'0' + (n % 10) as u8) as char);
        return;
    }
    // General case: compute digits on stack, push as str
    let mut tmp = [0u8; 10]; // u32 max = 4294967295 = 10 digits
    let mut pos = 10;
    let mut val = n;
    while val > 0 {
        pos -= 1;
        tmp[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    // SAFETY: digits are all ASCII (b'0'..=b'9'), valid UTF-8
    debug_assert!(std::str::from_utf8(&tmp[pos..]).is_ok());
    buf.push_str(unsafe { std::str::from_utf8_unchecked(&tmp[pos..]) });
}

// ======================== JS string escaping ========================

/// Quick check whether a string needs any JS string escaping.
///
/// Returns `true` if the string contains `\`, `"`, newline, carriage return,
/// tab, null, or the first byte of U+2028/U+2029 (0xE2).
#[inline]
pub fn needs_js_escaping(s: &str) -> bool {
    s.bytes()
        .any(|b| matches!(b, b'\\' | b'"' | b'\n' | b'\r' | b'\t' | b'\0' | 0xe2) || b < 0x20)
}

/// Append a JS-string-escaped version of `s` to `buf`.
///
/// Escapes: `\` → `\\`, `"` → `\"`, `\n` → `\\n`, `\r` → `\\r`, `\t` → `\\t`,
/// `\0` → `\\0`, U+2028 → `\\u2028`, U+2029 → `\\u2029`, ASCII control → `\\xHH`.
pub fn escape_js_string_into(buf: &mut String, s: &str) {
    // Fast path: ~90% of template strings need no escaping
    if !needs_js_escaping(s) {
        buf.push_str(s);
        return;
    }

    // Bulk-copy optimization: track start of unmodified region
    let bytes = s.as_bytes();
    let mut copy_from = 0;

    for (i, ch) in s.char_indices() {
        let escape = match ch {
            '\\' => "\\\\",
            '"' => "\\\"",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            '\0' => "\\0",
            '\u{2028}' => "\\u2028",
            '\u{2029}' => "\\u2029",
            c if c.is_ascii_control() => {
                // Flush pending unmodified region
                if copy_from < i {
                    buf.push_str(&s[copy_from..i]);
                }
                use std::fmt::Write;
                let _ = write!(buf, "\\x{:02x}", c as u32);
                copy_from = i + 1;
                continue;
            }
            _ => {
                continue;
            }
        };

        // Flush pending unmodified region, then push escape sequence
        if copy_from < i {
            buf.push_str(&s[copy_from..i]);
        }
        buf.push_str(escape);
        copy_from = i + ch.len_utf8();
    }

    // Flush remaining
    if copy_from < bytes.len() {
        buf.push_str(&s[copy_from..]);
    }
}

/// Escape a string for use in a JS string literal.
#[cfg(test)]
pub fn escape_js_string(s: &str) -> String {
    let mut buf = String::with_capacity(s.len() + 8);
    escape_js_string_into(&mut buf, s);
    buf
}

// ======================== HTML entity decoding ========================
// Canonical implementation lives in verter_parser::common::html_entities.
pub use verter_parser::common::html_entities::decode_html_entities_into;
pub use verter_parser::common::html_entities::has_html_entities;

// ======================== Vapor HTML helpers ========================

/// Write a `_template("...", true)` declaration directly into a buffer.
pub fn write_template_declaration_into(
    buf: &mut String,
    idx: u32,
    html: &str,
    is_single_root: bool,
) {
    buf.push_str("const t");
    push_u32(buf, idx);
    buf.push_str(" = _template(\"");
    escape_js_string_into(buf, html);
    buf.push('"');
    if is_single_root {
        buf.push_str(", true");
    }
    buf.push(')');
}

/// Wrap a Vapor HTML string in a `_template("...", true)` call.
#[cfg(test)]
pub fn format_template_declaration(idx: u32, html: &str, is_single_root: bool) -> String {
    let mut buf = String::with_capacity(html.len() + 32);
    write_template_declaration_into(&mut buf, idx, html, is_single_root);
    buf
}

/// Format a `_renderEffect(() => { ... })` wrapper around effect statements.
#[cfg(test)]
pub fn format_render_effect(effects: &[String]) -> String {
    if effects.is_empty() {
        return String::new();
    }
    let mut buf = String::with_capacity(effects.iter().map(|e| e.len()).sum::<usize>() + 32);
    buf.push_str("_renderEffect(() => {\n");
    for effect in effects {
        buf.push_str("  ");
        buf.push_str(effect);
        buf.push('\n');
    }
    buf.push_str("})");
    buf
}

// ======================== Patch flags ========================

/// Indicates that text content is dynamic.
pub const PATCH_TEXT: u32 = 1;
/// Indicates that class binding is dynamic.
pub const PATCH_CLASS: u32 = 2;
/// Indicates that style binding is dynamic.
pub const PATCH_STYLE: u32 = 4;
/// Indicates that named props are dynamic.
pub const PATCH_PROPS: u32 = 8;
/// Indicates that all props are dynamic (spread).
pub const PATCH_FULL_PROPS: u32 = 16;
/// Indicates that the element needs hydration.
pub const PATCH_NEED_HYDRATION: u32 = 32;
/// Stable fragment (children order doesn't change).
pub const PATCH_STABLE_FRAGMENT: u32 = 64;
/// Keyed fragment (v-for with :key).
pub const PATCH_KEYED_FRAGMENT: u32 = 128;
/// Unkeyed fragment (v-for without :key).
pub const PATCH_UNKEYED_FRAGMENT: u32 = 256;
/// Component needs forced patching (has non-optimizable slots).
pub const PATCH_NEED_PATCH: u32 = 512;
/// Component needs force update (has dynamic slots).
pub const PATCH_DYNAMIC_SLOTS: u32 = 1024;

/// Format a patch flag with dev-mode comment.
/// Returns a bump-allocated string like `1 /* TEXT */`.
pub fn format_patch_flag<'a>(
    flag: u32,
    is_production: bool,
    alloc_fn: impl FnOnce(&str) -> &'a str,
) -> &'a str {
    if is_production {
        // Static strings for common flag values — no allocation needed.
        // &'static str coerces to &'a str, bypassing both heap and bump allocation.
        match flag {
            0 => "0",
            1 => "1",
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "5",
            6 => "6",
            7 => "7",
            8 => "8",
            9 => "9",
            10 => "10",
            16 => "16",
            32 => "32",
            64 => "64",
            128 => "128",
            256 => "256",
            512 => "512",
            _ => {
                let s = flag.to_string();
                alloc_fn(&s)
            }
        }
    } else {
        // In dev, add comment with flag names — build directly into a buffer
        // to avoid Vec + join + format overhead.
        use std::fmt::Write;
        let mut buf = String::with_capacity(64);
        let _ = write!(buf, "{flag} /* ");
        let mut first = true;
        const FLAG_NAMES: &[(u32, &str)] = &[
            (PATCH_TEXT, "TEXT"),
            (PATCH_CLASS, "CLASS"),
            (PATCH_STYLE, "STYLE"),
            (PATCH_PROPS, "PROPS"),
            (PATCH_FULL_PROPS, "FULL_PROPS"),
            (PATCH_NEED_HYDRATION, "NEED_HYDRATION"),
            (PATCH_STABLE_FRAGMENT, "STABLE_FRAGMENT"),
            (PATCH_KEYED_FRAGMENT, "KEYED_FRAGMENT"),
            (PATCH_UNKEYED_FRAGMENT, "UNKEYED_FRAGMENT"),
            (PATCH_NEED_PATCH, "NEED_PATCH"),
            (PATCH_DYNAMIC_SLOTS, "DYNAMIC_SLOTS"),
        ];
        for &(mask, name) in FLAG_NAMES {
            if flag & mask != 0 {
                if !first {
                    buf.push_str(", ");
                }
                buf.push_str(name);
                first = false;
            }
        }
        buf.push_str(" */");
        alloc_fn(&buf)
    }
}

// ======================== Debug assertions for source slicing ========================

/// Debug-assert that a source slice `source[start..end]` is in bounds and valid.
///
/// Compiled away in release builds. Call at codegen entry points (enter/leave
/// element, visit_text, etc.) to catch AST position bugs early.
#[inline(always)]
pub fn debug_assert_slice_bounds(source: &str, start: u32, end: u32, context: &str) {
    debug_assert!(
        (end as usize) <= source.len() && (start as usize) <= (end as usize),
        "source slice out of bounds in {context}: start={start}, end={end}, source.len()={}",
        source.len()
    );
}

/// Debug-assert that an element's tag_open name span is valid within source.
///
/// Checks `start+1..name_end` (tag name) is in bounds. Does NOT check
/// `tag_open.end` because the AST builder may leave it at `start`
/// (only the StackElement updates it, not the stored NodeTag).
#[inline(always)]
pub fn debug_assert_element_bounds(
    source: &str,
    tag_open_start: u32,
    _tag_open_end: u32,
    name_end: u32,
) {
    debug_assert!(
        (tag_open_start as usize) < source.len() && (name_end as usize) <= source.len(),
        "element tag name out of bounds: start={tag_open_start}, name_end={name_end}, source.len()={}",
        source.len()
    );
    debug_assert!(
        tag_open_start < name_end,
        "element tag_open.start ({tag_open_start}) must be before name_end ({name_end})"
    );
}

// ======================== Directive helpers (shared by VDOM + Vapor) ========================

use crate::types::NodeProp;

/// Extract the directive value from source using NodeProp value span.
///
/// Returns the source slice between `value_start` and `value_end`,
/// or an empty string if neither is set.
pub fn extract_directive_value<'a>(prop: &NodeProp, source: &'a str) -> &'a str {
    match (prop.value_start, prop.value_end) {
        (Some(start), Some(end)) => &source[start as usize..end as usize],
        _ => "",
    }
}

/// Parse a v-for expression into (params, iterable).
///
/// Examples:
/// - `"item in items"` → `("item", "items")`
/// - `"(item, index) in items"` → `("item, index", "items")`
/// - `"item of items"` → `("item", "items")`
/// - `"(val, key, idx) in obj"` → `("val, key, idx", "obj")`
pub fn parse_v_for_expression(expr: &str) -> (&str, &str) {
    // Find " in " or " of " separator in a single pass
    let separator = if let Some(pos) = find_v_for_separator_any(expr) {
        pos
    } else {
        // Fallback: return entire expression as iterable with empty params
        return ("", expr.trim());
    };

    let params_raw = expr[..separator].trim();
    let iterable = expr[separator + 4..].trim(); // +4 for " in " or " of "

    // Strip surrounding parens from params if present
    let params = params_raw
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(params_raw);

    (params, iterable)
}

/// Find ` in ` or ` of ` separator in a v-for expression, respecting nesting.
/// Searches for both separators in a single pass.
fn find_v_for_separator_any(expr: &str) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let bytes = expr.as_bytes();

    if bytes.len() < 4 {
        return None;
    }

    for i in 0..=bytes.len() - 4 {
        match bytes[i] {
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b'{' => depth_brace += 1,
            b'}' => depth_brace -= 1,
            _ => {}
        }

        if depth_paren == 0
            && depth_bracket == 0
            && depth_brace == 0
            && bytes[i] == b' '
            && (bytes[i + 3] == b' ')
            && ((bytes[i + 1] == b'i' && bytes[i + 2] == b'n')
                || (bytes[i + 1] == b'o' && bytes[i + 2] == b'f'))
        {
            return Some(i);
        }
    }
    None
}

// ======================== Shared event & expression helpers ========================

/// Events that can be delegated (standard DOM event types).
/// Shared between VDOM and Vapor codegen backends.
pub const DELEGATABLE_EVENTS: &[&str] = &[
    "click",
    "dblclick",
    "mousedown",
    "mouseup",
    "mousemove",
    "mouseenter",
    "mouseleave",
    "mouseover",
    "mouseout",
    "keydown",
    "keyup",
    "keypress",
    "input",
    "change",
    "focus",
    "blur",
    "submit",
    "reset",
    "scroll",
    "wheel",
    "touchstart",
    "touchmove",
    "touchend",
    "touchcancel",
    "pointerdown",
    "pointerup",
    "pointermove",
    "pointerenter",
    "pointerleave",
    "pointerover",
    "pointerout",
    "contextmenu",
    "drag",
    "dragstart",
    "dragend",
    "dragenter",
    "dragleave",
    "dragover",
    "drop",
    "focusin",
    "focusout",
];

/// Check whether an expression is a simple member expression (identifier or
/// dot-separated property access like `foo`, `foo.bar`, `_ctx.onClick`).
/// Used to distinguish event handler references from inline handlers.
pub fn is_member_expression(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

// ======================== Template literal escaping ========================

/// Escape a string for use inside a JS template literal (backtick-delimited).
///
/// Only escapes `` ` ``, `\`, and `${` — all extremely rare in HTML.
/// The fast path (no escaping needed) does a single `buf.push_str()`.
pub fn escape_template_literal_into(buf: &mut String, s: &str) {
    let bytes = s.as_bytes();
    // Fast path: scan for characters needing escape
    let needs_escape = bytes.iter().any(|&b| b == b'`' || b == b'\\' || b == b'$');
    if !needs_escape {
        buf.push_str(s);
        return;
    }
    // Slow path: escape with bulk-copy optimization
    buf.reserve(s.len() + 8);
    let mut copy_from = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'`' => {
                buf.push_str(&s[copy_from..i]);
                buf.push_str("\\`");
                copy_from = i + 1;
            }
            b'\\' => {
                buf.push_str(&s[copy_from..i]);
                buf.push_str("\\\\");
                copy_from = i + 1;
            }
            b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                buf.push_str(&s[copy_from..i]);
                buf.push_str("\\${");
                copy_from = i + 2; // skip both $ and {
            }
            _ => {}
        }
    }
    if copy_from < bytes.len() {
        buf.push_str(&s[copy_from..]);
    }
}

/// Build an escaped HTML string with scope ID injected at known AST positions.
///
/// `injection_offsets` are SFC-absolute byte offsets where ` {scope_id}`
/// should be inserted (before `>` or `/>`), sorted ascending.
/// The result is a template-literal-escaped string ready to wrap with backticks.
pub fn build_static_html_with_scope(
    source: &str,
    start: u32,
    end: u32,
    scope_id: &str,
    injection_offsets: &[u32],
    buf: &mut String,
) {
    let start = start as usize;
    let end = end as usize;
    buf.reserve(end - start + injection_offsets.len() * (scope_id.len() + 1) + 8);

    if injection_offsets.is_empty() {
        // No scope ID injection needed — just escape
        escape_template_literal_into(buf, &source[start..end]);
        return;
    }

    // Copy source segments between injection points, inserting ` {scope_id}` at each
    let mut pos = start;
    for &offset in injection_offsets {
        let offset = offset as usize;
        debug_assert!(
            offset >= pos && offset <= end,
            "injection offset {offset} out of range [{pos}, {end}]"
        );
        // Escape and copy segment before injection point
        escape_template_literal_into(buf, &source[pos..offset]);
        // Insert scope ID
        buf.push(' ');
        buf.push_str(scope_id);
        pos = offset;
    }
    // Copy remaining segment after last injection
    if pos < end {
        escape_template_literal_into(buf, &source[pos..end]);
    }
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
