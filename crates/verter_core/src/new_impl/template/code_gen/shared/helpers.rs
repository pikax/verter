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
pub const RESOLVE_DIRECTIVE: &str = "_resolveDirective";
pub const SET_BLOCK_TRACKING: &str = "_setBlockTracking";
pub const V_MODEL_TEXT: &str = "_vModelText";
pub const V_MODEL_CHECKBOX: &str = "_vModelCheckbox";
pub const V_MODEL_RADIO: &str = "_vModelRadio";
pub const V_MODEL_SELECT: &str = "_vModelSelect";
pub const V_MODEL_DYNAMIC: &str = "_vModelDynamic";
pub const V_SHOW: &str = "_vShow";

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
pub const SET_TEMPLATE_REF: &str = "_setTemplateRef";
pub const WITH_VAPOR_CTX: &str = "_withVaporCtx";
pub const SET_INSERTION_STATE: &str = "_setInsertionState";

// ======================== Helper import bitflags ========================

/// VDOM runtime helper identifier. Each variant is a distinct bit in a `u32`.
#[repr(u32)]
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
        }
    }
}

/// Ordered lookup table for `VdomHelperFlags::to_imports()`.
const ALL_VDOM: [VdomHelper; 23] = [
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
];

/// Bitflag set of VDOM runtime helpers. Wraps a `u32` with O(1) add/has.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VdomHelperFlags(pub u32);

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
    #[inline(always)]
    pub const fn has(self, h: VdomHelper) -> bool {
        (self.0 & (h as u32)) != 0
    }

    /// Add a helper (returns new value).
    #[inline(always)]
    pub const fn add(self, h: VdomHelper) -> Self {
        Self(self.0 | (h as u32))
    }

    /// Merge two flag sets.
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
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check membership.
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

// ======================== u32 push helper ========================

