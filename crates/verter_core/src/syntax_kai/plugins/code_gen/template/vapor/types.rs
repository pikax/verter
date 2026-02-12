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
}

/// Part of a `_setText` call's arguments.
pub(crate) enum VaporTextPart {
    /// Literal text: `"Count: "`
    Static(String),
    /// Dynamic expression: `_toDisplayString(_ctx.count)`
    Dynamic(String),
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
        }
    }
}
