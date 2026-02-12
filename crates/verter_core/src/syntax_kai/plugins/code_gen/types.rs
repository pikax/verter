#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptSetupImportDependencies(pub u8);

impl ScriptSetupImportDependencies {
    pub const DEFINE_COMPONENT: u8 = 1 << 0;
    pub const USE_SLOTS: u8 = 1 << 1;
    pub const MERGE_MODELS: u8 = 1 << 2;

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
        imports.join(",")
    }
}

/// Bitwise flags tracking which Vue runtime helpers the compiled render function needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateImportDependencies(pub u32);

impl TemplateImportDependencies {
    pub const OPEN_BLOCK: u32 = 1 << 0;
    pub const CREATE_ELEMENT_BLOCK: u32 = 1 << 1;
    pub const CREATE_ELEMENT_VNODE: u32 = 1 << 2;
    pub const CREATE_VNODE: u32 = 1 << 3;
    pub const RENDER_LIST: u32 = 1 << 4;
    pub const TO_DISPLAY_STRING: u32 = 1 << 5;
    pub const CREATE_COMMENT_VNODE: u32 = 1 << 6;
    pub const FRAGMENT: u32 = 1 << 7;
    pub const WITH_CTX: u32 = 1 << 8;
    pub const RENDER_SLOT: u32 = 1 << 9;
    pub const NORMALIZE_PROPS: u32 = 1 << 10;
    pub const MERGE_PROPS: u32 = 1 << 11;
    pub const WITH_DIRECTIVES: u32 = 1 << 12;
    pub const RESOLVE_COMPONENT: u32 = 1 << 13;
    pub const WITH_MODIFIERS: u32 = 1 << 14;
    pub const WITH_KEYS: u32 = 1 << 15;
    pub const RESOLVE_DYNAMIC_COMPONENT: u32 = 1 << 16;
    pub const CREATE_BLOCK: u32 = 1 << 17;
    pub const CREATE_TEXT_VNODE: u32 = 1 << 18;
    pub const GUARD_REACTIVE_PROPS: u32 = 1 << 19;
    pub const RESOLVE_DIRECTIVE: u32 = 1 << 20;
    pub const SET_BLOCK_TRACKING: u32 = 1 << 21;
    pub const V_MODEL_TEXT: u32 = 1 << 22;
    pub const V_MODEL_SELECT: u32 = 1 << 23;
    pub const V_MODEL_CHECKBOX: u32 = 1 << 24;
    pub const V_MODEL_RADIO: u32 = 1 << 25;
    pub const V_MODEL_DYNAMIC: u32 = 1 << 26;
    pub const NORMALIZE_CLASS: u32 = 1 << 27;
    pub const NORMALIZE_STYLE: u32 = 1 << 28;
    pub const V_SHOW: u32 = 1 << 29;
    pub const CREATE_SLOTS: u32 = 1 << 30;
    pub const TO_HANDLERS: u32 = 1 << 31;

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn add(&mut self, flag: u32) {
        self.0 |= flag;
    }

    #[inline]
    pub fn contains(&self, flag: u32) -> bool {
        (self.0 & flag) != 0
    }

