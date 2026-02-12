/// Discriminated element kind — groups element-kind-specific fields so that
/// invalid states are unrepresentable. A native `<div>` cannot have a
/// `component_var`, and a `<slot>` outlet cannot have `needs_vapor_ctx`.
pub(crate) enum VaporElementKind {
    /// Plain HTML element (`<div>`, `<span>`, etc.).
    Native,
    /// Vue component (`<MyComp>`, `<my-comp>`, built-in like `<Teleport>`).
    Component {
        /// Resolved component variable name (e.g., `_component_MyComp`, `_VaporTeleport`).
        component_var: String,
        /// Collected slot content for component children.
        slot_children: Vec<VaporSlotInfo>,
        /// Whether this component uses `_withVaporCtx` for its default slot
        /// (KeepAlive, Suspense).
        needs_vapor_ctx: bool,
        /// Slot name from `v-slot:name` on the component itself.
        slot_name: Option<String>,
        /// Slot params from `v-slot="{ item }"` on the component itself.
        slot_params: Option<String>,
    },
    /// `<component :is="expr">` dynamic component.
    DynamicComponent {
        /// The `:is` expression.
        dynamic_is_expr: Option<String>,
        /// Collected slot content for component children.
        slot_children: Vec<VaporSlotInfo>,
    },
    /// `<slot>` outlet element.
    SlotOutlet {
        /// Static slot name from `<slot name="header">`.
        slot_name: Option<String>,
        /// Collected slot content (fallback content).
        slot_children: Vec<VaporSlotInfo>,
    },
    /// `<template>` wrapper element (for `<template v-if>`, `<template #name>`, etc.).
    TemplateWrapper {
        /// Slot name for `<template #name>` children of components.
        slot_name: Option<String>,
        /// Whether the slot name is dynamic (`#[expr]`).
        slot_name_is_dynamic: bool,
        /// Dynamic slot name expression.
        slot_dynamic_name_expr: Option<String>,
        /// Slot params string for scoped slots (e.g., `_slotProps0`).
        slot_params: Option<String>,
    },
}

impl VaporElementKind {
    pub fn is_component(&self) -> bool {
        matches!(self, VaporElementKind::Component { .. })
    }

    pub fn is_dynamic_component(&self) -> bool {
        matches!(self, VaporElementKind::DynamicComponent { .. })
    }

    pub fn is_slot_outlet(&self) -> bool {
        matches!(self, VaporElementKind::SlotOutlet { .. })
    }

    pub fn is_template_element(&self) -> bool {
        matches!(self, VaporElementKind::TemplateWrapper { .. })
    }

    /// Get a mutable reference to slot_children (available on Component, DynamicComponent, SlotOutlet).
    pub fn slot_children_mut(&mut self) -> Option<&mut Vec<VaporSlotInfo>> {
        match self {
            VaporElementKind::Component { slot_children, .. }
            | VaporElementKind::DynamicComponent { slot_children, .. }
            | VaporElementKind::SlotOutlet { slot_children, .. } => Some(slot_children),
            _ => None,
        }
    }

    /// Get a reference to slot_children.
    pub fn slot_children(&self) -> Option<&Vec<VaporSlotInfo>> {
        match self {
            VaporElementKind::Component { slot_children, .. }
            | VaporElementKind::DynamicComponent { slot_children, .. }
            | VaporElementKind::SlotOutlet { slot_children, .. } => Some(slot_children),
            _ => None,
        }
    }
}

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
    /// Each entry is a structured setter call that can render to code or extract
    /// component prop entries without string parsing.
    pub effects: Vec<VaporEffect>,
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

    /// Element kind — discriminated union of kind-specific fields.
    pub kind: VaporElementKind,

    /// Whether this element has `v-once` — effects are emitted as direct statements
    /// instead of being wrapped in `_renderEffect`.
    pub is_once: bool,

    /// Structural directive scope info extracted from `ElementScope` variants.
    pub scope: Option<VaporScopeKind>,

    /// Collected structural child output (v-if/v-for blocks) as statements.
    pub structural_children: Vec<String>,

    /// Active v-for variable mappings: original name → `_for_item{N}.value`.
    /// Inherited from parent + own v-for scope.
    pub for_var_mappings: Vec<(String, String)>,
}

/// Part of a `_setText` call's arguments.
pub(crate) enum VaporTextPart {
    /// Literal text: `"Count: "`
    Static(String),
    /// Dynamic expression: `_toDisplayString(_ctx.count)`
    Dynamic(String),
}

