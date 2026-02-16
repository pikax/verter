use crate::{
    common::Span,
    cursor::ScriptLanguage,
    syntax::{plugin::SyntaxPluginContext, types::*},
    tokenizer::{Event as TokenizerEvent, QuoteType},
    utils::vue::{
        is_html_tag, is_mathml_tag, is_svg_tag, is_tag_name_component, PatchFlag, PatchFlags,
    },
};

/// Flags that are mutually exclusive with FULL_PROPS.
/// When dynamic keys are detected, these individual flags are cleared
/// because FULL_PROPS implies a full diff that covers them all.
const FULL_PROPS_EXCLUDES: PatchFlag =
    PatchFlag(PatchFlags::Class as i16 | PatchFlags::Style as i16 | PatchFlags::Props as i16);

/// Estimate the patch flag contribution of a single prop and track dynamic prop names.
///
/// Vue's official compiler collects all props first, then computes flags.
/// FULL_PROPS (dynamic keys) is mutually exclusive with CLASS/STYLE/PROPS.
/// When we encounter a dynamic-key prop, we upgrade to FULL_PROPS and remove
/// the individual flags. Conversely, individual flags are only added when
/// FULL_PROPS is not already set.
///
/// Additional prop-level tracking:
/// - `dynamic_props`: arg spans of props contributing to PROPS (cleared on FULL_PROPS).
/// - `has_ref`: set when a `ref` attribute (static or `:ref`) is detected.
/// - `has_vnode_hook`: set when a `@vnode*` lifecycle hook listener is detected.
/// - Component CLASS/STYLE: on components, `:class`/`:style` become PROPS (not CLASS/STYLE)
///   because components handle their own class/style merging.
#[inline]
fn estimate_patch_flag(parent: &mut ElementOpenTagStart, prop: &Prop, bytes: &[u8]) {
    let is_component = parent.kind.is_component();

    // SAFETY: all flags used below are positive bitmask flags (not CACHED/BAIL).
    unsafe {
        match prop.kind {
            PropKind::BindSpread => {
                // v-bind="obj" spread → always dynamic keys
                parent.patch_flag = parent
                    .patch_flag
                    .remove_mask_unchecked(FULL_PROPS_EXCLUDES)
                    .add_unchecked(PatchFlags::FullProps);
                parent.dynamic_props.clear();
            }
            PropKind::OnSpread => {
                // v-on="obj" spread → dynamic keys + hydration for events
                parent.patch_flag = parent
                    .patch_flag
                    .remove_mask_unchecked(FULL_PROPS_EXCLUDES)
                    .add_unchecked(PatchFlags::FullProps)
                    .add_unchecked(PatchFlags::NeedHydration);
                parent.dynamic_props.clear();
            }
            PropKind::Bind => {
                // detect :ref
                if let Some(arg) = prop.arg {
                    if &bytes[arg.start as usize..arg.end as usize] == b"ref" {
                        parent.has_ref = true;
                    }
                }

                if prop.has_dynamic_arg {
                    // :[dynamicProp]="expr" → dynamic key
                    parent.patch_flag = parent
                        .patch_flag
                        .remove_mask_unchecked(FULL_PROPS_EXCLUDES)
                        .add_unchecked(PatchFlags::FullProps);
                    parent.dynamic_props.clear();
                } else if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    // :staticProp="expr" → PROPS (only if FULL_PROPS not already set)
                    parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Props);
                    if let Some(arg) = prop.arg {
                        parent.dynamic_props.push(arg);
                    }
                }
            }
            PropKind::On => {
                // detect @vnode* lifecycle hooks
                if let Some(arg) = prop.arg {
                    if bytes[arg.start as usize..arg.end as usize].starts_with(b"vnode") {
                        parent.has_vnode_hook = true;
                    }
                }

                // all event listeners need hydration
                parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::NeedHydration);
                if prop.has_dynamic_arg {
                    // @[dynamicEvent]="handler" → dynamic key
                    parent.patch_flag = parent
                        .patch_flag
                        .remove_mask_unchecked(FULL_PROPS_EXCLUDES)
                        .add_unchecked(PatchFlags::FullProps);
                    parent.dynamic_props.clear();
                }
            }
            PropKind::ClassBind => {
                // On components, :class becomes PROPS (components handle their own merging).
                // On elements, :class → CLASS.
                // NOTE: when class value is analysed it might remove this,
                // because when the class is static even when is a directive
                // it will remove the patch flag.
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    if is_component {
                        parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Props);
                        if let Some(arg) = prop.arg {
                            parent.dynamic_props.push(arg);
                        }
                    } else {
                        parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Class);
                    }
                }
            }
            PropKind::StyleBind => {
                // On components, :style becomes PROPS. On elements, :style → STYLE.
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    if is_component {
                        parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Props);
                        if let Some(arg) = prop.arg {
                            parent.dynamic_props.push(arg);
                        }
                    } else {
                        parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Style);
                    }
                }
            }
            PropKind::Model => {
                // v-model creates modelValue prop + onUpdate:modelValue event
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Props);
                    // "modelValue" is synthetic — codegen emits the string directly
                }
                parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::NeedHydration);
            }
            PropKind::Show | PropKind::Directive => {
                // v-show and custom directives have runtime hooks → NEED_PATCH
                parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::NeedPatch);
            }
            PropKind::Html | PropKind::Text => {
                // v-html/v-text create innerHTML/textContent prop bindings → PROPS
                // "innerHTML"/"textContent" are synthetic — codegen emits them directly
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    parent.patch_flag = parent.patch_flag.add_unchecked(PatchFlags::Props);
                }
            }
            PropKind::Value => {
                // detect static ref="..."
                if &bytes[prop.start as usize..prop.name_end as usize] == b"ref" {
                    parent.has_ref = true;
                }
            }
            // Static class/style and structural directives don't affect patch flags
            PropKind::ClassValue
            | PropKind::StyleValue
            | PropKind::If
            | PropKind::ElseIf
            | PropKind::Else
            | PropKind::For
            | PropKind::Slot
            | PropKind::Once => {}
        }
    }
}

// intermediary state for prop
struct PropTempState {
    /// Start position of the attribute/directive name
    start: u32,
    /// End position of the name
    name_end: u32,
    /// Whether this is a directive (vs a regular attribute)
    is_directive: bool,
    /// Directive argument start position (if any)
    arg_start: Option<u32>,
    /// Directive argument end position (if any)
    arg_end: Option<u32>,
    /// Start position of the value (after the opening quote)
    value_start: Option<u32>,
    /// Directive modifiers (e.g., .prevent, .stop)
    modifiers: Option<Vec<Span>>,
    /// Whether the directive argument is dynamic (e.g., :[arg])
    is_dynamic: Option<bool>,
}