    pub fn to_import_string(&self) -> String {
        if self.is_empty() {
            return String::new();
        }

        let mut imports = Vec::new();

        if self.contains(Self::OPEN_BLOCK) {
            imports.push("openBlock as _openBlock");
        }
        if self.contains(Self::CREATE_ELEMENT_BLOCK) {
            imports.push("createElementBlock as _createElementBlock");
        }
        if self.contains(Self::CREATE_ELEMENT_VNODE) {
            imports.push("createElementVNode as _createElementVNode");
        }
        if self.contains(Self::CREATE_VNODE) {
            imports.push("createVNode as _createVNode");
        }
        if self.contains(Self::RENDER_LIST) {
            imports.push("renderList as _renderList");
        }
        if self.contains(Self::TO_DISPLAY_STRING) {
            imports.push("toDisplayString as _toDisplayString");
        }
        if self.contains(Self::CREATE_COMMENT_VNODE) {
            imports.push("createCommentVNode as _createCommentVNode");
        }
        if self.contains(Self::CREATE_TEXT_VNODE) {
            imports.push("createTextVNode as _createTextVNode");
        }
        if self.contains(Self::FRAGMENT) {
            imports.push("Fragment as _Fragment");
        }
        if self.contains(Self::WITH_CTX) {
            imports.push("withCtx as _withCtx");
        }
        if self.contains(Self::RENDER_SLOT) {
            imports.push("renderSlot as _renderSlot");
        }
        if self.contains(Self::NORMALIZE_PROPS) {
            imports.push("normalizeProps as _normalizeProps");
        }
        if self.contains(Self::MERGE_PROPS) {
            imports.push("mergeProps as _mergeProps");
        }
        if self.contains(Self::WITH_DIRECTIVES) {
            imports.push("withDirectives as _withDirectives");
        }
        if self.contains(Self::RESOLVE_COMPONENT) {
            imports.push("resolveComponent as _resolveComponent");
        }
        if self.contains(Self::WITH_MODIFIERS) {
            imports.push("withModifiers as _withModifiers");
        }
        if self.contains(Self::WITH_KEYS) {
            imports.push("withKeys as _withKeys");
        }
        if self.contains(Self::RESOLVE_DYNAMIC_COMPONENT) {
            imports.push("resolveDynamicComponent as _resolveDynamicComponent");
        }
        if self.contains(Self::CREATE_BLOCK) {
            imports.push("createBlock as _createBlock");
        }
        if self.contains(Self::GUARD_REACTIVE_PROPS) {
            imports.push("guardReactiveProps as _guardReactiveProps");
        }
        if self.contains(Self::RESOLVE_DIRECTIVE) {
            imports.push("resolveDirective as _resolveDirective");
        }
        if self.contains(Self::SET_BLOCK_TRACKING) {
            imports.push("setBlockTracking as _setBlockTracking");
        }
        if self.contains(Self::V_MODEL_TEXT) {
            imports.push("vModelText as _vModelText");
        }
        if self.contains(Self::V_MODEL_SELECT) {
            imports.push("vModelSelect as _vModelSelect");
        }
        if self.contains(Self::V_MODEL_CHECKBOX) {
            imports.push("vModelCheckbox as _vModelCheckbox");
        }
        if self.contains(Self::V_MODEL_RADIO) {
            imports.push("vModelRadio as _vModelRadio");
        }
        if self.contains(Self::V_MODEL_DYNAMIC) {
            imports.push("vModelDynamic as _vModelDynamic");
        }
        if self.contains(Self::NORMALIZE_CLASS) {
            imports.push("normalizeClass as _normalizeClass");
        }
        if self.contains(Self::NORMALIZE_STYLE) {
            imports.push("normalizeStyle as _normalizeStyle");
        }
        if self.contains(Self::V_SHOW) {
            imports.push("vShow as _vShow");
        }
        if self.contains(Self::CREATE_SLOTS) {
            imports.push("createSlots as _createSlots");
        }
        if self.contains(Self::TO_HANDLERS) {
            imports.push("toHandlers as _toHandlers");
        }

        imports.join(",")
    }
}

/// Bitwise flags tracking which `vue/vapor` runtime helpers the compiled Vapor render needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VaporImportDependencies(pub u64);

impl VaporImportDependencies {
    // ── Template & nodes ────────────────────────────────────────────────
    pub const TEMPLATE: u64 = 1 << 0;
    pub const CREATE_TEXT_NODE: u64 = 1 << 1;
    pub const CREATE_COMMENT: u64 = 1 << 2;

    // ── DOM mutation ────────────────────────────────────────────────────
    pub const INSERT: u64 = 1 << 3;
    pub const PREPEND: u64 = 1 << 4;
    pub const REMOVE: u64 = 1 << 5;
    pub const SET_TEXT: u64 = 1 << 6;
    pub const SET_CLASS: u64 = 1 << 7;
    pub const SET_STYLE: u64 = 1 << 8;
    pub const SET_ATTR: u64 = 1 << 9;
    pub const SET_PROP: u64 = 1 << 10;
    pub const SET_DYNAMIC_PROPS: u64 = 1 << 11;
    pub const SET_HTML: u64 = 1 << 12;
    pub const SET_REF: u64 = 1 << 13;

    // ── Events ──────────────────────────────────────────────────────────
    pub const ON: u64 = 1 << 14;
    pub const DELEGATE: u64 = 1 << 15;
    pub const DELEGATE_EVENTS: u64 = 1 << 16;
    pub const WITH_MODIFIERS: u64 = 1 << 17;
    pub const WITH_KEYS: u64 = 1 << 18;