/// Structured representation of a vapor effect (setter call inside `_renderEffect`).
///
/// Instead of storing effects as opaque strings and re-parsing them later
/// (e.g., in `build_component_props`), this enum carries the structured data
/// needed to render the effect code and extract component prop entries.
///
/// This eliminates the fragile `parse_effect_as_component_prop` string parser
/// and makes node_ref rewriting safe (no string replacement needed).
pub(crate) enum VaporEffect {
    /// `_setClass(n{node_ref}, {expr})`
    SetClass { node_ref: u32, expr: String },
    /// `_setStyle(n{node_ref}, {expr})`
    SetStyle { node_ref: u32, expr: String },
    /// `_setProp(n{node_ref}, "{attr}", {expr})`
    SetProp {
        node_ref: u32,
        attr: String,
        expr: String,
    },
    /// `_setDynamicProps(n{node_ref}, [{expr}])`
    SetDynamicProps { node_ref: u32, expr: String },
    /// `_setHtml(n{node_ref}, {expr})`
    SetHtml { node_ref: u32, expr: String },
    /// `_on(n{node_ref}, {event_expr}, {handler}, {{ effect: true }})`
    OnDynamic {
        node_ref: u32,
        event_expr: String,
        handler: String,
    },
    /// A raw effect string for cases not covered by the structured variants
    /// (e.g., `_setText` calls generated from text parts).
    Raw(String),
}

impl VaporEffect {
    /// Render this effect as a code string, optionally overriding the node_ref.
    ///
    /// When `node_ref_override` is `Some(new_ref)`, the effect is rendered with
    /// `n{new_ref}` instead of its original node_ref. This is used by
    /// `build_block_body` to rewrite structural directive node refs to inner
    /// template node refs — safely, without string replacement.
    pub fn to_code_string(&self, node_ref_override: Option<u32>) -> String {
        match self {
            VaporEffect::SetClass { node_ref, expr } => {
                let nr = node_ref_override.unwrap_or(*node_ref);
                format!("_setClass(n{}, {})", nr, expr)
            }
            VaporEffect::SetStyle { node_ref, expr } => {
                let nr = node_ref_override.unwrap_or(*node_ref);
                format!("_setStyle(n{}, {})", nr, expr)
            }
            VaporEffect::SetProp {
                node_ref,
                attr,
                expr,
            } => {
                let nr = node_ref_override.unwrap_or(*node_ref);
                format!("_setProp(n{}, \"{}\", {})", nr, attr, expr)
            }
            VaporEffect::SetDynamicProps { node_ref, expr } => {
                let nr = node_ref_override.unwrap_or(*node_ref);
                format!("_setDynamicProps(n{}, [{}])", nr, expr)
            }
            VaporEffect::SetHtml { node_ref, expr } => {
                let nr = node_ref_override.unwrap_or(*node_ref);
                format!("_setHtml(n{}, {})", nr, expr)
            }
            VaporEffect::OnDynamic {
                node_ref,
                event_expr,
                handler,
            } => {
                let nr = node_ref_override.unwrap_or(*node_ref);
                format!(
                    "_on(n{}, {}, {}, {{\n      effect: true\n    }})",
                    nr, event_expr, handler
                )
            }
            VaporEffect::Raw(s) => {
                // Raw effects don't support node_ref override — they're already
                // fully rendered (e.g., _setText calls from pending_nested_effects).
                s.clone()
            }
        }
    }

    /// Extract a component prop entry from this effect.
    ///
    /// Returns `Some("attr: () => (expr)")` for effects that represent component
    /// prop bindings, or `None` for effects that don't map to props (e.g., `_setHtml`).
    ///
    /// This replaces the fragile `parse_effect_as_component_prop` string parser.
    pub fn to_component_prop(&self) -> Option<String> {
        match self {
            VaporEffect::SetClass { expr, .. } => Some(format!("class: () => ({})", expr)),
            VaporEffect::SetStyle { expr, .. } => Some(format!("style: () => ({})", expr)),
            VaporEffect::SetProp { attr, expr, .. } => Some(format!("{}: () => ({})", attr, expr)),
            // SetDynamicProps, SetHtml, OnDynamic, Raw don't map to simple component props.
            _ => None,
        }
    }

