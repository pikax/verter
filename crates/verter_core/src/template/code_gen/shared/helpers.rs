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
    RenderSlot = 1 << 23,
    CreateSlots = 1 << 24,
    MergeProps = 1 << 25,
    NormalizeClass = 1 << 26,
    NormalizeStyle = 1 << 27,
    ResolveDynamicComponent = 1 << 28,
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
        }
    }
}

/// Ordered lookup table for `VdomHelperFlags::to_imports()`.
const ALL_VDOM: [VdomHelper; 29] = [
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
    // SAFETY: digits are all ASCII, valid UTF-8
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

/// Escape a string for use in a JS string literal. Returns new `String` only if
/// escaping is needed; the caller should check [`needs_js_escaping`] first to
/// avoid allocation for the common case.
pub fn escape_js_string(s: &str) -> String {
    let mut buf = String::with_capacity(s.len() + 8);
    escape_js_string_into(&mut buf, s);
    buf
}

// ======================== HTML entity decoding ========================

/// Decode a single HTML entity at the start of `s` (which must begin with `&`).
/// Returns the decoded char and the byte length consumed (including `&` and `;`).
fn decode_html_entity(s: &str) -> Option<(char, usize)> {
    if !s.starts_with('&') {
        return None;
    }
    let semi = s[1..].find(';')?;
    if semi > 32 {
        return None;
    }
    let entity = &s[1..semi + 1];
    let ch = match entity {
        // XML predefined entities
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        // Common HTML entities
        "nbsp" => '\u{00A0}',
        "copy" => '\u{00A9}',
        "reg" => '\u{00AE}',
        "trade" => '\u{2122}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "laquo" => '\u{00AB}',
        "raquo" => '\u{00BB}',
        "bull" => '\u{2022}',
        "middot" => '\u{00B7}',
        "iexcl" => '\u{00A1}',
        "iquest" => '\u{00BF}',
        "cent" => '\u{00A2}',
        "pound" => '\u{00A3}',
        "yen" => '\u{00A5}',
        "euro" => '\u{20AC}',
        "curren" => '\u{00A4}',
        "sect" => '\u{00A7}',
        "para" => '\u{00B6}',
        "deg" => '\u{00B0}',
        "plusmn" => '\u{00B1}',
        "micro" => '\u{00B5}',
        "times" => '\u{00D7}',
        "divide" => '\u{00F7}',
        "frac14" => '\u{00BC}',
        "frac12" => '\u{00BD}',
        "frac34" => '\u{00BE}',
        "sup1" => '\u{00B9}',
        "sup2" => '\u{00B2}',
        "sup3" => '\u{00B3}',
        // Typographic
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "sbquo" => '\u{201A}',
        "bdquo" => '\u{201E}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        "permil" => '\u{2030}',
        "prime" => '\u{2032}',
        "Prime" => '\u{2033}',
        "lsaquo" => '\u{2039}',
        "rsaquo" => '\u{203A}',
        "oline" => '\u{203E}',
        // Arrows
        "larr" => '\u{2190}',
        "uarr" => '\u{2191}',
        "rarr" => '\u{2192}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        // Math
        "fnof" => '\u{0192}',
        "infin" => '\u{221E}',
        "radic" => '\u{221A}',
        "sum" => '\u{2211}',
        "prod" => '\u{220F}',
        "minus" => '\u{2212}',
        "lowast" => '\u{2217}',
        "sim" => '\u{223C}',
        "asymp" => '\u{2248}',
        "ne" => '\u{2260}',
        "equiv" => '\u{2261}',
        "le" => '\u{2264}',
        "ge" => '\u{2265}',
        "sub" => '\u{2282}',
        "sup" => '\u{2283}',
        "nsub" => '\u{2284}',
        "sube" => '\u{2286}',
        "supe" => '\u{2287}',
        "oplus" => '\u{2295}',
        "otimes" => '\u{2297}',
        "perp" => '\u{22A5}',
        // Spacing / formatting
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwnj" => '\u{200C}',
        "zwj" => '\u{200D}',
        "lrm" => '\u{200E}',
        "rlm" => '\u{200F}',
        // Latin extended
        "Agrave" => '\u{00C0}',
        "Aacute" => '\u{00C1}',
        "Acirc" => '\u{00C2}',
        "Atilde" => '\u{00C3}',
        "Auml" => '\u{00C4}',
        "Aring" => '\u{00C5}',
        "AElig" => '\u{00C6}',
        "Ccedil" => '\u{00C7}',
        "Egrave" => '\u{00C8}',
        "Eacute" => '\u{00C9}',
        "Ecirc" => '\u{00CA}',
        "Euml" => '\u{00CB}',
        "Igrave" => '\u{00CC}',
        "Iacute" => '\u{00CD}',
        "Icirc" => '\u{00CE}',
        "Iuml" => '\u{00CF}',
        "ETH" => '\u{00D0}',
        "Ntilde" => '\u{00D1}',
        "Ograve" => '\u{00D2}',
        "Oacute" => '\u{00D3}',
        "Ocirc" => '\u{00D4}',
        "Otilde" => '\u{00D5}',
        "Ouml" => '\u{00D6}',
        "Oslash" => '\u{00D8}',
        "Ugrave" => '\u{00D9}',
        "Uacute" => '\u{00DA}',
        "Ucirc" => '\u{00DB}',
        "Uuml" => '\u{00DC}',
        "Yacute" => '\u{00DD}',
        "THORN" => '\u{00DE}',
        "szlig" => '\u{00DF}',
        "agrave" => '\u{00E0}',
        "aacute" => '\u{00E1}',
        "acirc" => '\u{00E2}',
        "atilde" => '\u{00E3}',
        "auml" => '\u{00E4}',
        "aring" => '\u{00E5}',
        "aelig" => '\u{00E6}',
        "ccedil" => '\u{00E7}',
        "egrave" => '\u{00E8}',
        "eacute" => '\u{00E9}',
        "ecirc" => '\u{00EA}',
        "euml" => '\u{00EB}',
        "igrave" => '\u{00EC}',
        "iacute" => '\u{00ED}',
        "icirc" => '\u{00EE}',
        "iuml" => '\u{00EF}',
        "eth" => '\u{00F0}',
        "ntilde" => '\u{00F1}',
        "ograve" => '\u{00F2}',
        "oacute" => '\u{00F3}',
        "ocirc" => '\u{00F4}',
        "otilde" => '\u{00F5}',
        "ouml" => '\u{00F6}',
        "oslash" => '\u{00F8}',
        "ugrave" => '\u{00F9}',
        "uacute" => '\u{00FA}',
        "ucirc" => '\u{00FB}',
        "uuml" => '\u{00FC}',
        "yacute" => '\u{00FD}',
        "thorn" => '\u{00FE}',
        "yuml" => '\u{00FF}',
        // Numeric/hex references
        _ if entity.starts_with('#') => {
            let num = &entity[1..];
            let code_point = if num.starts_with('x') || num.starts_with('X') {
                u32::from_str_radix(&num[1..], 16).ok()?
            } else {
                num.parse::<u32>().ok()?
            };
            char::from_u32(code_point)?
        }
        _ => return None,
    };
    Some((ch, semi + 2))
}

/// Decode HTML entities in `s` and append the result to `buf`.
/// Handles `&quot;` → `"`, `&amp;` → `&`, `&lt;` → `<`, `&gt;` → `>`,
/// `&apos;` → `'`, `&nbsp;` → U+00A0, and numeric/hex references.
pub fn decode_html_entities_into(buf: &mut String, s: &str) {
    // Fast path: no ampersands means no entities
    if !s.contains('&') {
        buf.push_str(s);
        return;
    }

    let bytes = s.as_bytes();
    let mut copy_from = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some((decoded, entity_len)) = decode_html_entity(&s[i..]) {
                // Flush unmodified region
                if copy_from < i {
                    buf.push_str(&s[copy_from..i]);
                }
                buf.push(decoded);
                i += entity_len;
                copy_from = i;
                continue;
            }
        }
        i += 1;
    }
    // Flush remaining
    if copy_from < bytes.len() {
        buf.push_str(&s[copy_from..]);
    }
}

/// Returns true if the string contains any HTML entity (`&...;`).
pub fn has_html_entities(s: &str) -> bool {
    s.contains('&')
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
#[path = "helpers_tests.rs"]
mod tests;