pub enum RootNodeOpenTag {
    Start(RootNodeOpenTagStart),
    End(RootNodeOpenTagEnd),
}

pub struct Syntax<'alloc> {
    template_mode: bool,

    /// When true, skip patch flag estimation (Vapor uses renderEffect, not patch flags).
    is_vapor: bool,

    root_script_events: Vec<Event<'alloc>>,

    /// Current parent element ID (NO_PARENT at root level)
    last_parent_id: u32,
    nested_level: usize,

    last_root_node: Option<RootNodeOpenTag>,
    last_event_open_tag: Option<ElementOpenTagStart>,

    current_prop: Option<PropTempState>,

    /// Stack to track parent IDs for proper restoration on close tags.
    /// Pre-allocated with capacity 32 to avoid heap allocations for typical nesting depths.
    parent_stack: Vec<u32>,

    /// Stack of open element positions: `(start, name_end)`.
    /// Used by `finalize()` to detect unclosed elements ("Element is missing end tag.").
    /// Only tracks non-void, non-root elements.
    open_tag_stack: Vec<(u32, u32)>,

    events: Vec<Event<'alloc>>,

    scripts_found: usize,

    has_style_scope: bool,
    has_style_module: bool,

    /// Start position of `<script setup>` if present.
    script_setup_start: Option<u32>,
    /// Whether a `<template>` block exists.
    has_template: bool,
    /// Whether a `<template vapor>` block exists.
    has_vapor_template: bool,
    /// (start, close_end) byte range of the `<template>` block.
    template_block: Option<(u32, u32)>,
    /// End position (after `>`) of the `</script>` close tag.
    script_block_end: Option<u32>,

    /// Diagnostics accumulated during tokenization (errors, warnings).
    diagnostics: Vec<crate::syntax::plugin::Diagnostic>,

    /// Set to true when a fatal error is detected (e.g., invalid end tag).
    has_fatal_error: bool,
}

impl<'alloc> Syntax<'alloc> {
    // /// Take ownership of the root_script_events collected during parsing.
    // /// Call this after all tokenizer events have been processed.
    // pub fn take_root_script_events(&mut self) -> Vec<Event<'alloc>> {
    //     std::mem::take(&mut self.root_script_events)
    // }
    pub fn has_style_scope(&self) -> bool {
        self.has_style_scope
    }
    pub fn has_style_module(&self) -> bool {
        self.has_style_module
    }
    pub fn script_setup_start(&self) -> Option<u32> {
        self.script_setup_start
    }
    pub fn has_template(&self) -> bool {
        self.has_template
    }
    pub fn has_vapor_template(&self) -> bool {
        self.has_vapor_template
    }
    pub fn template_block(&self) -> Option<(u32, u32)> {
        self.template_block
    }
    pub fn script_block_end(&self) -> Option<u32> {
        self.script_block_end
    }

    /// Take accumulated diagnostics from the tokenization phase.
    pub fn take_diagnostics(&mut self) -> Vec<crate::syntax::plugin::Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Whether a fatal error was detected during tokenization.
    pub fn has_fatal_error(&self) -> bool {
        self.has_fatal_error
    }

