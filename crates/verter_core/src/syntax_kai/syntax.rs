use crate::{
    common::Span,
    cursor::ScriptLanguage,
    syntax_kai::{plugin::SyntaxPluginContext, types::*},
    tokenizer::{Event as TokenizerEvent, QuoteType},
    utils::{
        oxc::vue::extract_vfor_positions,
        vue::{
            is_html_tag, is_mathml_tag, is_svg_tag, is_tag_name_component, PatchFlag, PatchFlags,
        },
    },
};

/// Flags that are mutually exclusive with FULL_PROPS.
/// When dynamic keys are detected, these individual flags are cleared
/// because FULL_PROPS implies a full diff that covers them all.
const FULL_PROPS_EXCLUDES: PatchFlag = PatchFlag(
    PatchFlags::Class as i16 | PatchFlags::Style as i16 | PatchFlags::Props as i16,
);

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
                    parent.patch_flag =
                        parent.patch_flag.add_unchecked(PatchFlags::Props);
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
                parent.patch_flag =
                    parent.patch_flag.add_unchecked(PatchFlags::NeedHydration);
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
                        parent.patch_flag =
                            parent.patch_flag.add_unchecked(PatchFlags::Props);
                        if let Some(arg) = prop.arg {
                            parent.dynamic_props.push(arg);
                        }
                    } else {
                        parent.patch_flag =
                            parent.patch_flag.add_unchecked(PatchFlags::Class);
                    }
                }
            }
            PropKind::StyleBind => {
                // On components, :style becomes PROPS. On elements, :style → STYLE.
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    if is_component {
                        parent.patch_flag =
                            parent.patch_flag.add_unchecked(PatchFlags::Props);
                        if let Some(arg) = prop.arg {
                            parent.dynamic_props.push(arg);
                        }
                    } else {
                        parent.patch_flag =
                            parent.patch_flag.add_unchecked(PatchFlags::Style);
                    }
                }
            }
            PropKind::Model => {
                // v-model creates modelValue prop + onUpdate:modelValue event
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    parent.patch_flag =
                        parent.patch_flag.add_unchecked(PatchFlags::Props);
                    // "modelValue" is synthetic — codegen emits the string directly
                }
                parent.patch_flag =
                    parent.patch_flag.add_unchecked(PatchFlags::NeedHydration);
            }
            PropKind::Show | PropKind::Directive => {
                // v-show and custom directives have runtime hooks → NEED_PATCH
                parent.patch_flag =
                    parent.patch_flag.add_unchecked(PatchFlags::NeedPatch);
            }
            PropKind::Html | PropKind::Text => {
                // v-html/v-text create innerHTML/textContent prop bindings → PROPS
                // "innerHTML"/"textContent" are synthetic — codegen emits them directly
                if !parent.patch_flag.contains_unchecked(PatchFlags::FullProps) {
                    parent.patch_flag =
                        parent.patch_flag.add_unchecked(PatchFlags::Props);
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
            | PropKind::Slot => {}
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

    events: &'alloc mut Vec<Event<'alloc>>,
}

impl<'alloc> Syntax<'alloc> {
    pub fn new(events: &'alloc mut Vec<Event<'alloc>>, template_mode: bool) -> Self {
        Self {
            template_mode,
            last_parent_id: NO_PARENT,
            nested_level: 0,
            last_root_node: None,
            last_event_open_tag: None,
            current_prop: None,
            parent_stack: Vec::with_capacity(32),
            events,
            root_script_events: Vec::with_capacity(6),
        }
    }

    pub fn handle(
        &mut self,
        event: &TokenizerEvent<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) {
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
                        // TODO add error event for unmatched tag
                    } else {
                        self.nested_level -= 1;
                        self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
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
        if self.nested_level == 0 {
            // TODO add error event for unmatched tag
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
                // TODO add error event for mismatched tag
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
        }
        // Case 2: Closing a root node — only when we're at depth 1 (directly inside root)
        else if self.nested_level == 1 && !self.template_mode {
            if let Some(root) = self.last_root_node.take() {
                match root {
                    RootNodeOpenTag::End(root) => {
                        let root_name = &ctx.bytes[root.start as usize + 1..root.name_end as usize];
                        if root_name != name {
                            // TODO add error event for mismatched tag
                            self.last_root_node = Some(RootNodeOpenTag::End(root));
                            return;
                        }

                        let ev = RootNodeCloseTag {
                            kind: root.kind,
                            start,
                            name_end,
                            end,
                        };
                        self.events.push(Event::RootCloseTag(ev));

                        self.nested_level -= 1;
                        self.last_parent_id = NO_PARENT;
                    }
                    _ => unreachable!(),
                }
            }
        }
        // Case 3: Normal nested element close (non-empty element with children/text)
        else {
            let kind = Self::resolve_tag_kind(name, ctx);

            self.nested_level -= 1;
            self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);

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
                // TODO add error event for directive argument on non-directive
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
            // maybe this is unreachable...
            // TODO add error event for attribute end without name
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
        if let Some(parent) = &mut self.last_event_open_tag {
            estimate_patch_flag(parent, &ev, ctx.bytes);
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

        // // check for scope directives, v-if/else-if/else on root node, and v-for and emit corresponding events
        // if state.is_directive {
        //     match ev.kind {
        //         PropKind::If => {
        //             self.events.push(Event::ScopeIf(ElementScopeConditionIf {
        //                 element_start: ev.element_id,
        //                 start: ev.start,
        //                 end,
        //                 value,
        //             }));
        //         }
        //         PropKind::ElseIf => {
        //             self.events
        //                 .push(Event::ScopeElseIf(ElementScopeConditionElseIf {
        //                     element_start: ev.element_id,
        //                     start: ev.start,
        //                     end,
        //                     value,
        //                 }));
        //         }
        //         PropKind::Else => {
        //             self.events
        //                 .push(Event::ScopeElse(ElementScopeConditionElse {
        //                     element_start: ev.element_id,
        //                     start: ev.start,
        //                     end,
        //                 }));
        //         }
        //         PropKind::For => {
        //             if let Some(v) = value {
        //                 // let source_bytes = &ctx.bytes[v.start as usize..v.end as usize];
        //                 if let Some((left, in_of_pos, right, is_of)) =
        //                     extract_vfor_positions(ctx.bytes, v.start, v.end)
        //                 {
        //                     self.events.push(Event::ScopeFor(ElementScopeFor {
        //                         element_start: ev.element_id,
        //                         start: ev.start,
        //                         end,
        //                         value,

        //                         is_of,
        //                         iterator: Some(Span::new(left, in_of_pos)),
        //                         iterable: Some(Span::new(right, end)),
        //                     }));
        //                 } else {
        //                     // todo not a valid v-for expression, add error event
        //                 }
        //             } else {
        //                 // todo add error event for v-for without value
        //             }
        //         }
        //         PropKind::Slot => {
        //             // let see if the element is `template`
        //             if let Some(open_tag) = self.last_event_open_tag {
        //                 if open_tag.kind == ElementKind::Template {
        //                     self.events
        //                         .push(Event::ScopeSlotTemplate(ElementScopeSlotTemplate {
        //                             element_start: ev.element_id,
        //                             start: ev.start,
        //                             end,
        //                             name: value,
        //                         }));
        //                 } else {
        //                     self.events
        //                         .push(Event::ScopeSlotElement(ElementScopeSlotElement {
        //                             element_content_start: open_tag.,
        //                             element_start: ev.element_id,
        //                             start: ev.start,
        //                             end,
        //                             name: value,
        //                         }));
        //                 }
        //             }
        //         }
        //         _ => {}
        //     }
        // }

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
            } else if name.starts_with(b"v-") {
                PropKind::Directive
            } else {
                // TODO add error event for unrecognized directive
                // Maybe is unreachable since tokenizer should only emit directive events for valid directives
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
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::types::*;
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
            };

            let mut events_storage: Vec<Event<'_>> = Vec::new();
            let ptr = &mut events_storage as *mut Vec<Event<'_>>;
            {
                // SAFETY: Decouples the mutable borrow lifetime from the Event lifetime.
                // Syntax writes into the vec during handle() calls, then is dropped at
                // scope end. The events borrow from tokenizer_events/input which are alive.
                let mut syntax = Syntax::new(unsafe { &mut *ptr }, $template_mode);
                for event in &tokenizer_events {
                    syntax.handle(event, &mut ctx);
                }
            }

            let $events = &events_storage;
            $body
        }};
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
}
