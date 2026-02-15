use std::fmt;

/// Error type for template code generation invariant violations.
///
/// These errors indicate that the upstream tokenizer emitted an unexpected event
/// sequence (e.g., close without open, text outside template). Instead of panicking,
/// the codegen backends return these errors for graceful handling.
#[derive(Debug)]
pub enum TemplateCodeGenError {
    /// Stack underflow: attempted to pop/access an element that was never pushed.
    StackUnderflow(&'static str),
    /// A required scope was not set on the element state.
    MissingScope(&'static str),
    /// A required argument or value was not present.
    MissingArg(&'static str),
    /// An internal invariant was violated (e.g., wrong element kind for a codegen path).
    InvariantViolation(&'static str),
}

impl fmt::Display for TemplateCodeGenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateCodeGenError::StackUnderflow(ctx) => {
                write!(f, "template codegen stack underflow: {}", ctx)
            }
            TemplateCodeGenError::MissingScope(ctx) => {
                write!(f, "template codegen missing scope: {}", ctx)
            }
            TemplateCodeGenError::MissingArg(ctx) => {
                write!(f, "template codegen missing argument: {}", ctx)
            }
            TemplateCodeGenError::InvariantViolation(ctx) => {
                write!(f, "template codegen invariant violation: {}", ctx)
            }
        }
    }
}

impl std::error::Error for TemplateCodeGenError {}

/// Shorthand result type for template code generation.
pub type TemplateCodeGenResult<T = ()> = Result<T, TemplateCodeGenError>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptSetupImportDependencies(pub u8);

impl ScriptSetupImportDependencies {
    pub const DEFINE_COMPONENT: u8 = 1 << 0;
    pub const USE_SLOTS: u8 = 1 << 1;
    pub const MERGE_MODELS: u8 = 1 << 2;
    pub const USE_CSS_VARS: u8 = 1 << 3;

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn add(&mut self, flag: u8) {
        self.0 |= flag;
    }

    #[inline]
    pub fn contains(&self, flag: u8) -> bool {
        (self.0 & flag) != 0
    }

    pub fn to_import_string(&self) -> String {
        let mut imports = Vec::new();
        if self.contains(Self::DEFINE_COMPONENT) {
            imports.push("defineComponent as _defineComponent");
        }
        if self.contains(Self::USE_SLOTS) {
            imports.push("useSlots as _useSlots");
        }
        if self.contains(Self::MERGE_MODELS) {
            imports.push("mergeModels as _mergeModels");
        }
        if self.contains(Self::USE_CSS_VARS) {
            imports.push("useCssVars as _useCssVars");
        }
        imports.join(",")
    }
}

/// Bitwise flags tracking which Vue runtime helpers the compiled render function needs.
///
/// Upgraded from `u32` to `u64` — the previous `u32` was completely full at bit 31.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateImportDependencies(pub u64);

impl TemplateImportDependencies {
    pub const OPEN_BLOCK: u64 = 1 << 0;
    pub const CREATE_ELEMENT_BLOCK: u64 = 1 << 1;
    pub const CREATE_ELEMENT_VNODE: u64 = 1 << 2;
    pub const CREATE_VNODE: u64 = 1 << 3;
    pub const RENDER_LIST: u64 = 1 << 4;
    pub const TO_DISPLAY_STRING: u64 = 1 << 5;
    pub const CREATE_COMMENT_VNODE: u64 = 1 << 6;
    pub const FRAGMENT: u64 = 1 << 7;
    pub const WITH_CTX: u64 = 1 << 8;
    pub const RENDER_SLOT: u64 = 1 << 9;
    pub const NORMALIZE_PROPS: u64 = 1 << 10;
    pub const MERGE_PROPS: u64 = 1 << 11;
    pub const WITH_DIRECTIVES: u64 = 1 << 12;
    pub const RESOLVE_COMPONENT: u64 = 1 << 13;
    pub const WITH_MODIFIERS: u64 = 1 << 14;
    pub const WITH_KEYS: u64 = 1 << 15;
    pub const RESOLVE_DYNAMIC_COMPONENT: u64 = 1 << 16;
    pub const CREATE_BLOCK: u64 = 1 << 17;
    pub const CREATE_TEXT_VNODE: u64 = 1 << 18;
    pub const GUARD_REACTIVE_PROPS: u64 = 1 << 19;
    pub const RESOLVE_DIRECTIVE: u64 = 1 << 20;
    pub const SET_BLOCK_TRACKING: u64 = 1 << 21;
    pub const V_MODEL_TEXT: u64 = 1 << 22;
    pub const V_MODEL_SELECT: u64 = 1 << 23;
    pub const V_MODEL_CHECKBOX: u64 = 1 << 24;
    pub const V_MODEL_RADIO: u64 = 1 << 25;
    pub const V_MODEL_DYNAMIC: u64 = 1 << 26;
    pub const NORMALIZE_CLASS: u64 = 1 << 27;
    pub const NORMALIZE_STYLE: u64 = 1 << 28;
    pub const V_SHOW: u64 = 1 << 29;
    pub const CREATE_SLOTS: u64 = 1 << 30;
    pub const TO_HANDLERS: u64 = 1 << 31;