    pub fn events(&mut self) -> Vec<Event<'alloc>> {
        let mut out = Vec::with_capacity(self.root_script_events.len() + self.events.len());
        out.append(&mut self.root_script_events);
        out.append(&mut self.events);
        out
    }

    pub fn new(template_mode: bool) -> Self {
        Self {
            template_mode,
            is_vapor: false,
            last_parent_id: NO_PARENT,
            nested_level: 0,
            last_root_node: None,
            last_event_open_tag: None,
            current_prop: None,
            parent_stack: Vec::with_capacity(32),
            open_tag_stack: Vec::with_capacity(32),
            events: Vec::with_capacity(256),
            root_script_events: Vec::with_capacity(6),

            scripts_found: 0,
            has_style_scope: false,
            has_style_module: false,
            script_setup_start: None,
            has_template: false,
            has_vapor_template: false,
            template_block: None,
            script_block_end: None,
            diagnostics: Vec::new(),
            has_fatal_error: false,
        }
    }

    /// Finalize tokenization: detect unclosed elements and emit errors.
    ///
    /// Must be called after all tokenizer events have been processed.
    /// Checks if any elements were opened but never closed, emitting
    /// Vue-compatible `X_MISSING_END_TAG` errors for each.
    pub fn finalize(&mut self, bytes: &[u8]) {
        use crate::syntax::plugin::{CompilerErrorCode, Diagnostic};

        // Check for unclosed elements (iterate in reverse = innermost first).
        for &(start, name_end) in self.open_tag_stack.iter().rev() {
            let tag_name = std::str::from_utf8(&bytes[start as usize + 1..name_end as usize])
                .unwrap_or("unknown");
            self.diagnostics.push(
                Diagnostic::error_with_message(
                    "syntax",
                    CompilerErrorCode::XMissingEndTag,
                    format!("Element <{}> is missing end tag.", tag_name),
                )
                .with_span(crate::common::Span {
                    start,
                    end: name_end,
                }),
            );
            self.has_fatal_error = true;
        }
    }

    pub fn handle(&mut self, event: &TokenizerEvent<'alloc>, ctx: &SyntaxPluginContext<'alloc>) {
        match event {
            // Element events
            TokenizerEvent::OpenTagName { start, end } => {
                self.handle_tag_open(*start, *end, ctx);
            }
            TokenizerEvent::OpenTagEnd { end } => {
                self.handle_tag_close(*end, false);
            }
            TokenizerEvent::SelfClosingTag { end } => {
                self.handle_tag_close(*end, true);
            }
            TokenizerEvent::CloseTag {
                start,
                end,
                name_end,
            } => {
                self.handle_close_tag(*start, *end, *name_end, ctx);
            }
            // Prop events
            TokenizerEvent::AttribName { start, end } => {
                self.handle_attribute_name(*start, *end);
            }
            TokenizerEvent::DirName { start, end } => {
                self.handle_directive_name(*start, *end);
            }
            TokenizerEvent::DirArg {
                start,
                end,
                is_dynamic,
            } => {
                self.handle_directive_arg(*start, *end, *is_dynamic);
            }
            TokenizerEvent::DirModifier { start, end } => {
                self.handle_directive_modifier(*start, *end);
            }

            TokenizerEvent::AttribData { start, .. } => {
                self.handle_attribute_value(*start);
            }
            TokenizerEvent::AttribEnd { end, quote } => {
                self.handle_attribute_end(*end, *quote, ctx);
            }

            // leafs
            TokenizerEvent::Text { start, end } => {
                self.handle_text(*start, *end, false);
            }
            TokenizerEvent::TextEntity { start, end } => {
                self.handle_text(*start, *end, true);
            }
            TokenizerEvent::Comment {
                start,
                end,
                content_end,
                content_start,
            } => {
                self.handle_comment(*start, *end, *content_start, *content_end);
            }

            TokenizerEvent::Interpolation {
                start,
                end,
                delimiter_close_len,
                delimiter_open_len,
            } => {
                self.handle_interpolation(
                    *start,
                    *end,
                    *start + *delimiter_open_len as u32,
                    *end - *delimiter_close_len as u32,
                );
            }

            _ => {}
        }
    }

    // Element handling logic:

    #[inline]
    fn handle_tag_open(&mut self, start: u32, name_end: u32, ctx: &SyntaxPluginContext<'alloc>) {
        let name = &ctx.bytes[start as usize + 1..name_end as usize];

        if self.nested_level == 0 && !self.template_mode {
            // handle root
            let kind = Self::resolve_root_kind(name);

            if kind == RootNodeKind::Script {
                self.scripts_found += 1;
            }

            self.last_root_node = Some(RootNodeOpenTag::Start(RootNodeOpenTagStart {
                kind,
                start,
                name_end,
            }));
            self.nested_level += 1;
            self.last_parent_id = start;
            self.parent_stack.push(start);
        } else {
            let kind = Self::resolve_tag_kind(name, ctx);
            // handle element
            let is_void_element = kind == ElementKind::Element && (ctx.options.is_void_tag)(name);

            let ev = ElementOpenTagStart {
                kind,
                start,
                name_end,
                parent_id: self.last_parent_id,
                is_void_element,

                nested_level: self.nested_level,
                patch_flag: PatchFlag::empty(),
                dynamic_props: Vec::new(),
                has_ref: false,
                has_vnode_hook: false,
            };
            self.last_event_open_tag = Some(ev.clone());

            if !is_void_element {
                self.nested_level += 1;
                self.parent_stack.push(self.last_parent_id);
                self.last_parent_id = start;
                self.open_tag_stack.push((start, name_end));
            }

            self.events.push(Event::OpenTag(ev));
        }
    }

    #[inline]
    fn handle_tag_close(&mut self, end: u32, is_self_closing: bool) {
        if self.last_event_open_tag.is_none() {
            // root
            if let Some(root) = self.last_root_node.take() {
                match root {
                    RootNodeOpenTag::Start(root) => {
                        let ev = RootNodeOpenTagEnd {
                            kind: root.kind,
                            start: root.start,
                            name_end: root.name_end,
                            end,

                            is_self_closing,
                        };

                        if !is_self_closing {
                            self.last_root_node = Some(RootNodeOpenTag::End(ev.clone()));
                        } else {
                            // Self-closing root: decrement nested_level so the next
                            // top-level tag is also treated as a root node.
                            self.nested_level -= 1;
                            self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
                        }

                        if ev.kind == RootNodeKind::Template {
                            self.has_template = true;
                            self.template_block = Some((ev.start, 0));
                        }

                        if ev.kind == RootNodeKind::Script {
                            self.root_script_events.push(Event::RootOpenTagEnd(ev));
                        } else {
                            self.events.push(Event::RootOpenTagEnd(ev));
                        }
                    }
                    _ => unreachable!(),
                }
            }
        } else {
            // element
            if let Some(open_tag) = self.last_event_open_tag.take() {
                let ev = ElementOpenTagEnd {
                    kind: open_tag.kind,
                    start: open_tag.start,
                    name_end: open_tag.name_end,
                    end,
                    parent_id: open_tag.parent_id,
                    is_void_element: open_tag.is_void_element,
                    nested_level: open_tag.nested_level,
                    patch_flag: open_tag.patch_flag,
                    dynamic_props: open_tag.dynamic_props,
                    has_ref: open_tag.has_ref,
                    has_vnode_hook: open_tag.has_vnode_hook,

                    is_self_closing: is_self_closing || open_tag.is_void_element, // for void elements, treat as self-closing
                };

                if is_self_closing && !open_tag.is_void_element {
                    // Only decrement for non-void self-closing elements.
                    // Void elements never increment nested_level in handle_tag_open,
                    // so decrementing here would corrupt nesting for siblings.
                    if self.nested_level == 0 {
                        self.diagnostics.push(
                            crate::syntax::plugin::Diagnostic::error(
                                "syntax",
                                crate::syntax::plugin::CompilerErrorCode::XInvalidEndTag,
                            )
                            .with_span(crate::common::Span {
                                start: open_tag.start,
                                end: ev.end,
                            }),
                        );
                        self.has_fatal_error = true;
                    } else {
                        self.nested_level -= 1;
                        self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
                        self.open_tag_stack.pop();
                    }
                }

                self.events.push(Event::OpenTagEnd(ev));
            }
        }
    }

    #[inline]
    fn handle_close_tag(
        &mut self,
        start: u32,
        end: u32,
        name_end: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        use crate::syntax::plugin::{CompilerErrorCode, Diagnostic};

        if self.nested_level == 0 {
            // X_INVALID_END_TAG: close tag at root level with no open tag
            self.diagnostics.push(
                Diagnostic::error("syntax", CompilerErrorCode::XInvalidEndTag)
                    .with_span(crate::common::Span { start, end }),
            );
            self.has_fatal_error = true;
            return;
        }

        let name = &ctx.bytes[start as usize + 2..name_end as usize];

        // Case 1: Close tag immediately follows open tag (empty element, e.g. <div></div>
        // where no children/text were emitted between open and close)
        if let Some(open_tag) = self.last_event_open_tag.take() {
            let open_name = &ctx.bytes[open_tag.start as usize + 1..open_tag.name_end as usize];
            if open_name != name {
                // Restore open tag state so it's available for the correct close tag.
                self.last_event_open_tag = Some(open_tag);
                // X_INVALID_END_TAG: close tag doesn't match last open tag
                self.diagnostics.push(
                    Diagnostic::error("syntax", CompilerErrorCode::XInvalidEndTag)
                        .with_span(crate::common::Span { start, end }),
                );
                self.has_fatal_error = true;
                return;
            }

            let ev = ElementCloseTag {
                kind: open_tag.kind,
                start,
                name_end,
                end,
                parent_id: open_tag.parent_id,
                nested_level: open_tag.nested_level,
                is_void_element: open_tag.is_void_element,
            };
            self.events.push(Event::CloseTag(ev));

            self.nested_level -= 1;
            self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
            self.open_tag_stack.pop();
        }
        // Case 2: Closing a root node — only when we're at depth 1 (directly inside root)
        else if self.nested_level == 1 && !self.template_mode {
            if let Some(root) = self.last_root_node.take() {
                match root {
                    RootNodeOpenTag::End(root) => {
                        let root_name = &ctx.bytes[root.start as usize + 1..root.name_end as usize];
                        if root_name != name {
                            // X_INVALID_END_TAG: close tag doesn't match root
                            self.diagnostics.push(
                                Diagnostic::error("syntax", CompilerErrorCode::XInvalidEndTag)
                                    .with_span(crate::common::Span { start, end }),
                            );
                            self.has_fatal_error = true;
                            self.last_root_node = Some(RootNodeOpenTag::End(root));
                            return;
                        }

                        let ev = RootNodeCloseTag {
                            kind: root.kind.clone(),
                            start,
                            name_end,
                            end,
                        };

                        // Track block end positions for codegen.
                        if root.kind == RootNodeKind::Template {
                            if let Some(ref mut tb) = self.template_block {
                                tb.1 = end;
                            }
                        } else if root.kind == RootNodeKind::Script {
                            self.script_block_end = Some(end);
                        }

                        // Route script close tag to root_script_events
                        if root.kind == RootNodeKind::Script {
                            self.root_script_events.push(Event::RootCloseTag(ev));
                        } else {
                            self.events.push(Event::RootCloseTag(ev));
                        }

                        self.nested_level -= 1;
                        self.last_parent_id = NO_PARENT;
                    }
                    _ => unreachable!(),
                }
            }
        }
        // Case 3: Normal nested element close (non-empty element with children/text)
        else {
            // Validate close tag against the innermost open element.
            if let Some(&(open_start, open_name_end)) = self.open_tag_stack.last() {
                let open_name = &ctx.bytes[open_start as usize + 1..open_name_end as usize];
                if open_name != name {
                    // X_INVALID_END_TAG: close tag doesn't match innermost open element.
                    // Don't pop — finalize() will catch the unclosed element.
                    self.diagnostics.push(
                        Diagnostic::error("syntax", CompilerErrorCode::XInvalidEndTag)
                            .with_span(crate::common::Span { start, end }),
                    );
                    self.has_fatal_error = true;
                    return;
                }
            }

            let kind = Self::resolve_tag_kind(name, ctx);

            self.nested_level -= 1;
            self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
            self.open_tag_stack.pop();

            let ev = ElementCloseTag {
                kind,
                start,
                name_end,
                end,
                parent_id: self.last_parent_id,
                nested_level: self.nested_level,
                is_void_element: false,
            };
            self.events.push(Event::CloseTag(ev));
        }
    }

    #[inline]
    fn resolve_root_kind(name: &[u8]) -> RootNodeKind {
        match name {
            b"template" => RootNodeKind::Template,
            b"script" => RootNodeKind::Script,
            b"style" => RootNodeKind::Style,
            _ => RootNodeKind::Unknown,
        }
    }

    #[inline]
    fn resolve_tag_kind(name: &[u8], ctx: &SyntaxPluginContext<'alloc>) -> ElementKind {
        match name {
            b"component" => ElementKind::DynamicComponent,
            b"template" => ElementKind::Template,
            b"slot" => ElementKind::SlotOutlet,
            _ if (ctx.options.is_custom_element)(name) => ElementKind::CustomComponent,
            _ if is_tag_name_component(name) => ElementKind::Component, // PascalCase => component
            _ if is_html_tag(name) || is_svg_tag(name) || is_mathml_tag(name) => {
                ElementKind::Element
            }
            _ => ElementKind::Component, // default to component if it doesn't match known tags
        }
    }
    // /Element

    // Prop handling logic:

    fn handle_attribute_name(&mut self, start: u32, name_end: u32) {
        self.current_prop = Some(PropTempState {
            start,
            name_end,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            modifiers: None,
        });
    }

    fn handle_directive_name(&mut self, start: u32, name_end: u32) {
        self.current_prop = Some(PropTempState {
            start,
            name_end,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            modifiers: None,
        });
    }

    fn handle_directive_arg(&mut self, arg_start: u32, arg_end: u32, is_dynamic: bool) {
        if let Some(prop) = &mut self.current_prop {
            prop.arg_start = Some(arg_start);
            prop.arg_end = Some(arg_end);
            prop.is_dynamic = Some(is_dynamic);

            if !prop.is_directive {
                self.diagnostics.push(
                    crate::syntax::plugin::Diagnostic::warning(
                        "syntax",
                        crate::syntax::plugin::CompilerErrorCode::XMissingDirectiveName,
                    )
                    .with_span(crate::common::Span {
                        start: arg_start,
                        end: arg_end,
                    }),
                );
            }
        }
    }
    fn handle_directive_modifier(&mut self, modifier_start: u32, modifier_end: u32) {
        if let Some(prop) = &mut self.current_prop {
            let modifier_span = Span::new(modifier_start, modifier_end);
            if let Some(modifiers) = &mut prop.modifiers {
                modifiers.push(modifier_span);
            } else {
                prop.modifiers = Some(vec![modifier_span]);
            }
        }
    }

    fn handle_attribute_value(&mut self, value_start: u32) {
        if let Some(prop) = &mut self.current_prop {
            prop.value_start = Some(value_start);
        }
    }

    fn handle_attribute_end(
        &mut self,
        end: u32,
        quote: QuoteType,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let Some(state) = self.current_prop.take() else {
            self.diagnostics.push(
                crate::syntax::plugin::Diagnostic::warning(
                    "syntax",
                    crate::syntax::plugin::CompilerErrorCode::MissingAttributeValue,
                )
                .with_span(crate::common::Span { start: end, end }),
            );
            return;
        };

        let value = match state.value_start {
            Some(v_start) => {
                // NOTE: Only apply -1 adjustment for quoted values (Single, Double).
                // Unquoted values should use the full range from tokenizer.
                let value_end = match quote {
                    QuoteType::Single | QuoteType::Double => {
                        // For quoted values, end points after the closing quote,
                        // so subtract 1 to exclude the quote
                        if end > 0 {
                            end - 1
                        } else {
                            state.name_end
                        }
                    }
                    QuoteType::Unquoted => {
                        // For unquoted values, use the full range
                        end
                    }
                    QuoteType::NoValue => {
                        // No value case
                        state.name_end
                    }
                };
                Some(Span {
                    start: v_start,
                    end: value_end,
                })
            }
            None => None,
        };

        let arg = match state.arg_start {
            Some(a_start) => Some(Span {
                start: a_start,
                end: state.arg_end.unwrap_or(a_start), // fallback to start if end is missing
            }),
            None => None,
        };

        let name = &ctx.bytes[state.start as usize..state.name_end as usize];

        let ev = Prop {
            kind: self.resolve_prop_kind(name, arg, state.is_directive, ctx),
            has_dynamic_arg: state.is_dynamic.unwrap_or(false),
            element_id: self.last_parent_id,

            start: state.start,
            end,
            name_end: state.name_end,
            value,
            arg,
            modifiers: state.modifiers,
            quote: Some(quote),

            is_directive: state.is_directive,
        };

        // estimate the patch_flag based on props
        // note patch_flags can also be changed by children
        // Vapor uses renderEffect instead of patch flags, so skip estimation.
        if !self.is_vapor {
            if let Some(parent) = &mut self.last_event_open_tag {
                estimate_patch_flag(parent, &ev, ctx.bytes);
            }
        }

        if self.last_event_open_tag.is_none() && self.last_root_node.is_some() {
            // in root node, treat as root prop
            if name == b"lang" {
                if let Some(v) = value {
                    let lang =
                        ScriptLanguage::from_bytes(&ctx.bytes[v.start as usize..v.end as usize]);
                    self.events.push(Event::Lang(ScriptLang { lang }));
                }
            }
        }

        // Route script root props to root_script_events so the script pipeline
        // has all the props it needs (setup, lang, etc.)
        if self.last_event_open_tag.is_none() {
            if let Some(RootNodeOpenTag::Start(ref root)) = self.last_root_node {
                if root.kind == RootNodeKind::Script {
                    if name == b"setup" {
                        self.script_setup_start = Some(root.start);
                    }
                    self.root_script_events.push(Event::Prop(ev));
                    return;
                }
                if root.kind == RootNodeKind::Template && name == b"vapor" {
                    self.has_vapor_template = true;
                }
                if root.kind == RootNodeKind::Style {
                    if name == b"scoped" {
                        self.has_style_scope = true;
                    } else if name == b"module" {
                        self.has_style_module = true;
                    }
                }
            }
        }

        self.events.push(Event::Prop(ev));
    }

    #[inline]
    fn resolve_prop_kind(
        &self,
        name: &[u8],
        arg: Option<Span>,
        is_directive: bool,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> PropKind {
        if is_directive {
            if name == b"v-bind" || name == b":" {
                match arg {
                    None => PropKind::BindSpread,
                    Some(a) => {
                        let arg_name = &ctx.bytes[a.start as usize..a.end as usize];
                        if arg_name == b"class" {
                            PropKind::ClassBind
                        } else if arg_name == b"style" {
                            PropKind::StyleBind
                        } else {
                            PropKind::Bind
                        }
                    }
                }
            } else if name == b"v-on" || name == b"@" {
                if arg.is_none() {
                    PropKind::OnSpread
                } else {
                    PropKind::On
                }
            } else if name == b"v-model" {
                PropKind::Model
            } else if name == b"v-if" {
                PropKind::If
            } else if name == b"v-else-if" {
                PropKind::ElseIf
            } else if name == b"v-else" {
                PropKind::Else
            } else if name == b"v-for" {
                PropKind::For
            } else if name == b"v-slot" || name == b"#" {
                PropKind::Slot
            } else if name == b"v-show" {
                PropKind::Show
            } else if name == b"v-html" {
                PropKind::Html
            } else if name == b"v-text" {
                PropKind::Text
            } else if name == b"v-once" {
                PropKind::Once
            } else if name.starts_with(b"v-") {
                PropKind::Directive
            } else {
                // Likely unreachable — tokenizer should only emit directive events for valid directives
                PropKind::Directive
            }
        } else if name == b"class" {
            PropKind::ClassValue
        } else if name == b"style" {
            PropKind::StyleValue
        } else {
            PropKind::Value
        }
    }
    // /Prop handling logic

    // other elements

    fn handle_text(&mut self, start: u32, end: u32, has_entity: bool) {
        self.events.push(Event::Text(Text {
            parent_id: self.last_parent_id,
            start,
            end,
            has_entity,
        }));
    }

    fn handle_comment(&mut self, start: u32, end: u32, content_start: u32, content_end: u32) {
        self.events.push(Event::Comment(Comment {
            parent_id: self.last_parent_id,
            start,
            end,
            content: Span::new(content_start, content_end),
        }));
    }

    fn handle_interpolation(&mut self, start: u32, end: u32, content_start: u32, content_end: u32) {
        self.events.push(Event::Interpolation(Interpolation {
            parent_id: self.last_parent_id,
            start,
            end,
            content: Span::new(content_start, content_end),
        }));
    }

    // /other elements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::plugin::{SyntaxPluginContext, SyntaxPluginOptions};

    use crate::tokenizer::byte::tokenize;

    /// Helper macro: tokenize input, run through Syntax, execute body with `events` in scope.
    ///
    /// Uses a raw pointer to decouple Syntax's mutable borrow from the events vector,
    /// allowing us to read events after Syntax is dropped. This is safe because:
    /// - The events in the vec only borrow from `tokenizer_events` and `input`, both of
    ///   which outlive the entire macro invocation.
    /// - Syntax is fully dropped (via scope) before we read the events.
    macro_rules! with_syntax_events {
        ($input:expr, $template_mode:expr, |$events:ident| $body:block) => {{
            let input: &str = $input;
            let mut tokenizer_events = Vec::new();
            tokenize(input.as_bytes(), |event| tokenizer_events.push(event));

            let options = SyntaxPluginOptions::default();
            let mut ctx = SyntaxPluginContext {
                input,
                bytes: input.as_bytes(),
                options: &options,
                diagnostics: Vec::new(),
            };

            let mut syntax = Syntax::new($template_mode);
            for event in &tokenizer_events {
                syntax.handle(event, &mut ctx);
            }

            let events_storage = syntax.events();
            let $events = &events_storage;
            $body
        }};
    }

    /// Like `with_syntax_events!` but also provides access to the `Syntax` struct
    /// after events have been collected (for testing metadata getters).
    macro_rules! with_syntax {
        ($input:expr, $template_mode:expr, |$syntax:ident, $events:ident| $body:block) => {{
            let input: &str = $input;
            let mut tokenizer_events = Vec::new();
            tokenize(input.as_bytes(), |event| tokenizer_events.push(event));

            let options = SyntaxPluginOptions::default();
            let mut ctx = SyntaxPluginContext {
                input,
                bytes: input.as_bytes(),
                options: &options,
                diagnostics: Vec::new(),
            };

            let mut $syntax = Syntax::new($template_mode);
            for event in &tokenizer_events {
                $syntax.handle(event, &mut ctx);
            }

            let events_storage = $syntax.events();
            let $events = &events_storage;
            $body
        }};
    }

    use crate::syntax::plugin::CompilerErrorCode;

    /// Helper: run input through tokenizer → Syntax → finalize, return diagnostics.
    fn get_diagnostics(input: &str) -> Vec<crate::syntax::plugin::Diagnostic> {
        get_diagnostics_with_mode(input, false)
    }

    /// Helper: run input through tokenizer → Syntax → finalize, return diagnostics.
    /// `template_mode=true` skips root node handling (treats input as template content).
    fn get_diagnostics_with_mode(
        input: &str,
        template_mode: bool,
    ) -> Vec<crate::syntax::plugin::Diagnostic> {
        let mut tokenizer_events = Vec::new();
        tokenize(input.as_bytes(), |event| tokenizer_events.push(event));

        let options = SyntaxPluginOptions::default();
        let ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &options,
            diagnostics: Vec::new(),
        };

        let mut syntax = Syntax::new(template_mode);
        for event in &tokenizer_events {
            syntax.handle(event, &ctx);
        }
        syntax.finalize(input.as_bytes());

        syntax.take_diagnostics()
    }

    // ==================== Diagnostics: Vue-compatible error events ====================

    /// @ai-generated — X_INVALID_END_TAG: close tag with no matching open tag.
    #[test]
    fn test_invalid_end_tag_unmatched() {
        // </orphan> at root level → X_INVALID_END_TAG (no open tag exists)
        let diags = get_diagnostics_with_mode("</orphan>", true);
        assert!(
            diags
                .iter()
                .any(|d| d.code == CompilerErrorCode::XInvalidEndTag),
            "Expected X_INVALID_END_TAG for </orphan>, got: {:?}",
            diags
        );
    }

    /// @ai-generated — X_INVALID_END_TAG: close tag doesn't match open tag.
    #[test]
    fn test_invalid_end_tag_mismatched() {
        // <div></span> → X_INVALID_END_TAG (span != div)
        let diags = get_diagnostics_with_mode("<div></span>", true);
        assert!(
            diags
                .iter()
                .any(|d| d.code == CompilerErrorCode::XInvalidEndTag),
            "Expected X_INVALID_END_TAG for <div></span>, got: {:?}",
            diags
        );
    }

    /// @ai-generated — X_MISSING_END_TAG: element opened but never closed.
    #[test]
    fn test_missing_end_tag_unclosed_element() {
        // <div> without </div> → X_MISSING_END_TAG
        let diags = get_diagnostics_with_mode("<div>", true);
        assert!(
            diags
                .iter()
                .any(|d| d.code == CompilerErrorCode::XMissingEndTag),
            "Expected X_MISSING_END_TAG for unclosed <div>, got: {:?}",
            diags
        );
        // Check that the message includes the tag name
        let missing = diags
            .iter()
            .find(|d| d.code == CompilerErrorCode::XMissingEndTag)
            .unwrap();
        assert!(
            missing.message.contains("<div>"),
            "Error message should include tag name, got: {}",
            missing.message
        );
    }

    /// @ai-generated — X_MISSING_END_TAG: nested unclosed elements.
    #[test]
    fn test_missing_end_tag_nested() {
        // <div><span> — both unclosed → two X_MISSING_END_TAG errors
        let diags = get_diagnostics_with_mode("<div><span>", true);
        let missing: Vec<_> = diags
            .iter()
            .filter(|d| d.code == CompilerErrorCode::XMissingEndTag)
            .collect();
        assert_eq!(
            missing.len(),
            2,
            "Expected 2 X_MISSING_END_TAG errors for <div><span>, got: {:?}",
            diags
        );
    }

    /// @ai-generated — X_MISSING_END_TAG in SFC mode: unclosed element inside <template>.
    #[test]
    fn test_missing_end_tag_in_sfc_template() {
        // In SFC mode, <template><div></template> — div is unclosed.
        // This produces X_INVALID_END_TAG (</template> doesn't match <div>)
        // and X_MISSING_END_TAG (finalize detects <div> still open).
        let diags = get_diagnostics("<template><div></template>");
        assert!(
            diags
                .iter()
                .any(|d| d.code == CompilerErrorCode::XMissingEndTag),
            "Expected X_MISSING_END_TAG for unclosed <div> inside <template>, got: {:?}",
            diags
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == CompilerErrorCode::XInvalidEndTag),
            "Expected X_INVALID_END_TAG for </template> mismatching <div>, got: {:?}",
            diags
        );
    }

    /// @ai-generated — Void elements don't need closing tags.
    #[test]
    fn test_void_elements_no_missing_end_tag() {
        // <br> is void — should NOT produce X_MISSING_END_TAG
        let diags = get_diagnostics_with_mode("<div><br></div>", true);
        assert!(
            !diags
                .iter()
                .any(|d| d.code == CompilerErrorCode::XMissingEndTag),
            "Void element <br> should not produce X_MISSING_END_TAG, got: {:?}",
            diags
        );
    }

    /// @ai-generated — Valid template produces no diagnostics.
    #[test]
    fn test_valid_template_no_errors() {
        let diags = get_diagnostics_with_mode("<div><span></span></div>", true);
        assert!(
            diags.is_empty(),
            "Valid template should produce no errors, got: {:?}",
            diags
        );
    }

    // ==================== Bug 1: Close tags for nested elements are never emitted ====================

    /// @ai-generated - Demonstrates that close tags for nested elements inside a root
    /// node are never emitted. When processing <template><div></div></template>,
    /// the </div> close tag should produce an Event::CloseTag, but it doesn't because
    /// handle_close_tag only checks last_event_open_tag and last_root_node.
    #[test]
    fn test_bug_nested_close_tag_inside_root_is_emitted() {
        with_syntax_events!("<template><div></div></template>", false, |events| {
            let close_tag_count = events
                .iter()
                .filter(|e| matches!(e, Event::CloseTag(_)))
                .count();

            assert_eq!(
                close_tag_count, 1,
                "Expected 1 CloseTag event for </div>, got {}. \
                 Close tags for nested elements inside a root node are not emitted.",
                close_tag_count
            );
        });
    }

    /// @ai-generated - Demonstrates the same bug with deeper nesting:
    /// <template><div><span></span></div></template>
    /// Both </span> and </div> should produce CloseTag events.
    #[test]
    fn test_bug_deeply_nested_close_tags_inside_root() {
        with_syntax_events!(
            "<template><div><span></span></div></template>",
            false,
            |events| {
                let close_tag_count = events
                    .iter()
                    .filter(|e| matches!(e, Event::CloseTag(_)))
                    .count();

                assert_eq!(
                    close_tag_count, 2,
                    "Expected 2 CloseTag events (</span> and </div>), got {}. \
                     Nested close tags are silently dropped.",
                    close_tag_count
                );
            }
        );
    }

    /// @ai-generated - Demonstrates that last_root_node gets corrupted when a nested
    /// close tag is processed: the .take() on last_root_node consumes it, so the
    /// actual root close tag (</template>) also fails.
    #[test]
    fn test_bug_root_close_tag_lost_after_nested_close() {
        with_syntax_events!("<template><div></div></template>", false, |events| {
            let root_close_count = events
                .iter()
                .filter(|e| matches!(e, Event::RootCloseTag(_)))
                .count();

            assert_eq!(
                root_close_count, 1,
                "Expected 1 RootCloseTag for </template>, got {}. \
                 The root close tag is lost because last_root_node was consumed \
                 when trying to match it against the nested </div>.",
                root_close_count
            );
        });
    }

    /// @ai-generated - Verifies close tags work in template_mode (no root handling).
    /// This bypasses the root node code path, so close tags should work correctly.
    #[test]
    fn test_close_tags_work_in_template_mode() {
        with_syntax_events!("<div><span></span></div>", true, |events| {
            let close_tag_count = events
                .iter()
                .filter(|e| matches!(e, Event::CloseTag(_)))
                .count();

            // In template_mode, everything is treated as nested, so close tags
            // go through the last_event_open_tag path. For empty <span></span>,
            // the close tag immediately follows the open tag, so it might work.
            // But for <div>..content..</div>, last_event_open_tag is cleared
            // by the time </div> is reached.
            assert_eq!(
                close_tag_count, 2,
                "Expected 2 CloseTag events in template_mode, got {}.",
                close_tag_count
            );
        });
    }

    // ==================== Bug 2: resolve_prop_kind checks name instead of arg ====================

    /// @ai-generated - Demonstrates that :class="active" is misclassified as PropKind::Bind
    /// instead of PropKind::ClassBind. The resolve_prop_kind method checks `name == b"class"`
    /// after already matching `name == b":" || name == b"v-bind"`, so the class/style
    /// branches are dead code.
    #[test]
    fn test_bug_bind_class_should_be_class_bind() {
        let input = r#"<template><div :class="active"></div></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            let prop = &props[0];

            // :class="active" should be classified as ClassBind, not Bind
            assert!(
                matches!(prop.kind, PropKind::ClassBind),
                "Expected :class to be PropKind::ClassBind, got {:?}. \
                 resolve_prop_kind checks `name == b\"class\"` after matching \
                 `name == b\":\"`, so name can never be \"class\" — dead code.",
                prop.kind
            );
        });
    }

    /// @ai-generated - Same bug for :style bindings.
    #[test]
    fn test_bug_bind_style_should_be_style_bind() {
        let input = r#"<template><div :style="styles"></div></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            let prop = &props[0];

            assert!(
                matches!(prop.kind, PropKind::StyleBind),
                "Expected :style to be PropKind::StyleBind, got {:?}. \
                 Same dead code issue as :class.",
                prop.kind
            );
        });
    }

    // ==================== Bug 3: OpenTagEnd missing +1 offset ====================

    /// @ai-generated - Verifies that OpenTagEnd.end includes the '>' character,
    /// so that input[start..end] gives the full opening tag like "<div>".
    #[test]
    fn test_open_tag_end_offset_includes_closing_bracket() {
        let input = "<template><div></div></template>";
        with_syntax_events!(input, false, |events| {
            let open_tag_ends: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::OpenTagEnd(ev) => Some(ev),
                    _ => None,
                })
                .collect();

            assert_eq!(open_tag_ends.len(), 1, "Expected 1 OpenTagEnd for <div>");
            let ev = open_tag_ends[0];

            // The end offset should point PAST the '>', so input[start..end] == "<div>"
            let tag_slice = &input[ev.start as usize..ev.end as usize];
            assert_eq!(
                tag_slice, "<div>",
                "OpenTagEnd offsets should give '<div>' but got '{}'.",
                tag_slice
            );
        });
    }

    // ==================== Bug 4: Static class/style use Bind variant instead of Value ====================

    /// @ai-generated - Demonstrates that static class="foo" is classified as ClassBind
    /// instead of ClassValue. The PropKind enum has ClassValue/StyleValue variants
    /// for static attributes but they're never used.
    #[test]
    fn test_bug_static_class_should_be_class_value() {
        let input = r#"<template><div class="foo"></div></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if !p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            // Filter to just the class prop (exclude any root-level props)
            let class_props: Vec<_> = props
                .iter()
                .filter(|p| {
                    let name = &input[p.start as usize..p.name_end as usize];
                    name == "class"
                })
                .collect();

            assert_eq!(class_props.len(), 1, "Expected 1 class prop");
            let prop = class_props[0];

            // Static class should be ClassValue, not ClassBind
            assert!(
                matches!(prop.kind, PropKind::ClassValue),
                "Expected static class to be PropKind::ClassValue, got {:?}. \
                 PropKind::ClassValue and StyleValue variants exist but are never assigned.",
                prop.kind
            );
        });
    }

    // ==================== Issue #4: Self-closing root doesn't decrement nested_level ====================

    /// @ai-generated - Self-closing root (<template />) should decrement nested_level
    /// so the next top-level tag is also treated as a root node.
    #[test]
    fn test_bug_self_closing_root_decrement() {
        with_syntax_events!("<template /><style></style>", false, |events| {
            let root_open_count = events
                .iter()
                .filter(|e| matches!(e, Event::RootOpenTagEnd(_)))
                .count();

            assert_eq!(
                root_open_count, 2,
                "Expected 2 RootOpenTagEnd events (<template/> and <style>), got {}. \
                 Self-closing root doesn't decrement nested_level, causing <style> \
                 to be treated as a nested element instead of a root.",
                root_open_count
            );
        });
    }

    // ==================== Issue #5: Void self-closing double-decrements nested_level ====================

    /// @ai-generated - A void self-closing element like <br /> should not decrement
    /// nested_level since it was never incremented for void elements.
    #[test]
    fn test_bug_void_self_closing_no_double_decrement() {
        with_syntax_events!("<template><br /><div></div></template>", false, |events| {
            let open_tag_count = events
                .iter()
                .filter(|e| matches!(e, Event::OpenTag(_)))
                .count();

            assert_eq!(
                open_tag_count, 2,
                "Expected 2 OpenTag events (<br> and <div>), got {}. \
                 Void self-closing <br /> incorrectly decrements nested_level, \
                 causing <div> to be treated as a root instead of an element.",
                open_tag_count
            );
        });
    }

    /// @ai-generated - After a void self-closing, the root close tag should still work.
    #[test]
    fn test_void_self_closing_preserves_root_close() {
        with_syntax_events!("<template><br /><div></div></template>", false, |events| {
            let root_close_count = events
                .iter()
                .filter(|e| matches!(e, Event::RootCloseTag(_)))
                .count();

            assert_eq!(
                root_close_count, 1,
                "Expected 1 RootCloseTag for </template>, got {}. \
                 Void self-closing corrupted nested_level, preventing root close.",
                root_close_count
            );
        });
    }

    // ==================== Issue #6: Missing directive kinds in resolve_prop_kind ====================

    /// @ai-generated - v-show should be classified as PropKind::Show, not PropKind::Directive.
    #[test]
    fn test_bug_v_show_should_be_show_kind() {
        let input = r#"<template><div v-show="visible"></div></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            assert!(
                matches!(props[0].kind, PropKind::Show),
                "Expected v-show to be PropKind::Show, got {:?}",
                props[0].kind
            );
        });
    }

    /// @ai-generated - v-html should be classified as PropKind::Html, not PropKind::Directive.
    #[test]
    fn test_bug_v_html_should_be_html_kind() {
        let input = r#"<template><div v-html="content"></div></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            assert!(
                matches!(props[0].kind, PropKind::Html),
                "Expected v-html to be PropKind::Html, got {:?}",
                props[0].kind
            );
        });
    }

    /// @ai-generated - v-text should be classified as PropKind::Text, not PropKind::Directive.
    #[test]
    fn test_bug_v_text_should_be_text_kind() {
        let input = r#"<template><div v-text="msg"></div></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            assert!(
                matches!(props[0].kind, PropKind::Text),
                "Expected v-text to be PropKind::Text, got {:?}",
                props[0].kind
            );
        });
    }

    /// @ai-generated - v-slot should be classified as PropKind::Slot, not PropKind::Directive.
    #[test]
    fn test_bug_v_slot_should_be_slot_kind() {
        let input = r#"<template><MyComp v-slot:default="props"></MyComp></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            assert!(
                matches!(props[0].kind, PropKind::Slot),
                "Expected v-slot to be PropKind::Slot, got {:?}",
                props[0].kind
            );
        });
    }

    /// @ai-generated - # shorthand should be classified as PropKind::Slot.
    #[test]
    fn test_bug_hash_slot_should_be_slot_kind() {
        let input = r#"<template><MyComp #default="props"></MyComp></template>"#;
        with_syntax_events!(input, false, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Prop(p) if p.is_directive => Some(p),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive prop");
            assert!(
                matches!(props[0].kind, PropKind::Slot),
                "Expected # shorthand to be PropKind::Slot, got {:?}",
                props[0].kind
            );
        });
    }

    // ==================== Entity handling ====================

    /// @ai-generated - HTML entities like &amp; should be emitted as Text events
    /// with has_entity=true, while regular text has has_entity=false.
    #[test]
    fn test_entity_emitted_as_text_with_flag() {
        let input = "<template>hello &amp; world</template>";
        with_syntax_events!(input, false, |events| {
            let texts: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Text(t) => Some(t),
                    _ => None,
                })
                .collect();

            // Should have 3 text events: "hello ", "&amp;", " world"
            assert_eq!(
                texts.len(),
                3,
                "Expected 3 Text events, got {}",
                texts.len()
            );

            // Regular text: has_entity=false
            assert!(
                !texts[0].has_entity,
                "Plain text should have has_entity=false"
            );

            // Entity text: has_entity=true
            assert!(
                texts[1].has_entity,
                "Entity text should have has_entity=true"
            );
            let entity_slice = &input[texts[1].start as usize..texts[1].end as usize];
            assert_eq!(entity_slice, "&amp;", "Entity span should be '&amp;'");

            // Regular text: has_entity=false
            assert!(
                !texts[2].has_entity,
                "Plain text should have has_entity=false"
            );
        });
    }

    // ==================== SFC metadata getters ====================

    /// @ai-generated — Syntax should track script setup status during tokenization.
    #[test]
    fn test_syntax_tracks_script_setup() {
        let input = r#"<script setup lang="ts">const x = 1;</script><template><div/></template>"#;
        with_syntax!(input, false, |syntax, _events| {
            assert!(
                syntax.script_setup_start().is_some(),
                "Should detect script setup"
            );
            assert_eq!(syntax.script_setup_start().unwrap(), 0);
        });
    }

    /// @ai-generated — Syntax should NOT report setup for plain script blocks.
    #[test]
    fn test_syntax_no_setup_for_plain_script() {
        let input = r#"<script>const x = 1;</script><template><div/></template>"#;
        with_syntax!(input, false, |syntax, _events| {
            assert!(
                syntax.script_setup_start().is_none(),
                "Plain script should not be detected as setup"
            );
        });
    }

    /// @ai-generated — Syntax should track template and vapor presence.
    #[test]
    fn test_syntax_tracks_template_and_vapor() {
        let input = r#"<script setup>const x = 1;</script><template vapor><div/></template>"#;
        with_syntax!(input, false, |syntax, _events| {
            assert!(syntax.has_template(), "Should detect template");
            assert!(syntax.has_vapor_template(), "Should detect vapor template");
        });
    }

    /// @ai-generated — Syntax should track template block positions.
    #[test]
    fn test_syntax_tracks_template_block_position() {
        let input = r#"<script setup>const x = 1;</script>
<template><div/></template>"#;
        with_syntax!(input, false, |syntax, _events| {
            let tb = syntax.template_block();
            assert!(tb.is_some(), "Should have template block positions");
            let (start, end) = tb.unwrap();
            let block = &input[start as usize..end as usize];
            assert!(
                block.starts_with("<template>"),
                "Template block should start with <template>, got: {}",
                block
            );
            assert!(
                block.ends_with("</template>"),
                "Template block should end with </template>, got: {}",
                block
            );
        });
    }

    /// @ai-generated — Syntax should track script block end position.
    #[test]
    fn test_syntax_tracks_script_block_end() {
        let input = r#"<script setup>const x = 1;</script><template><div/></template>"#;
        with_syntax!(input, false, |syntax, _events| {
            let se = syntax.script_block_end();
            assert!(se.is_some(), "Should have script block end");
            let end = se.unwrap() as usize;
            assert!(
                input[..end].ends_with("</script>"),
                "Script block end should be after </script>, got: ...{}",
                &input[end.saturating_sub(15)..end]
            );
        });
    }
}