    /// Get the node_ref from this effect, if it has one.
    #[allow(dead_code)]
    pub fn node_ref(&self) -> Option<u32> {
        match self {
            VaporEffect::SetClass { node_ref, .. }
            | VaporEffect::SetStyle { node_ref, .. }
            | VaporEffect::SetProp { node_ref, .. }
            | VaporEffect::SetDynamicProps { node_ref, .. }
            | VaporEffect::SetHtml { node_ref, .. }
            | VaporEffect::OnDynamic { node_ref, .. } => Some(*node_ref),
            VaporEffect::Raw(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── VaporEffect::to_code_string tests ───────────────────────────────

    #[test]
    fn test_effect_set_class_to_code() {
        let effect = VaporEffect::SetClass {
            node_ref: 0,
            expr: "_ctx.cls".to_string(),
        };
        assert_eq!(effect.to_code_string(None), "_setClass(n0, _ctx.cls)");
    }

    #[test]
    fn test_effect_set_class_with_override() {
        let effect = VaporEffect::SetClass {
            node_ref: 5,
            expr: "_ctx.cls".to_string(),
        };
        assert_eq!(effect.to_code_string(Some(12)), "_setClass(n12, _ctx.cls)");
    }

    #[test]
    fn test_effect_set_prop_to_code() {
        let effect = VaporEffect::SetProp {
            node_ref: 3,
            attr: "title".to_string(),
            expr: "_ctx.msg".to_string(),
        };
        assert_eq!(
            effect.to_code_string(None),
            "_setProp(n3, \"title\", _ctx.msg)"
        );
    }

    #[test]
    fn test_effect_set_prop_with_override() {
        let effect = VaporEffect::SetProp {
            node_ref: 1,
            attr: "title".to_string(),
            expr: "fn(a, b)".to_string(),
        };
        // This is the key test: nested parens in expr are preserved correctly
        assert_eq!(
            effect.to_code_string(Some(20)),
            "_setProp(n20, \"title\", fn(a, b))"
        );
    }

    #[test]
    fn test_effect_set_dynamic_props_to_code() {
        let effect = VaporEffect::SetDynamicProps {
            node_ref: 0,
            expr: "{ [_ctx.attr]: _ctx.val }".to_string(),
        };
        assert_eq!(
            effect.to_code_string(None),
            "_setDynamicProps(n0, [{ [_ctx.attr]: _ctx.val }])"
        );
    }

    #[test]
    fn test_effect_raw_ignores_override() {
        let effect = VaporEffect::Raw("_setText(x0, _ctx.msg)".to_string());
        assert_eq!(effect.to_code_string(Some(99)), "_setText(x0, _ctx.msg)");
    }

    // ── VaporEffect::to_component_prop tests ────────────────────────────

    #[test]
    fn test_effect_set_class_to_component_prop() {
        let effect = VaporEffect::SetClass {
            node_ref: 0,
            expr: "_ctx.cls".to_string(),
        };
        assert_eq!(
            effect.to_component_prop(),
            Some("class: () => (_ctx.cls)".to_string())
        );
    }

    #[test]
    fn test_effect_set_style_to_component_prop() {
        let effect = VaporEffect::SetStyle {
            node_ref: 0,
            expr: "_ctx.sty".to_string(),
        };
        assert_eq!(
            effect.to_component_prop(),
            Some("style: () => (_ctx.sty)".to_string())
        );
    }

    #[test]
    fn test_effect_set_prop_to_component_prop() {
        let effect = VaporEffect::SetProp {
            node_ref: 0,
            attr: "title".to_string(),
            expr: "_ctx.msg".to_string(),
        };
        assert_eq!(
            effect.to_component_prop(),
            Some("title: () => (_ctx.msg)".to_string())
        );
    }

    #[test]
    fn test_effect_set_prop_nested_parens_to_component_prop() {
        let effect = VaporEffect::SetProp {
            node_ref: 0,
            attr: "title".to_string(),
            expr: "fn(a, b)".to_string(),
        };
        // Structured data preserves nested parens correctly — no string parsing needed
        assert_eq!(
            effect.to_component_prop(),
            Some("title: () => (fn(a, b))".to_string())
        );
    }

    #[test]
    fn test_effect_set_html_no_component_prop() {
        let effect = VaporEffect::SetHtml {
            node_ref: 0,
            expr: "_ctx.html".to_string(),
        };
        assert_eq!(effect.to_component_prop(), None);
    }

    #[test]
    fn test_effect_raw_no_component_prop() {
        let effect = VaporEffect::Raw("_setText(x0, _ctx.msg)".to_string());
        assert_eq!(effect.to_component_prop(), None);
    }

    // ── VaporEffect::node_ref tests ─────────────────────────────────────

    #[test]
    fn test_effect_node_ref() {
        let effect = VaporEffect::SetClass {
            node_ref: 42,
            expr: "x".to_string(),
        };
        assert_eq!(effect.node_ref(), Some(42));
    }

    #[test]
    fn test_effect_raw_no_node_ref() {
        let effect = VaporEffect::Raw("something".to_string());
        assert_eq!(effect.node_ref(), None);
    }
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
            kind: VaporElementKind::Native,
            is_once: false,
            scope: None,
            structural_children: Vec::new(),
            for_var_mappings: Vec::new(),
        }
    }

    // ── Kind convenience accessors ──────────────────────────────────────

    pub fn is_component(&self) -> bool {
        self.kind.is_component()
    }

    pub fn is_dynamic_component(&self) -> bool {
        self.kind.is_dynamic_component()
    }

    pub fn is_slot_outlet(&self) -> bool {
        self.kind.is_slot_outlet()
    }

    pub fn is_template_element(&self) -> bool {
        self.kind.is_template_element()
    }
}

// ── Generator sub-structs ──────────────────────────────────────────────

/// Counter state for node/text/path variable naming.
///
/// Extracted from `VaporTemplateGenerator` to reduce its field count
/// and group related counter logic.
pub(crate) struct VaporCounters {
    /// Node reference counter (`n0`, `n1`, ...).
    pub node: u32,
    /// Text node reference counter (`x0`, `x1`, ...).
    pub text_node: u32,
    /// Path variable counter (`p0`, `p1`, ...) for intermediate navigation.
    pub path: u32,
    /// Current v-for nesting depth (for `_for_item0`, `_for_item1` naming).
    pub for_depth: u32,
    /// Counter for `_slotProps0`, `_slotProps1` naming.
    pub slot_props: u32,
}

impl VaporCounters {
    pub fn new() -> Self {
        Self {
            node: 0,
            text_node: 0,
            path: 0,
            for_depth: 0,
            slot_props: 0,
        }
    }