/// Push a `u32` as decimal digits directly into a string buffer.
/// Avoids the intermediate `String` allocation of `n.to_string()`.
#[inline]
pub fn push_u32(buf: &mut String, n: u32) {
    use std::fmt::Write;
    let _ = write!(buf, "{}", n);
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

/// Escape a string for use in a JS string literal. Returns new `String` only if
/// escaping is needed; the caller should check [`needs_js_escaping`] first to
/// avoid allocation for the common case.
pub fn escape_js_string(s: &str) -> String {
    let mut buf = String::with_capacity(s.len() + 8);
    escape_js_string_into(&mut buf, s);
    buf
}

// ======================== Vapor HTML helpers ========================

/// Wrap a Vapor HTML string in a `_template("...", true)` call.
///
/// The `is_single_root` parameter adds `, true` for single-root templates
/// which enables the optimization of using `firstChild` directly.
pub fn format_template_declaration(idx: u32, html: &str, is_single_root: bool) -> String {
    let mut buf = String::with_capacity(html.len() + 32);
    write_template_declaration_into(&mut buf, idx, html, is_single_root);
    buf
}

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

/// Format a `_renderEffect(() => { ... })` wrapper around effect statements.
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
/// Component needs force update (has dynamic slots).
pub const PATCH_DYNAMIC_SLOTS: u32 = 512;

/// Format a patch flag with dev-mode comment.
/// Returns a bump-allocated string like `1 /* TEXT */`.
pub fn format_patch_flag<'a>(
    flag: u32,
    is_production: bool,
    alloc_fn: impl FnOnce(&str) -> &'a str,
) -> &'a str {
    if is_production {
        // In production, just the number
        let s = flag.to_string();
        alloc_fn(&s)
    } else {
        // In dev, add comment with flag names
        let mut names = Vec::new();
        if flag & PATCH_TEXT != 0 {
            names.push("TEXT");
        }
        if flag & PATCH_CLASS != 0 {
            names.push("CLASS");
        }
        if flag & PATCH_STYLE != 0 {
            names.push("STYLE");
        }
        if flag & PATCH_PROPS != 0 {
            names.push("PROPS");
        }
        if flag & PATCH_FULL_PROPS != 0 {
            names.push("FULL_PROPS");
        }
        if flag & PATCH_NEED_HYDRATION != 0 {
            names.push("NEED_HYDRATION");
        }
        if flag & PATCH_STABLE_FRAGMENT != 0 {
            names.push("STABLE_FRAGMENT");
        }
        if flag & PATCH_KEYED_FRAGMENT != 0 {
            names.push("KEYED_FRAGMENT");
        }
        if flag & PATCH_UNKEYED_FRAGMENT != 0 {
            names.push("UNKEYED_FRAGMENT");
        }
        if flag & PATCH_DYNAMIC_SLOTS != 0 {
            names.push("DYNAMIC_SLOTS");
        }
        let comment = names.join(", ");
        let s = format!("{flag} /* {comment} */");
        alloc_fn(&s)
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

use crate::new_impl::types::NodeProp;

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
    // Find " in " or " of " separator
    let separator = if let Some(pos) = find_v_for_separator(expr, " in ") {
        pos
    } else if let Some(pos) = find_v_for_separator(expr, " of ") {
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
pub fn find_v_for_separator(expr: &str, sep: &str) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let bytes = expr.as_bytes();
    let sep_bytes = sep.as_bytes();

    if bytes.len() < sep_bytes.len() {
        return None;
    }

    for i in 0..=bytes.len() - sep_bytes.len() {
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
            && bytes[i..].starts_with(sep_bytes)
        {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::SmallVec;

    // ==================== parse_v_for_expression ====================

    #[test]
    fn shared_parse_v_for_simple() {
        let (params, iterable) = parse_v_for_expression("item in items");
        assert_eq!(params, "item");
        assert_eq!(iterable, "items");
    }

    #[test]
    fn shared_parse_v_for_destructured() {
        let (params, iterable) = parse_v_for_expression("(item, index) in items");
        assert_eq!(params, "item, index");
        assert_eq!(iterable, "items");
    }

    #[test]
    fn shared_parse_v_for_of() {
        let (params, iterable) = parse_v_for_expression("item of items");
        assert_eq!(params, "item");
        assert_eq!(iterable, "items");
    }

    #[test]
    fn shared_parse_v_for_complex_iterable() {
        let (params, iterable) = parse_v_for_expression("item in items.filter(x => x.active)");
        assert_eq!(params, "item");
        assert_eq!(iterable, "items.filter(x => x.active)");
    }

    // ==================== extract_directive_value ====================

    #[test]
    fn shared_extract_directive_value_with_span() {
        let prop = NodeProp {
            start: 0,
            name_end: 4,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(6),
            value_end: Some(10),
            modifiers: SmallVec::new(),
        };
        assert_eq!(extract_directive_value(&prop, "v-if=\"show\""), "show");
    }

    #[test]
    fn shared_extract_directive_value_no_span() {
        let prop = NodeProp {
            start: 0,
            name_end: 4,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        };
        assert_eq!(extract_directive_value(&prop, "v-else"), "");
    }

    // ==================== Patch flags ====================

    #[test]
    fn format_patch_flag_production() {
        let result = format_patch_flag(1, true, |s| {
            // Simulate allocation — in tests we just leak
            Box::leak(s.to_string().into_boxed_str())
        });
        assert_eq!(result, "1");
    }

    #[test]
    fn format_patch_flag_dev_single() {
        let result = format_patch_flag(1, false, |s| Box::leak(s.to_string().into_boxed_str()));
        assert_eq!(result, "1 /* TEXT */");
    }

    #[test]
    fn format_patch_flag_dev_combined() {
        let result = format_patch_flag(PATCH_TEXT | PATCH_PROPS, false, |s| {
            Box::leak(s.to_string().into_boxed_str())
        });
        assert_eq!(result, "9 /* TEXT, PROPS */");
    }

    #[test]
    fn format_patch_flag_dev_class_style() {
        let result = format_patch_flag(PATCH_CLASS | PATCH_STYLE, false, |s| {
            Box::leak(s.to_string().into_boxed_str())
        });
        assert_eq!(result, "6 /* CLASS, STYLE */");
    }

    // ==================== JS string escaping ====================

    #[test]
    fn needs_js_escaping_plain_text() {
        assert!(!needs_js_escaping("hello world"));
    }

    #[test]
    fn needs_js_escaping_with_quote() {
        assert!(needs_js_escaping("say \"hi\""));
    }

    #[test]
    fn needs_js_escaping_with_backslash() {
        assert!(needs_js_escaping("path\\to\\file"));
    }

    #[test]
    fn needs_js_escaping_with_newline() {
        assert!(needs_js_escaping("line1\nline2"));
    }

    #[test]
    fn needs_js_escaping_with_control_char() {
        assert!(needs_js_escaping("bell\x07"));
    }

    #[test]
    fn escape_js_string_no_escaping() {
        assert_eq!(escape_js_string("hello"), "hello");
    }

    #[test]
    fn escape_js_string_quotes_and_backslash() {
        assert_eq!(escape_js_string(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn escape_js_string_newlines() {
        assert_eq!(escape_js_string("a\nb\rc"), "a\\nb\\rc");
    }

    #[test]
    fn escape_js_string_null_and_tab() {
        assert_eq!(escape_js_string("a\0b\tc"), "a\\0b\\tc");
    }

    #[test]
    fn escape_js_string_unicode_line_separators() {
        assert_eq!(escape_js_string("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
    }

    #[test]
    fn escape_js_string_ascii_control() {
        assert_eq!(escape_js_string("a\x07b"), "a\\x07b");
    }

    #[test]
    fn escape_js_string_into_appends() {
        let mut buf = String::from("prefix:");
        escape_js_string_into(&mut buf, "a\"b");
        assert_eq!(buf, "prefix:a\\\"b");
    }

    // ==================== Vapor HTML helpers ====================

    #[test]
    fn format_template_declaration_single_root() {
        let result = format_template_declaration(0, "<div>hello</div>", true);
        assert_eq!(result, "const t0 = _template(\"<div>hello</div>\", true)");
    }

    #[test]
    fn format_template_declaration_multi_root() {
        let result = format_template_declaration(1, "<div>text</div>", false);
        assert_eq!(result, "const t1 = _template(\"<div>text</div>\")");
    }

    #[test]
    fn format_template_declaration_escapes_quotes() {
        let result = format_template_declaration(0, r#"<div class="foo">text</div>"#, true);
        assert_eq!(
            result,
            r#"const t0 = _template("<div class=\"foo\">text</div>", true)"#
        );
    }

    #[test]
    fn format_render_effect_single() {
        let result = format_render_effect(&["_setClass(n0, _ctx.cls)".to_string()]);
        assert_eq!(
            result,
            "_renderEffect(() => {\n  _setClass(n0, _ctx.cls)\n})"
        );
    }

    #[test]
    fn format_render_effect_multiple() {
        let result = format_render_effect(&[
            "_setText(x0, _toDisplayString(_ctx.msg))".to_string(),
            "_setClass(n0, _ctx.cls)".to_string(),
        ]);
        assert_eq!(
            result,
            "_renderEffect(() => {\n  _setText(x0, _toDisplayString(_ctx.msg))\n  _setClass(n0, _ctx.cls)\n})"
        );
    }

    #[test]
    fn format_render_effect_empty() {
        let result = format_render_effect(&[]);
        assert_eq!(result, "");
    }

    // ==================== VdomHelperFlags ====================

    #[test]
    fn vdom_flags_empty() {
        let flags = VdomHelperFlags::empty();
        assert!(flags.is_empty());
        assert!(!flags.has(VdomHelper::CreateElementVNode));
        assert!(flags.to_imports().is_empty());
    }

    #[test]
    fn vdom_flags_add_single() {
        let flags = VdomHelperFlags::empty().add(VdomHelper::ToDisplayString);
        assert!(!flags.is_empty());
        assert!(flags.has(VdomHelper::ToDisplayString));
        assert!(!flags.has(VdomHelper::Fragment));
        assert_eq!(flags.to_imports(), vec!["_toDisplayString"]);
    }

    #[test]
    fn vdom_flags_add_deduplicates() {
        let flags = VdomHelperFlags::empty()
            .add(VdomHelper::OpenBlock)
            .add(VdomHelper::OpenBlock);
        assert_eq!(flags.to_imports().len(), 1);
    }

    #[test]
    fn vdom_flags_multiple() {
        let flags = VdomHelperFlags::empty()
            .add(VdomHelper::CreateElementVNode)
            .add(VdomHelper::Fragment)
            .add(VdomHelper::OpenBlock);
        assert!(flags.has(VdomHelper::CreateElementVNode));
        assert!(flags.has(VdomHelper::Fragment));
        assert!(flags.has(VdomHelper::OpenBlock));
        let imports = flags.to_imports();
        assert_eq!(imports.len(), 3);
        // Ordered by bit position
        assert_eq!(imports[0], "_createElementVNode");
        assert_eq!(imports[1], "_openBlock");
        assert_eq!(imports[2], "_Fragment");
    }

    #[test]
    fn vdom_flags_union() {
        let a = VdomHelperFlags::empty()
            .add(VdomHelper::CreateVNode)
            .add(VdomHelper::Fragment);
        let b = VdomHelperFlags::empty()
            .add(VdomHelper::Fragment)
            .add(VdomHelper::VShow);
        let merged = a.union(b);
        assert!(merged.has(VdomHelper::CreateVNode));
        assert!(merged.has(VdomHelper::Fragment));
        assert!(merged.has(VdomHelper::VShow));
        assert_eq!(merged.to_imports().len(), 3);
    }

    #[test]
    fn vdom_helper_name_roundtrip() {
        // Verify every variant's name matches the corresponding const
        assert_eq!(VdomHelper::CreateElementVNode.name(), CREATE_ELEMENT_VNODE);
        assert_eq!(VdomHelper::CreateElementBlock.name(), CREATE_ELEMENT_BLOCK);
        assert_eq!(VdomHelper::CreateVNode.name(), CREATE_VNODE);
        assert_eq!(VdomHelper::CreateBlock.name(), CREATE_BLOCK);
        assert_eq!(VdomHelper::CreateCommentVNode.name(), CREATE_COMMENT_VNODE);
        assert_eq!(VdomHelper::CreateTextVNode.name(), CREATE_TEXT_VNODE);
        assert_eq!(VdomHelper::OpenBlock.name(), OPEN_BLOCK);
        assert_eq!(VdomHelper::Fragment.name(), FRAGMENT);
        assert_eq!(VdomHelper::ToDisplayString.name(), TO_DISPLAY_STRING);
        assert_eq!(VdomHelper::RenderList.name(), RENDER_LIST);
        assert_eq!(VdomHelper::WithCtx.name(), WITH_CTX);
        assert_eq!(VdomHelper::WithDirectives.name(), WITH_DIRECTIVES);
        assert_eq!(VdomHelper::WithModifiers.name(), WITH_MODIFIERS);
        assert_eq!(VdomHelper::WithKeys.name(), WITH_KEYS);
        assert_eq!(VdomHelper::ResolveComponent.name(), RESOLVE_COMPONENT);
        assert_eq!(VdomHelper::ResolveDirective.name(), RESOLVE_DIRECTIVE);
        assert_eq!(VdomHelper::SetBlockTracking.name(), SET_BLOCK_TRACKING);
        assert_eq!(VdomHelper::VModelText.name(), V_MODEL_TEXT);
        assert_eq!(VdomHelper::VModelCheckbox.name(), V_MODEL_CHECKBOX);
        assert_eq!(VdomHelper::VModelRadio.name(), V_MODEL_RADIO);
        assert_eq!(VdomHelper::VModelSelect.name(), V_MODEL_SELECT);
        assert_eq!(VdomHelper::VModelDynamic.name(), V_MODEL_DYNAMIC);
        assert_eq!(VdomHelper::VShow.name(), V_SHOW);
    }

    // ==================== VaporHelperFlags ====================

    #[test]
    fn vapor_flags_empty() {
        let flags = VaporHelperFlags::empty();
        assert!(flags.is_empty());
        assert!(flags.to_imports().is_empty());
    }

    #[test]
    fn vapor_flags_add_single() {
        let flags = VaporHelperFlags::empty().add(VaporHelper::Template);
        assert!(flags.has(VaporHelper::Template));
        assert_eq!(flags.to_imports(), vec!["_template"]);
    }

    #[test]
    fn vapor_flags_add_deduplicates() {
        let flags = VaporHelperFlags::empty()
            .add(VaporHelper::RenderEffect)
            .add(VaporHelper::RenderEffect);
        assert_eq!(flags.to_imports().len(), 1);
    }

    #[test]
    fn vapor_flags_multiple() {
        let flags = VaporHelperFlags::empty()
            .add(VaporHelper::Template)
            .add(VaporHelper::SetText)
            .add(VaporHelper::RenderEffect);
        let imports = flags.to_imports();
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0], "_template");
        assert_eq!(imports[1], "_setText");
        assert_eq!(imports[2], "_renderEffect");
    }

    #[test]
    fn vapor_flags_union() {
        let a = VaporHelperFlags::empty()
            .add(VaporHelper::Child)
            .add(VaporHelper::SetClass);
        let b = VaporHelperFlags::empty()
            .add(VaporHelper::SetClass)
            .add(VaporHelper::CreateFor);
        let merged = a.union(b);
        assert_eq!(merged.to_imports().len(), 3);
    }

    #[test]
    fn vapor_helper_name_roundtrip() {
        assert_eq!(VaporHelper::Template.name(), TEMPLATE);
        assert_eq!(VaporHelper::Txt.name(), TXT);
        assert_eq!(VaporHelper::SetText.name(), SET_TEXT);
        assert_eq!(VaporHelper::SetClass.name(), SET_CLASS);
        assert_eq!(VaporHelper::SetStyle.name(), SET_STYLE);
        assert_eq!(VaporHelper::SetProp.name(), SET_PROP);
        assert_eq!(VaporHelper::SetAttr.name(), SET_ATTR);
        assert_eq!(VaporHelper::SetHtml.name(), SET_HTML);
        assert_eq!(VaporHelper::SetDynamicProps.name(), SET_DYNAMIC_PROPS);
        assert_eq!(VaporHelper::Child.name(), CHILD);
        assert_eq!(VaporHelper::Next.name(), NEXT);
        assert_eq!(VaporHelper::RenderEffect.name(), RENDER_EFFECT);
        assert_eq!(VaporHelper::DelegateEvents.name(), DELEGATE_EVENTS);
        assert_eq!(VaporHelper::On.name(), ON);
        assert_eq!(VaporHelper::CreateInvoker.name(), CREATE_INVOKER);
        assert_eq!(VaporHelper::CreateIf.name(), CREATE_IF);
        assert_eq!(VaporHelper::CreateFor.name(), CREATE_FOR);
        assert_eq!(VaporHelper::CreateSlot.name(), CREATE_SLOT);
        assert_eq!(VaporHelper::CreateComponent.name(), CREATE_COMPONENT);
        assert_eq!(VaporHelper::ToDisplayString.name(), VAPOR_TO_DISPLAY_STRING);
    }
}