    // ── Structural ──────────────────────────────────────────────────────
    pub const CREATE_IF: u64 = 1 << 19;
    pub const CREATE_FOR: u64 = 1 << 20;
    pub const CREATE_COMPONENT: u64 = 1 << 21;
    pub const CREATE_DYNAMIC_COMPONENT: u64 = 1 << 22;
    pub const CREATE_SLOT: u64 = 1 << 23;
    pub const CREATE_FOR_SLOTS: u64 = 1 << 24;

    // ── Reactivity / effects ────────────────────────────────────────────
    pub const RENDER_EFFECT: u64 = 1 << 25;
    pub const TO_DISPLAY_STRING: u64 = 1 << 26;

    // ── Resolution ──────────────────────────────────────────────────────
    pub const RESOLVE_COMPONENT: u64 = 1 << 27;
    pub const RESOLVE_DIRECTIVE: u64 = 1 << 28;
    pub const WITH_DIRECTIVES: u64 = 1 << 29;

    // ── Normalize helpers ───────────────────────────────────────────────
    pub const NORMALIZE_CLASS: u64 = 1 << 30;
    pub const NORMALIZE_STYLE: u64 = 1 << 31;

    // ── Vue 3.6 vapor helpers ──────────────────────────────────────────
    pub const TXT: u64 = 1 << 32;
    pub const CREATE_INVOKER: u64 = 1 << 33;
    pub const CHILD: u64 = 1 << 34;
    pub const NEXT: u64 = 1 << 35;
    pub const APPLY_V_SHOW: u64 = 1 << 36;
    pub const APPLY_TEXT_MODEL: u64 = 1 << 37;
    pub const APPLY_CHECKBOX_MODEL: u64 = 1 << 38;
    pub const APPLY_RADIO_MODEL: u64 = 1 << 39;
    pub const APPLY_SELECT_MODEL: u64 = 1 << 40;
    pub const SET_VALUE: u64 = 1 << 41;
    pub const CREATE_TEMPLATE_REF_SETTER: u64 = 1 << 42;
    pub const WITH_VAPOR_DIRECTIVES: u64 = 1 << 43;

    // ── Structural (Phase 4+) ──────────────────────────────────────────
    pub const CREATE_COMPONENT_WITH_FALLBACK: u64 = 1 << 44;
    pub const SET_INSERTION_STATE: u64 = 1 << 45;
    pub const WITH_VAPOR_CTX: u64 = 1 << 46;
    pub const VAPOR_TELEPORT: u64 = 1 << 47;
    pub const VAPOR_TRANSITION: u64 = 1 << 48;
    pub const VAPOR_TRANSITION_GROUP: u64 = 1 << 49;
    pub const TO_HANDLERS: u64 = 1 << 50;

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

        let mut imports = Vec::new();