    /// Allocate a new node reference index.
    pub fn next_node_ref(&mut self) -> u32 {
        let idx = self.node;
        self.node += 1;
        idx
    }

    /// Allocate a new text node reference index.
    pub fn next_text_node_ref(&mut self) -> u32 {
        let idx = self.text_node;
        self.text_node += 1;
        idx
    }

    /// Allocate a new path variable index.
    pub fn next_path_ref(&mut self) -> u32 {
        let idx = self.path;
        self.path += 1;
        idx
    }
}

/// Resolved component and directive declarations for hoisting.
///
/// Tracks unique component/directive names (deduped via hash sets) and
/// the declaration strings to emit before the render function.
pub(crate) struct VaporResolutions {
    /// Resolved component names for `_resolveComponent` declarations.
    pub components: Vec<String>,
    /// Hash set for O(1) component dedup lookups.
    pub components_set: rustc_hash::FxHashSet<String>,
    /// Resolved component declarations to emit before render function.
    pub component_decls: Vec<String>,
    /// Resolved custom directive names for deduplication.
    pub directives: Vec<String>,
    /// Hash set for O(1) directive dedup lookups.
    pub directives_set: rustc_hash::FxHashSet<String>,
    /// Resolved directive declarations to emit at top of render function.
    pub directive_decls: Vec<String>,
}

impl VaporResolutions {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
            components_set: rustc_hash::FxHashSet::default(),
            component_decls: Vec::new(),
            directives: Vec::new(),
            directives_set: rustc_hash::FxHashSet::default(),
            directive_decls: Vec::new(),
        }
    }
}

/// Collected nested content waiting to be emitted when the root element closes.
///
/// During tree traversal, nested dynamic descendants generate navigation,
/// text creations, effects, and statements that bubble up to the root.
pub(crate) struct VaporPendingContent {
    /// Navigation instructions (`const nX = _child(...)`, `const pX = _next(...)`).
    pub nav: Vec<String>,
    /// Text node creations (`const xN = _txt(nX)`).
    pub text_creations: Vec<String>,
    /// Effects from nested dynamic descendants.
    pub nested_effects: Vec<VaporEffect>,
    /// Statements from nested dynamic descendants.
    pub nested_statements: Vec<String>,
}

impl VaporPendingContent {
    pub fn new() -> Self {
        Self {
            nav: Vec::new(),
            text_creations: Vec::new(),
            nested_effects: Vec::new(),
            nested_statements: Vec::new(),
        }
    }

    /// Drain navigation instructions and text node creations into a buffer.
    pub fn drain_instructions(&mut self, buf: &mut String) {
        for nav in self.nav.drain(..) {
            buf.push_str(&nav);
            buf.push('\n');
        }
        for tc in self.text_creations.drain(..) {
            buf.push_str(&tc);
            buf.push('\n');
        }
    }
}