    // Compile-time assertion: highest flag must fit in the backing type.
    #[allow(dead_code)]
    const _HIGHEST_BIT_CHECK: () = assert!(Self::TO_HANDLERS <= (1u64 << 63));

    /// (flag, import_string) pairs for data-driven `to_import_string()`.
    const IMPORTS: &[(u64, &str)] = &[
        (Self::OPEN_BLOCK, "openBlock as _openBlock"),
        (
            Self::CREATE_ELEMENT_BLOCK,
            "createElementBlock as _createElementBlock",
        ),
        (
            Self::CREATE_ELEMENT_VNODE,
            "createElementVNode as _createElementVNode",
        ),
        (Self::CREATE_VNODE, "createVNode as _createVNode"),
        (Self::RENDER_LIST, "renderList as _renderList"),
        (
            Self::TO_DISPLAY_STRING,
            "toDisplayString as _toDisplayString",
        ),
        (
            Self::CREATE_COMMENT_VNODE,
            "createCommentVNode as _createCommentVNode",
        ),
        (
            Self::CREATE_TEXT_VNODE,
            "createTextVNode as _createTextVNode",
        ),
        (Self::FRAGMENT, "Fragment as _Fragment"),
        (Self::WITH_CTX, "withCtx as _withCtx"),
        (Self::RENDER_SLOT, "renderSlot as _renderSlot"),
        (Self::NORMALIZE_PROPS, "normalizeProps as _normalizeProps"),
        (Self::MERGE_PROPS, "mergeProps as _mergeProps"),
        (Self::WITH_DIRECTIVES, "withDirectives as _withDirectives"),
        (
            Self::RESOLVE_COMPONENT,
            "resolveComponent as _resolveComponent",
        ),
        (Self::WITH_MODIFIERS, "withModifiers as _withModifiers"),
        (Self::WITH_KEYS, "withKeys as _withKeys"),
        (
            Self::RESOLVE_DYNAMIC_COMPONENT,
            "resolveDynamicComponent as _resolveDynamicComponent",
        ),
        (Self::CREATE_BLOCK, "createBlock as _createBlock"),
        (
            Self::GUARD_REACTIVE_PROPS,
            "guardReactiveProps as _guardReactiveProps",
        ),
        (
            Self::RESOLVE_DIRECTIVE,
            "resolveDirective as _resolveDirective",
        ),
        (
            Self::SET_BLOCK_TRACKING,
            "setBlockTracking as _setBlockTracking",
        ),
        (Self::V_MODEL_TEXT, "vModelText as _vModelText"),
        (Self::V_MODEL_SELECT, "vModelSelect as _vModelSelect"),
        (Self::V_MODEL_CHECKBOX, "vModelCheckbox as _vModelCheckbox"),
        (Self::V_MODEL_RADIO, "vModelRadio as _vModelRadio"),
        (Self::V_MODEL_DYNAMIC, "vModelDynamic as _vModelDynamic"),
        (Self::NORMALIZE_CLASS, "normalizeClass as _normalizeClass"),
        (Self::NORMALIZE_STYLE, "normalizeStyle as _normalizeStyle"),
        (Self::V_SHOW, "vShow as _vShow"),
        (Self::CREATE_SLOTS, "createSlots as _createSlots"),
        (Self::TO_HANDLERS, "toHandlers as _toHandlers"),
    ];

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn add(&mut self, flag: u64) {
        self.0 |= flag;
    }

    #[inline]
    pub fn contains(&self, flag: u64) -> bool {
        (self.0 & flag) != 0
    }