        if self.contains(Self::TEMPLATE) {
            imports.push("template as _template");
        }
        if self.contains(Self::CREATE_TEXT_NODE) {
            imports.push("createTextNode as _createTextNode");
        }
        if self.contains(Self::CREATE_COMMENT) {
            imports.push("createComment as _createComment");
        }
        if self.contains(Self::INSERT) {
            imports.push("insert as _insert");
        }
        if self.contains(Self::PREPEND) {
            imports.push("prepend as _prepend");
        }
        if self.contains(Self::REMOVE) {
            imports.push("remove as _remove");
        }
        if self.contains(Self::SET_TEXT) {
            imports.push("setText as _setText");
        }
        if self.contains(Self::SET_CLASS) {
            imports.push("setClass as _setClass");
        }
        if self.contains(Self::SET_STYLE) {
            imports.push("setStyle as _setStyle");
        }
        if self.contains(Self::SET_ATTR) {
            imports.push("setAttr as _setAttr");
        }
        if self.contains(Self::SET_PROP) {
            imports.push("setProp as _setProp");
        }
        if self.contains(Self::SET_DYNAMIC_PROPS) {
            imports.push("setDynamicProps as _setDynamicProps");
        }
        if self.contains(Self::SET_HTML) {
            imports.push("setHtml as _setHtml");
        }
        if self.contains(Self::SET_REF) {
            imports.push("setRef as _setRef");
        }
        if self.contains(Self::ON) {
            imports.push("on as _on");
        }
        if self.contains(Self::DELEGATE) {
            imports.push("delegate as _delegate");
        }
        if self.contains(Self::DELEGATE_EVENTS) {
            imports.push("delegateEvents as _delegateEvents");
        }
        if self.contains(Self::WITH_MODIFIERS) {
            imports.push("withModifiers as _withModifiers");
        }
        if self.contains(Self::WITH_KEYS) {
            imports.push("withKeys as _withKeys");
        }
        if self.contains(Self::CREATE_IF) {
            imports.push("createIf as _createIf");
        }
        if self.contains(Self::CREATE_FOR) {
            imports.push("createFor as _createFor");
        }
        if self.contains(Self::CREATE_COMPONENT) {
            imports.push("createComponent as _createComponent");
        }
        if self.contains(Self::CREATE_DYNAMIC_COMPONENT) {
            imports.push("createDynamicComponent as _createDynamicComponent");
        }
        if self.contains(Self::CREATE_SLOT) {
            imports.push("createSlot as _createSlot");
        }
        if self.contains(Self::CREATE_FOR_SLOTS) {
            imports.push("createForSlots as _createForSlots");
        }
        if self.contains(Self::RENDER_EFFECT) {
            imports.push("renderEffect as _renderEffect");
        }
        if self.contains(Self::TO_DISPLAY_STRING) {
            imports.push("toDisplayString as _toDisplayString");
        }
        if self.contains(Self::RESOLVE_COMPONENT) {
            imports.push("resolveComponent as _resolveComponent");
        }
        if self.contains(Self::RESOLVE_DIRECTIVE) {
            imports.push("resolveDirective as _resolveDirective");
        }
        if self.contains(Self::WITH_DIRECTIVES) {
            imports.push("withDirectives as _withDirectives");
        }
        if self.contains(Self::NORMALIZE_CLASS) {
            imports.push("normalizeClass as _normalizeClass");
        }
        if self.contains(Self::NORMALIZE_STYLE) {
            imports.push("normalizeStyle as _normalizeStyle");
        }
        if self.contains(Self::TXT) {
            imports.push("txt as _txt");
        }
        if self.contains(Self::CREATE_INVOKER) {
            imports.push("createInvoker as _createInvoker");
        }
        if self.contains(Self::CHILD) {
            imports.push("child as _child");
        }
        if self.contains(Self::NEXT) {
            imports.push("next as _next");
        }
        if self.contains(Self::APPLY_V_SHOW) {
            imports.push("applyVShow as _applyVShow");
        }
        if self.contains(Self::APPLY_TEXT_MODEL) {
            imports.push("applyTextModel as _applyTextModel");
        }
        if self.contains(Self::APPLY_CHECKBOX_MODEL) {
            imports.push("applyCheckboxModel as _applyCheckboxModel");
        }
        if self.contains(Self::APPLY_RADIO_MODEL) {
            imports.push("applyRadioModel as _applyRadioModel");
        }
        if self.contains(Self::APPLY_SELECT_MODEL) {
            imports.push("applySelectModel as _applySelectModel");
        }
        if self.contains(Self::SET_VALUE) {
            imports.push("setValue as _setValue");
        }
        if self.contains(Self::CREATE_TEMPLATE_REF_SETTER) {
            imports.push("createTemplateRefSetter as _createTemplateRefSetter");
        }
        if self.contains(Self::WITH_VAPOR_DIRECTIVES) {
            imports.push("withVaporDirectives as _withVaporDirectives");
        }
        if self.contains(Self::CREATE_COMPONENT_WITH_FALLBACK) {
            imports.push("createComponentWithFallback as _createComponentWithFallback");
        }
        if self.contains(Self::SET_INSERTION_STATE) {
            imports.push("setInsertionState as _setInsertionState");
        }
        if self.contains(Self::WITH_VAPOR_CTX) {
            imports.push("withVaporCtx as _withVaporCtx");
        }
        if self.contains(Self::VAPOR_TELEPORT) {
            imports.push("VaporTeleport as _VaporTeleport");
        }
        if self.contains(Self::VAPOR_TRANSITION) {
            imports.push("VaporTransition as _VaporTransition");
        }
        if self.contains(Self::VAPOR_TRANSITION_GROUP) {
            imports.push("VaporTransitionGroup as _VaporTransitionGroup");
        }
        if self.contains(Self::TO_HANDLERS) {
            imports.push("toHandlers as _toHandlers");
        }

        imports.join(",")
    }
}
