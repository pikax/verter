/// Per-element state on the vapor element stack.
pub(crate) struct VaporElementState {
    /// This element's node reference index (for `n{X}` variable name).
    pub node_ref: u32,
    /// Tag name (e.g., "div", "span").
    pub tag_name: String,
    /// Whether this is a root element (direct child of `<template>`).
    pub is_root: bool,
    /// Whether this is a void element (`<br>`, `<input>`, etc.).
    pub is_void: bool,
    /// Whether this is a self-closing element (`<br/>`).
    pub is_self_closing: bool,
    /// Position of `<` in the open tag.
    pub open_tag_start: u32,
    /// Position after `>` in the open tag.
    pub open_tag_end: u32,
    /// Whether any direct text child is dynamic (interpolation present).
    pub has_dynamic_children: bool,
    /// Accumulated effect bodies for `_renderEffect`.
    /// Each entry is a single setter call (e.g., `_setClass(n0, _ctx.cls)`).
    pub effects: Vec<String>,
    /// Direct statements that don't need `_renderEffect` wrapping
    /// (e.g., event assignments like `n0.$evtclick = ...`).
    pub statements: Vec<String>,
    /// Text parts for building a combined `_setText` call.
    pub text_parts: Vec<VaporTextPart>,
    /// Text node reference index (`x{N}`) if this element has dynamic text.
    pub text_node_ref: Option<u32>,
    /// Whether this element or a descendant has dynamic content requiring navigation.
    pub needs_node_ref: bool,
    /// Number of DOM child nodes seen so far (for `_child`/`_next` navigation).
    /// Counts elements and text node groups.
    pub child_count: u32,
    /// 0-based position among parent's children (elements + text nodes in HTML).
    pub child_index: u32,
    /// Navigation variable name once assigned (`"n{X}"` for dynamic, `"p{X}"` for path).
    pub var_name: Option<String>,
    /// Variable name of the last navigated-to child (for `_next` chaining).
    pub last_nav_child_var: Option<String>,
    /// Whether a text/interpolation child has been started (to coalesce consecutive
    /// text + interpolation into one DOM child node for `child_count`).
    pub text_child_started: bool,

    // ── Structural directive fields ─────────────────────────────────────
    /// Whether this element is a Vue component (PascalCase, kebab-case component, etc.).
    pub is_component: bool,
    /// Whether this is a `<slot>` outlet element.
    pub is_slot_outlet: bool,
    /// Whether this is a `<component :is="...">` dynamic component.
    pub is_dynamic_component: bool,
    /// Whether this is a `<template>` wrapper element (for `<template v-if>`, etc.).
    pub is_template_element: bool,
    /// Resolved component variable name (e.g., `_component_MyComp`).
    pub component_var: Option<String>,
    /// The `:is` expression for dynamic components.
    pub dynamic_is_expr: Option<String>,

    /// Whether this element has `v-once` — effects are emitted as direct statements
    /// instead of being wrapped in `_renderEffect`.
    pub is_once: bool,

    /// Structural directive scope info extracted from `ElementScope` variants.
    pub scope: Option<VaporScopeKind>,

    /// Collected slot content for component children.
    /// Key = slot name, Value = slot info.
    pub slot_children: Vec<VaporSlotInfo>,

    /// Whether this component uses `_withVaporCtx` for its default slot
    /// (KeepAlive, Suspense).
    pub needs_vapor_ctx: bool,

    /// Collected structural child output (v-if/v-for blocks) as statements.
    pub structural_children: Vec<String>,

    /// Active v-for variable mappings: original name → `_for_item{N}.value`.
    /// Inherited from parent + own v-for scope.
    pub for_var_mappings: Vec<(String, String)>,

    /// Slot name for `<template #name>` children of components.
    pub slot_name: Option<String>,
    /// Whether the slot name is dynamic (`#[expr]`).
    pub slot_name_is_dynamic: bool,
    /// Dynamic slot name expression.
    pub slot_dynamic_name_expr: Option<String>,
    /// Slot params string for scoped slots (e.g., `_slotProps0`).
    pub slot_params: Option<String>,
}

/// Part of a `_setText` call's arguments.
pub(crate) enum VaporTextPart {
    /// Literal text: `"Count: "`
    Static(String),
    /// Dynamic expression: `_toDisplayString(_ctx.count)`
    Dynamic(String),
}

/// Structural directive scope kind for an element.
pub(crate) enum VaporScopeKind {
    If {
        condition: String,
    },
    ElseIf {
        condition: String,
    },
    Else,
    For {
        iterable: String,
        /// Callback parameter names: `_for_item0`, `_for_key0`, etc.
        callback_params: Vec<String>,
        /// Original parameter names from the template (for key function).
        original_params: Vec<String>,
        /// Key function expression (from `:key` prop), if any.
        key_fn: Option<String>,
        /// Nesting depth (0 for outermost v-for).
        #[allow(dead_code)]
        depth: u32,
    },
}

/// Info about a slot being collected for a component.
pub(crate) struct VaporSlotInfo {
    /// Slot name (e.g., "default", "header").
    pub name: String,
    /// Whether the slot name is dynamic (`#[expr]`).
    pub is_dynamic: bool,
    /// Dynamic name expression (for `#[expr]`).
    pub dynamic_name_expr: Option<String>,
    /// Slot function parameter (e.g., `_slotProps0`), if scoped.
    pub params: Option<String>,
    /// The generated body of the slot function.
    pub body: String,
}

/// State for tracking a v-if chain across sibling elements.
pub(crate) struct VaporVIfChainState {
    /// The node reference for the `_createIf` result.
    pub node_ref: u32,
    /// Current branch index (0, 1, 2, ...).
    pub branch_index: u32,
    /// Accumulated code for the v-if chain so far.
    pub code: String,
    /// Number of unclosed `_createIf(` calls (for closing parens).
    pub open_parens: u32,
    /// Source position where the chain started (for code_transform).
    pub chain_start: u32,
    /// Source position where the last branch ended.
    pub chain_end: u32,
    /// The child_index in the parent where this v-if chain sits.
    #[allow(dead_code)] // Used in future phases for nested v-if insertion state
    pub child_index: u32,
}

impl VaporElementState {
    pub fn new(
        node_ref: u32,
        tag_name: String,
        is_root: bool,
        is_void: bool,
        is_self_closing: bool,
        open_tag_start: u32,
        open_tag_end: u32,
    ) -> Self {
        Self {
            node_ref,
            tag_name,
            is_root,
            is_void,
            is_self_closing,
            open_tag_start,
            open_tag_end,
            has_dynamic_children: false,
            effects: Vec::new(),
            statements: Vec::new(),
            text_parts: Vec::new(),
            text_node_ref: None,
            needs_node_ref: false,
            child_count: 0,
            child_index: 0,
            var_name: None,
            last_nav_child_var: None,
            text_child_started: false,
            is_component: false,
            is_slot_outlet: false,
            is_dynamic_component: false,
            is_template_element: false,
            component_var: None,
            dynamic_is_expr: None,
            is_once: false,
            scope: None,
            slot_children: Vec::new(),
            needs_vapor_ctx: false,
            structural_children: Vec::new(),
            for_var_mappings: Vec::new(),
            slot_name: None,
            slot_name_is_dynamic: false,
            slot_dynamic_name_expr: None,
            slot_params: None,
        }
    }
}