    pub fn to_import_string(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        Self::IMPORTS
            .iter()
            .filter(|(flag, _)| self.contains(*flag))
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Bitwise flags tracking which `vue/vapor` runtime helpers the compiled Vapor render needs.
///
/// Upgraded from `u64` to `u128` — the previous `u64` was at bit 50 with only 13 bits remaining.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VaporImportDependencies(pub u128);

impl VaporImportDependencies {
    // ── Template & nodes ────────────────────────────────────────────────
    pub const TEMPLATE: u128 = 1 << 0;
    pub const CREATE_TEXT_NODE: u128 = 1 << 1;
    pub const CREATE_COMMENT: u128 = 1 << 2;

    // ── DOM mutation ────────────────────────────────────────────────────
    pub const INSERT: u128 = 1 << 3;
    pub const PREPEND: u128 = 1 << 4;
    pub const REMOVE: u128 = 1 << 5;
    pub const SET_TEXT: u128 = 1 << 6;
    pub const SET_CLASS: u128 = 1 << 7;
    pub const SET_STYLE: u128 = 1 << 8;
    pub const SET_ATTR: u128 = 1 << 9;
    pub const SET_PROP: u128 = 1 << 10;
    pub const SET_DYNAMIC_PROPS: u128 = 1 << 11;
    pub const SET_HTML: u128 = 1 << 12;
    pub const SET_REF: u128 = 1 << 13;

    // ── Events ──────────────────────────────────────────────────────────
    pub const ON: u128 = 1 << 14;
    pub const DELEGATE: u128 = 1 << 15;
    pub const DELEGATE_EVENTS: u128 = 1 << 16;
    pub const WITH_MODIFIERS: u128 = 1 << 17;
    pub const WITH_KEYS: u128 = 1 << 18;

    // ── Structural ──────────────────────────────────────────────────────
    pub const CREATE_IF: u128 = 1 << 19;
    pub const CREATE_FOR: u128 = 1 << 20;
    pub const CREATE_COMPONENT: u128 = 1 << 21;
    pub const CREATE_DYNAMIC_COMPONENT: u128 = 1 << 22;
    pub const CREATE_SLOT: u128 = 1 << 23;
    pub const CREATE_FOR_SLOTS: u128 = 1 << 24;

    // ── Reactivity / effects ────────────────────────────────────────────
    pub const RENDER_EFFECT: u128 = 1 << 25;
    pub const TO_DISPLAY_STRING: u128 = 1 << 26;

    // ── Resolution ──────────────────────────────────────────────────────
    pub const RESOLVE_COMPONENT: u128 = 1 << 27;
    pub const RESOLVE_DIRECTIVE: u128 = 1 << 28;
    pub const WITH_DIRECTIVES: u128 = 1 << 29;

    // ── Normalize helpers ───────────────────────────────────────────────
    pub const NORMALIZE_CLASS: u128 = 1 << 30;
    pub const NORMALIZE_STYLE: u128 = 1 << 31;

    // ── Vue 3.6 vapor helpers ──────────────────────────────────────────
    pub const TXT: u128 = 1 << 32;
    pub const CREATE_INVOKER: u128 = 1 << 33;
    pub const CHILD: u128 = 1 << 34;
    pub const NEXT: u128 = 1 << 35;
    pub const APPLY_V_SHOW: u128 = 1 << 36;
    pub const APPLY_TEXT_MODEL: u128 = 1 << 37;
    pub const APPLY_CHECKBOX_MODEL: u128 = 1 << 38;
    pub const APPLY_RADIO_MODEL: u128 = 1 << 39;
    pub const APPLY_SELECT_MODEL: u128 = 1 << 40;
    pub const SET_VALUE: u128 = 1 << 41;
    pub const CREATE_TEMPLATE_REF_SETTER: u128 = 1 << 42;
    pub const WITH_VAPOR_DIRECTIVES: u128 = 1 << 43;

    // ── Structural (Phase 4+) ──────────────────────────────────────────
    pub const CREATE_COMPONENT_WITH_FALLBACK: u128 = 1 << 44;
    pub const SET_INSERTION_STATE: u128 = 1 << 45;
    pub const WITH_VAPOR_CTX: u128 = 1 << 46;
    pub const VAPOR_TELEPORT: u128 = 1 << 47;
    pub const VAPOR_TRANSITION: u128 = 1 << 48;
    pub const VAPOR_TRANSITION_GROUP: u128 = 1 << 49;
    pub const TO_HANDLERS: u128 = 1 << 50;

    // Compile-time assertion: highest flag must fit in the backing type.
    #[allow(dead_code)]
    const _HIGHEST_BIT_CHECK: () = assert!(Self::TO_HANDLERS <= (1u128 << 127));

    /// (flag, import_string) pairs for data-driven `to_import_string()`.
    const IMPORTS: &[(u128, &str)] = &[
        (Self::TEMPLATE, "template as _template"),
        (Self::CREATE_TEXT_NODE, "createTextNode as _createTextNode"),
        (Self::CREATE_COMMENT, "createComment as _createComment"),
        (Self::INSERT, "insert as _insert"),
        (Self::PREPEND, "prepend as _prepend"),
        (Self::REMOVE, "remove as _remove"),
        (Self::SET_TEXT, "setText as _setText"),
        (Self::SET_CLASS, "setClass as _setClass"),
        (Self::SET_STYLE, "setStyle as _setStyle"),
        (Self::SET_ATTR, "setAttr as _setAttr"),
        (Self::SET_PROP, "setProp as _setProp"),
        (
            Self::SET_DYNAMIC_PROPS,
            "setDynamicProps as _setDynamicProps",
        ),
        (Self::SET_HTML, "setHtml as _setHtml"),
        (Self::SET_REF, "setRef as _setRef"),
        (Self::ON, "on as _on"),
        (Self::DELEGATE, "delegate as _delegate"),
        (Self::DELEGATE_EVENTS, "delegateEvents as _delegateEvents"),
        (Self::WITH_MODIFIERS, "withModifiers as _withModifiers"),
        (Self::WITH_KEYS, "withKeys as _withKeys"),
        (Self::CREATE_IF, "createIf as _createIf"),
        (Self::CREATE_FOR, "createFor as _createFor"),
        (
            Self::CREATE_COMPONENT,
            "createComponent as _createComponent",
        ),
        (
            Self::CREATE_DYNAMIC_COMPONENT,
            "createDynamicComponent as _createDynamicComponent",
        ),
        (Self::CREATE_SLOT, "createSlot as _createSlot"),
        (Self::CREATE_FOR_SLOTS, "createForSlots as _createForSlots"),
        (Self::RENDER_EFFECT, "renderEffect as _renderEffect"),
        (
            Self::TO_DISPLAY_STRING,
            "toDisplayString as _toDisplayString",
        ),
        (
            Self::RESOLVE_COMPONENT,
            "resolveComponent as _resolveComponent",
        ),
        (
            Self::RESOLVE_DIRECTIVE,
            "resolveDirective as _resolveDirective",
        ),
        (Self::WITH_DIRECTIVES, "withDirectives as _withDirectives"),
        (Self::NORMALIZE_CLASS, "normalizeClass as _normalizeClass"),
        (Self::NORMALIZE_STYLE, "normalizeStyle as _normalizeStyle"),
        (Self::TXT, "txt as _txt"),
        (Self::CREATE_INVOKER, "createInvoker as _createInvoker"),
        (Self::CHILD, "child as _child"),
        (Self::NEXT, "next as _next"),
        (Self::APPLY_V_SHOW, "applyVShow as _applyVShow"),
        (Self::APPLY_TEXT_MODEL, "applyTextModel as _applyTextModel"),
        (
            Self::APPLY_CHECKBOX_MODEL,
            "applyCheckboxModel as _applyCheckboxModel",
        ),
        (
            Self::APPLY_RADIO_MODEL,
            "applyRadioModel as _applyRadioModel",
        ),
        (
            Self::APPLY_SELECT_MODEL,
            "applySelectModel as _applySelectModel",
        ),
        (Self::SET_VALUE, "setValue as _setValue"),
        (
            Self::CREATE_TEMPLATE_REF_SETTER,
            "createTemplateRefSetter as _createTemplateRefSetter",
        ),
        (
            Self::WITH_VAPOR_DIRECTIVES,
            "withVaporDirectives as _withVaporDirectives",
        ),
        (
            Self::CREATE_COMPONENT_WITH_FALLBACK,
            "createComponentWithFallback as _createComponentWithFallback",
        ),
        (
            Self::SET_INSERTION_STATE,
            "setInsertionState as _setInsertionState",
        ),
        (Self::WITH_VAPOR_CTX, "withVaporCtx as _withVaporCtx"),
        (Self::VAPOR_TELEPORT, "VaporTeleport as _VaporTeleport"),
        (
            Self::VAPOR_TRANSITION,
            "VaporTransition as _VaporTransition",
        ),
        (
            Self::VAPOR_TRANSITION_GROUP,
            "VaporTransitionGroup as _VaporTransitionGroup",
        ),
        (Self::TO_HANDLERS, "toHandlers as _toHandlers"),
    ];

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn add(&mut self, flag: u128) {
        self.0 |= flag;
    }

    #[inline]
    pub fn contains(&self, flag: u128) -> bool {
        (self.0 & flag) != 0
    }

    pub fn to_import_string(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        Self::IMPORTS
            .iter()
            .filter(|(flag, _)| self.contains(*flag))
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(",")
    }
}
