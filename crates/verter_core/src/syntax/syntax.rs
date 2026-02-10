use crate::{
    common::Span,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::*,
    },
    tokenizer::Event,
};

pub struct Syntax<'a, 'p> {
    /// Current parent element ID (NO_PARENT at root level)
    last_parent_id: u32,
    // element_stack: Vec<SyntaxOpenTagIntermediary>,
    nested_level: usize,

    last_event_open_tag: Option<SyntaxOpenTagStart>,

    current_prop: Option<PropTempState>,

    /// Stack to track parent IDs for proper restoration on close tags.
    /// Pre-allocated with capacity 32 to avoid heap allocations for typical nesting depths.
    parent_stack: Vec<u32>,

    pipeline: Vec<&'p mut dyn SyntaxPlugin<'a>>,
}

impl<'a, 'p> Syntax<'a, 'p> {
    pub fn new(pipeline: Vec<&'p mut dyn SyntaxPlugin<'a>>) -> Self {
        Self {
            // element_stack: Vec::with_capacity((script_len / 80).max(10)),
            nested_level: 0,
            current_prop: None,
            last_parent_id: NO_PARENT,
            last_event_open_tag: None,
            parent_stack: Vec::with_capacity(32),
            pipeline,
        }
    }

    fn emit(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        let mut current_event = event;
        for plugin in self.pipeline.iter_mut() {
            match plugin.process_event(current_event, ctx) {
                SyntaxResult::Keep(e) => {
                    current_event = e;
                }
                SyntaxResult::Replace(new_event) => {
                    current_event = new_event;
                }
                SyntaxResult::Drop => {
                    return SyntaxResult::Drop;
                }
            }
        }
        SyntaxResult::Keep(current_event)
    }

    pub fn start(&mut self, ctx: &mut SyntaxPluginContext<'a>) {
        for plugin in self.pipeline.iter_mut() {
            plugin.start(ctx);
        }
    }
    pub fn end(&mut self, ctx: &mut SyntaxPluginContext<'a>) {
        for plugin in self.pipeline.iter_mut() {
            plugin.end(ctx);
        }
    }

    #[inline]
    pub fn resolve_tag_type(
        name: &[u8],
        nested_level: usize,
        ctx: &SyntaxPluginContext<'a>,
    ) -> SyntaxTagType {
        if nested_level == 0 {
            if name == b"template" {
                SyntaxTagType::RootTemplate
            } else if name == b"script" {
                SyntaxTagType::RootScript
            } else if name == b"style" {
                SyntaxTagType::RootStyle
            } else {
                SyntaxTagType::RootUnknown
            }
        } else if (ctx.options.is_custom_element)(name) {
            SyntaxTagType::CustomElement
        } else if name[0] >= b'A' && name[0] <= b'Z' {
            SyntaxTagType::Component
        } else if name == b"slot" {
            SyntaxTagType::Slot
        } else if name == b"template" {
            SyntaxTagType::Template
        } else if name == b"component" {
            // <component :is="..."> dynamic component
            SyntaxTagType::DynamicComponent
        } else if name.contains(&b'-') {
            // Hyphenated tags (e.g. <v-alert>, <my-component>) are always components.
            // Native HTML elements never contain hyphens; custom elements with hyphens
            // are caught earlier by the is_custom_element check.
            SyntaxTagType::Component
        } else {
            SyntaxTagType::Element
        }
    }

    pub fn handle(
        &mut self,
        event: &Event<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match event {
            Event::OpenTagName { start, end } => {
                let name = &ctx.bytes[*start as usize + 1..*end as usize];
                let is_void_element = (ctx.options.is_void_tag)(name);

                // TODO maybe handle this better if the mode is just template
                let tag_type = Syntax::resolve_tag_type(name, self.nested_level, ctx);

                // parent_id is NO_PARENT for root elements (nested_level == 0)
                let parent_id = if self.nested_level == 0 {
                    NO_PARENT
                } else {
                    self.last_parent_id
                };

                let ev = SyntaxOpenTagStart {
                    element_id: *start,
                    start: *start,
                    name_end: *end,

                    tag_type,
                    nested_level: self.nested_level,
                    parent_id,

                    is_void_element,
                };

                self.last_event_open_tag = Some(ev.clone());
                if !is_void_element {
                    self.nested_level += 1;
                    // Push current parent before changing to new element
                    self.parent_stack.push(self.last_parent_id);
                    self.last_parent_id = ev.get_id();
                }
                // Void elements don't update last_parent_id because they can't have children.
                // Updating it would corrupt parent tracking for subsequent siblings.

                self.emit(SyntaxEvent::OpenTagStart(ev), ctx);
            }
            Event::OpenTagEnd { end } => {
                if let Some(start_ev) = self.last_event_open_tag.take() {
                    // Use is_void_element to set self_closing for void tags like <br>, <img>, etc.
                    // Without explicit />, the tokenizer emits OpenTagEnd (not SelfClosingTag),
                    // but void elements should still be treated as self-closing.
                    let is_void = start_ev.is_void_element;
                    let ev = start_ev.to_end(*end, is_void);
                    // Only update last_parent_id for non-void elements.
                    // Void elements can't have children, so they shouldn't become the
                    // "current parent" — doing so corrupts parent tracking for siblings.
                    if !is_void {
                        self.last_parent_id = ev.get_id();
                    }

                    self.emit(SyntaxEvent::OpenTagEnd(ev), ctx);
                }
            }
            Event::SelfClosingTag { end } => {
                if let Some(start_ev) = self.last_event_open_tag.take() {
                    // NOTE: For self-closing tags, tokenizer already emits end pointing PAST '>'.
                    // No need to add +1 (unlike OpenTagEnd).
                    let ev = start_ev.to_end(*end, true);

                    // For self-closing non-void elements, we incremented nested_level at OpenTagStart.
                    // We need to decrement it now since there won't be a matching CloseTag.
                    if !ev.is_void_element && self.nested_level > 0 {
                        self.nested_level -= 1;
                        self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
                    } else {
                        self.last_parent_id = ev.parent_id;
                    }

                    self.emit(SyntaxEvent::OpenTagEnd(ev), ctx);
                }
            }
            Event::CloseTag {
                start,
                end,
                name_end,
            } => {
                if self.nested_level == 0 {
                    // no open tag to match
                    let ev = SyntaxEvent::Error(SyntaxError {
                        start: *start,
                        end: *end,
                        message: SyntaxErrorMessages::OpenTagNotFound,
                    });

                    return self.emit(ev, ctx);
                }

                let close_tag_name: &[u8] = &ctx.bytes[*start as usize + 2..*name_end as usize];

                // Use nested_level - 1 because we're inside the element being closed,
                // but we want the tag_type at the level where the element was declared
                let tag_type = Syntax::resolve_tag_type(close_tag_name, self.nested_level - 1, ctx);

                let is_void_tag = (ctx.options.is_void_tag)(close_tag_name);

                // Capture element_id before pop (this is the element being closed)
                let element_id = self.last_parent_id;

                // Restore parent_id from stack (defaults to NO_PARENT if stack is empty)
                self.last_parent_id = self.parent_stack.pop().unwrap_or(NO_PARENT);
                self.nested_level -= 1;

                self.emit(
                    SyntaxEvent::CloseTag(SyntaxCloseTag {
                        element_id,
                        parent_id: self.last_parent_id,

                        tag_type,

                        start: *start,
                        name_end: *name_end,
                        end: *end,
                        nested_level: self.nested_level,
                        is_void_element: is_void_tag,
                    }),
                    ctx,
                );
            }

            Event::AttribName { start, end } => {
                self.current_prop = Some(PropTempState {
                    start: *start,
                    name_end: *end,
                    is_directive: false,
                    arg_start: None,
                    arg_end: None,
                    is_dynamic: None,
                    value_start: None,
                    modifiers: None,
                });
            }
            Event::DirName { start, end } => {
                self.current_prop = Some(PropTempState {
                    start: *start,
                    name_end: *end,
                    is_directive: true,
                    arg_start: None,
                    arg_end: None,
                    is_dynamic: None,
                    value_start: None,
                    modifiers: None,
                });
            }
            // Directive argument (e.g., :arg in v-bind:arg)
            Event::DirArg {
                start,
                end,
                is_dynamic,
            } => {
                if let Some(ref mut state) = self.current_prop {
                    state.arg_start = Some(*start);
                    state.arg_end = Some(*end);
                    state.is_dynamic = Some(*is_dynamic);
                }
            }
            // Directive modifier (e.g., .prevent in @click.prevent)
            Event::DirModifier { start, end } => {
                if let Some(ref mut state) = self.current_prop {
                    let span = Span {
                        start: *start,
                        end: *end,
                    };
                    match &mut state.modifiers {
                        Some(mods) => mods.push(span),
                        None => {
                            let vec = vec![span];
                            state.modifiers = Some(vec);
                        }
                    };
                }
            }
            // Attribute/directive value data
            Event::AttribData { start, .. } => {
                if let Some(ref mut state) = self.current_prop {
                    // Only set value_start if not already set (first data chunk)
                    if state.value_start.is_none() {
                        state.value_start = Some(*start);
                    }
                }
                // RunnerResult::Drop
            }
            // Attribute/directive end - emit the aggregated event
            Event::AttribEnd { quote, end } => {
                let Some(state) = self.current_prop.take() else {
                    // TODO emit error?
                    return SyntaxResult::Drop;
                };

                let value = match state.value_start {
                    Some(v_start) => {
                        // NOTE: Only apply -1 adjustment for quoted values (Single, Double).
                        // Unquoted values should use the full range from tokenizer.
                        use crate::tokenizer::QuoteType;
                        let value_end = match quote {
                            QuoteType::Single | QuoteType::Double => {
                                // For quoted values, end points after the closing quote,
                                // so subtract 1 to exclude the quote
                                if *end > 0 {
                                    *end - 1
                                } else {
                                    state.name_end
                                }
                            }
                            QuoteType::Unquoted => {
                                // For unquoted values, use the full range
                                *end
                            }
                            QuoteType::NoValue => {
                                // No value case
                                state.name_end
                            }
                        };
                        Some(SyntaxPropValue {
                            start: v_start,
                            end: value_end,
                        })
                    }
                    None => None,
                };
                let arg = match state.arg_start {
                    Some(a_start) => Some(SyntaxPropArg {
                        start: a_start,
                        end: state.arg_end.unwrap(),
                        is_dynamic: state.is_dynamic.unwrap(),
                    }),
                    None => None,
                };
                let ev = SyntaxProp {
                    element_id: self.last_parent_id,
                    parent_id: self.parent_stack.last().copied().unwrap_or(NO_PARENT),
                    start: state.start,
                    end: *end,
                    name_end: state.name_end,
                    is_directive: state.is_directive,
                    value,
                    arg,
                    modifiers: state.modifiers,
                    quote: Some(*quote),
                };

                self.emit(SyntaxEvent::Prop(ev), ctx);
            }

            Event::Text { start, end } => {
                let ev = SyntaxText {
                    parent_id: self.last_parent_id,
                    start: *start,
                    end: *end,
                };
                self.emit(SyntaxEvent::Text(ev), ctx);
            }
            Event::Comment { start, end, .. } => {
                let ev = SyntaxComment {
                    parent_id: self.last_parent_id,
                    start: *start,
                    end: *end,
                };
                self.emit(SyntaxEvent::Comment(ev), ctx);
            }
            Event::Interpolation {
                start,
                end,
                delimiter_open_len,
                delimiter_close_len,
            } => {
                let ev = SyntaxInterpolation {
                    parent_id: self.last_parent_id,
                    start: *start,
                    end: *end,
                    content_start: *start + *delimiter_open_len as u32,
                    content_end: *end - *delimiter_close_len as u32,
                };
                self.emit(SyntaxEvent::Interpolation(ev), ctx);
            }

            _ => {}
        }

        SyntaxResult::Drop
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

pub enum SyntaxOpenTagIntermediary {
    Start(SyntaxOpenTagStart),
    End(SyntaxOpenTagEnd),
}

impl SyntaxOpenTagStart {
    #[inline(always)]
    pub fn to_end(self, end: u32, self_closing: bool) -> SyntaxOpenTagEnd {
        SyntaxOpenTagEnd {
            element_id: self.element_id,
            start: self.start,
            end,
            name_end: self.name_end,
            nested_level: self.nested_level,
            parent_id: self.parent_id,
            self_closing,
            tag_type: self.tag_type,
            is_void_element: self.is_void_element,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::plugin::{
        SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult,
    };
    use crate::tokenizer::byte::tokenize;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Test plugin that collects all emitted SyntaxEvents
    struct CollectorPlugin<'a> {
        events: Rc<RefCell<Vec<SyntaxEvent<'a>>>>,
    }

    impl<'a> CollectorPlugin<'a> {
        fn new(events: Rc<RefCell<Vec<SyntaxEvent<'a>>>>) -> Self {
            Self { events }
        }
    }

    impl<'a> SyntaxPlugin<'a> for CollectorPlugin<'a> {
        fn name(&self) -> &str {
            "collector"
        }

        fn process_event(
            &mut self,
            event: SyntaxEvent<'a>,
            _ctx: &mut SyntaxPluginContext<'a>,
        ) -> SyntaxResult<SyntaxEvent<'a>> {
            // Clone and collect the event, then pass through
            match &event {
                SyntaxEvent::Prop(p) => {
                    self.events.borrow_mut().push(SyntaxEvent::Prop(p.clone()));
                }
                SyntaxEvent::Text(t) => {
                    self.events.borrow_mut().push(SyntaxEvent::Text(t.clone()));
                }
                SyntaxEvent::Interpolation(i) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Interpolation(i.clone()));
                }
                SyntaxEvent::Comment(c) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Comment(c.clone()));
                }
                SyntaxEvent::OpenTagStart(o) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::OpenTagStart(o.clone()));
                }
                SyntaxEvent::OpenTagEnd(o) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::OpenTagEnd(o.clone()));
                }
                SyntaxEvent::CloseTag(c) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::CloseTag(c.clone()));
                }
                SyntaxEvent::Error(e) => {
                    panic!("Unexpected error event: {:?}", e);
                }
                SyntaxEvent::Warning(w) => {
                    panic!("Unexpected warning event: {:?}", w);
                }
                _ => {
                    // For OxcProp, OxcScriptContent, OxcInterpolation - skip them in tests
                }
            }
            SyntaxResult::Keep(event)
        }
    }

    /// Helper: tokenize input and collect all events into a Vec.
    fn tokenize_to_events(input: &str) -> Vec<Event<'static>> {
        let mut events = Vec::new();
        tokenize(input.as_bytes(), |event| events.push(event));
        events
    }

    /// Helper: process tokenizer events through Syntax and collect all SyntaxEvents.
    fn process_through_syntax<F>(input: &str, mut callback: F)
    where
        F: FnMut(&[SyntaxEvent<'_>]),
    {
        let events = tokenize_to_events(input);
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);

        // Create collector to capture events
        let collected_events: Rc<RefCell<Vec<SyntaxEvent<'_>>>> = Rc::new(RefCell::new(Vec::new()));
        let mut collector = CollectorPlugin::new(collected_events.clone());

        let mut syntax = Syntax::new(vec![&mut collector]);
        syntax.start(&mut ctx);

        for event in &events {
            syntax.handle(event, &mut ctx);
        }

        syntax.end(&mut ctx);

        // Pass collected events to callback
        let events_vec = collected_events.borrow();
        callback(&events_vec);
    }

    // ==================== Tag offset tests ====================

    #[test]
    fn test_open_tag_offsets() {
        process_through_syntax("<div></div>", |events| {
            let open_starts: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagStart(ev) => Some((ev.start, ev.name_end)),
                    _ => None,
                })
                .collect();

            assert_eq!(open_starts.len(), 1, "Expected 1 OpenTagStart event");
            let (start, name_end) = open_starts[0];

            // OpenTagStart includes the '<', so full tag is <div
            assert_eq!(
                &"<div></div>"[start as usize..name_end as usize],
                "<div",
                "OpenTagStart offsets should match '<div'"
            );
        });
    }

    #[test]
    fn test_open_tag_end_offsets() {
        process_through_syntax("<div></div>", |events| {
            let open_ends: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagEnd(ev) => Some((ev.start, ev.end)),
                    _ => None,
                })
                .collect();

            assert_eq!(open_ends.len(), 1, "Expected 1 OpenTagEnd event");
            let (start, end) = open_ends[0];

            // Full opening tag from < to >
            assert_eq!(
                &"<div></div>"[start as usize..end as usize],
                "<div>",
                "OpenTagEnd offsets should match '<div>'"
            );
        });
    }

    // ==================== Attribute offset tests ====================

    #[test]
    fn test_attribute_name_offsets() {
        process_through_syntax(r#"<div class="hello"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if !prop.is_directive => {
                        Some((prop.start, prop.name_end, prop.end))
                    }
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 Prop event");
            let (start, name_end, _) = props[0];

            let input = r#"<div class="hello"></div>"#;
            assert_eq!(
                &input[start as usize..name_end as usize],
                "class",
                "Attribute name offsets should match 'class'"
            );
        });
    }

    #[test]
    fn test_attribute_value_offsets() {
        process_through_syntax(r#"<div class="hello"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if !prop.is_directive => prop.value.as_ref(),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 Prop with value");
            let value = props[0];

            let input = r#"<div class="hello"></div>"#;
            assert_eq!(
                &input[value.start as usize..value.end as usize],
                "hello",
                "Attribute value offsets should match 'hello'"
            );
        });
    }

    #[test]
    fn test_attribute_full_span_offsets() {
        process_through_syntax(r#"<div class="hello"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if !prop.is_directive => Some((prop.start, prop.end)),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 Prop event");
            let (start, end) = props[0];

            let input = r#"<div class="hello"></div>"#;
            assert_eq!(
                &input[start as usize..end as usize],
                r#"class="hello""#,
                "Full attribute offsets should match 'class=\"hello\"'"
            );
        });
    }

    #[test]
    fn test_attribute_single_quote() {
        process_through_syntax("<div class='bar'></div>", |events| {
            let values: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if !prop.is_directive => prop.value.as_ref(),
                    _ => None,
                })
                .collect();

            assert_eq!(values.len(), 1);
            let value = values[0];

            let input = "<div class='bar'></div>";
            assert_eq!(
                &input[value.start as usize..value.end as usize],
                "bar",
                "Single-quoted value offsets should match 'bar'"
            );
        });
    }

    #[test]
    fn test_attribute_unquoted() {
        process_through_syntax("<div id=test></div>", |events| {
            let values: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if !prop.is_directive => prop.value.as_ref(),
                    _ => None,
                })
                .collect();

            assert_eq!(values.len(), 1);
            let value = values[0];

            let input = "<div id=test></div>";
            assert_eq!(
                &input[value.start as usize..value.end as usize],
                "test",
                "Unquoted value offsets should match 'test'"
            );
        });
    }

    #[test]
    fn test_multiple_attributes() {
        process_through_syntax(r#"<div id="a" class="b"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if !prop.is_directive => Some((prop.start, prop.end)),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 2, "Expected 2 attributes");

            let input = r#"<div id="a" class="b"></div>"#;
            assert_eq!(
                &input[props[0].0 as usize..props[0].1 as usize],
                r#"id="a""#,
                "First attribute should be 'id=\"a\"'"
            );
            assert_eq!(
                &input[props[1].0 as usize..props[1].1 as usize],
                r#"class="b""#,
                "Second attribute should be 'class=\"b\"'"
            );
        });
    }

    // ==================== Directive offset tests ====================

    #[test]
    fn test_directive_name_offsets() {
        process_through_syntax(r#"<div v-if="show"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if prop.is_directive => {
                        Some((prop.start, prop.name_end))
                    }
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1, "Expected 1 directive");
            let (start, name_end) = props[0];

            let input = r#"<div v-if="show"></div>"#;
            assert_eq!(
                &input[start as usize..name_end as usize],
                "v-if",
                "Directive name should be 'v-if'"
            );
        });
    }

    #[test]
    fn test_directive_value_offsets() {
        process_through_syntax(r#"<div v-if="show"></div>"#, |events| {
            let values: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if prop.is_directive => prop.value.as_ref(),
                    _ => None,
                })
                .collect();

            assert_eq!(values.len(), 1);
            let value = values[0];

            let input = r#"<div v-if="show"></div>"#;
            assert_eq!(
                &input[value.start as usize..value.end as usize],
                "show",
                "Directive value should be 'show'"
            );
        });
    }

    #[test]
    fn test_directive_with_arg_offsets() {
        process_through_syntax(r#"<div v-bind:class="active"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if prop.is_directive => Some(prop),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1);
            let prop = props[0];

            let input = r#"<div v-bind:class="active"></div>"#;

            // Check directive name
            assert_eq!(
                &input[prop.start as usize..prop.name_end as usize],
                "v-bind",
                "Directive name should be 'v-bind'"
            );

            // Check argument
            if let Some(arg) = &prop.arg {
                assert_eq!(
                    &input[arg.start as usize..arg.end as usize],
                    "class",
                    "Directive argument should be 'class'"
                );
            } else {
                panic!("Expected directive to have an argument");
            }

            // Check value
            if let Some(value) = &prop.value {
                assert_eq!(
                    &input[value.start as usize..value.end as usize],
                    "active",
                    "Directive value should be 'active'"
                );
            } else {
                panic!("Expected directive to have a value");
            }
        });
    }

    #[test]
    fn test_directive_shorthand_offsets() {
        process_through_syntax(r#"<div :class="active"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if prop.is_directive => Some(prop),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1);
            let prop = props[0];

            let input = r#"<div :class="active"></div>"#;

            // Full directive span should include the : and argument
            assert_eq!(
                &input[prop.start as usize..prop.end as usize],
                r#":class="active""#,
                "Full directive should be ':class=\"active\"'"
            );
        });
    }

    #[test]
    fn test_directive_with_modifiers_offsets() {
        process_through_syntax(
            r#"<button @click.prevent.stop="handler"></button>"#,
            |events| {
                let props: Vec<_> = events
                    .iter()
                    .filter_map(|e| match e {
                        SyntaxEvent::Prop(prop) if prop.is_directive => Some(prop),
                        _ => None,
                    })
                    .collect();

                assert_eq!(props.len(), 1);
                let prop = props[0];

                let input = r#"<button @click.prevent.stop="handler"></button>"#;

                // Check modifiers
                if let Some(modifiers) = &prop.modifiers {
                    assert_eq!(modifiers.len(), 2, "Expected 2 modifiers");

                    assert_eq!(
                        &input[modifiers[0].start as usize..modifiers[0].end as usize],
                        "prevent",
                        "First modifier should be 'prevent'"
                    );
                    assert_eq!(
                        &input[modifiers[1].start as usize..modifiers[1].end as usize],
                        "stop",
                        "Second modifier should be 'stop'"
                    );
                } else {
                    panic!("Expected directive to have modifiers");
                }
            },
        );
    }

    #[test]
    fn test_directive_dynamic_arg_offsets() {
        process_through_syntax(r#"<div v-bind:[key]="value"></div>"#, |events| {
            let props: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Prop(prop) if prop.is_directive => Some(prop),
                    _ => None,
                })
                .collect();

            assert_eq!(props.len(), 1);
            let prop = props[0];

            let input = r#"<div v-bind:[key]="value"></div>"#;

            if let Some(arg) = &prop.arg {
                assert!(arg.is_dynamic, "Argument should be marked as dynamic");
                assert_eq!(
                    &input[arg.start as usize..arg.end as usize],
                    "[key]",
                    "Dynamic argument should include brackets '[key]'"
                );
            } else {
                panic!("Expected directive to have an argument");
            }
        });
    }

    // ==================== Text node offset tests ====================

    #[test]
    fn test_text_node_offsets() {
        process_through_syntax("<div>hello world</div>", |events| {
            let texts: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Text(text) => Some((text.start, text.end)),
                    _ => None,
                })
                .collect();

            assert_eq!(texts.len(), 1, "Expected 1 text node");
            let (start, end) = texts[0];

            let input = "<div>hello world</div>";
            assert_eq!(
                &input[start as usize..end as usize],
                "hello world",
                "Text node offsets should match 'hello world'"
            );
        });
    }

    // ==================== Interpolation offset tests ====================

    #[test]
    fn test_interpolation_offsets() {
        process_through_syntax("<div>{{ message }}</div>", |events| {
            let interpolations: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Interpolation(interp) => Some((
                        interp.start,
                        interp.end,
                        interp.content_start,
                        interp.content_end,
                    )),
                    _ => None,
                })
                .collect();

            assert_eq!(interpolations.len(), 1, "Expected 1 interpolation");
            let (start, end, content_start, content_end) = interpolations[0];

            let input = "<div>{{ message }}</div>";
            assert_eq!(
                &input[start as usize..end as usize],
                "{{ message }}",
                "Full interpolation should be '{{ message }}'"
            );
            assert_eq!(
                &input[content_start as usize..content_end as usize],
                " message ",
                "Interpolation content should be ' message ' (with spaces)"
            );
        });
    }

    // ==================== Comment offset tests ====================

    #[test]
    fn test_comment_offsets() {
        process_through_syntax("<!-- comment text -->", |events| {
            let comments: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::Comment(comment) => Some((comment.start, comment.end)),
                    _ => None,
                })
                .collect();

            assert_eq!(comments.len(), 1, "Expected 1 comment");
            let (start, end) = comments[0];

            let input = "<!-- comment text -->";
            assert_eq!(
                &input[start as usize..end as usize],
                "<!-- comment text -->",
                "Comment offsets should match full comment"
            );
        });
    }

    // ==================== Self-closing tag offset tests ====================

    #[test]
    fn test_self_closing_tag_offsets() {
        process_through_syntax(r#"<input type="text" />"#, |events| {
            let open_ends: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagEnd(ev) => Some((ev.start, ev.end, ev.self_closing)),
                    _ => None,
                })
                .collect();

            assert_eq!(open_ends.len(), 1);
            let (start, end, self_closing) = open_ends[0];

            assert!(self_closing, "Tag should be marked as self-closing");

            let input = r#"<input type="text" />"#;
            assert_eq!(
                &input[start as usize..end as usize],
                r#"<input type="text" />"#,
                "Self-closing tag full span should match entire tag"
            );
        });
    }

    // ==================== Nested elements offset tests ====================

    #[test]
    fn test_nested_elements_offsets() {
        process_through_syntax("<div><span>text</span></div>", |events| {
            let open_starts: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagStart(ev) => Some((ev.start, ev.name_end)),
                    _ => None,
                })
                .collect();

            assert_eq!(open_starts.len(), 2, "Expected 2 OpenTagStart events");

            let input = "<div><span>text</span></div>";

            // First should be <div
            assert_eq!(
                &input[open_starts[0].0 as usize..open_starts[0].1 as usize],
                "<div",
                "First tag should be '<div'"
            );

            // Second should be <span
            assert_eq!(
                &input[open_starts[1].0 as usize..open_starts[1].1 as usize],
                "<span",
                "Second tag should be '<span'"
            );
        });
    }

    // ==================== Parent ID tests ====================

    #[test]
    fn test_close_tag_parent_id() {
        // Test that CloseTag events have the correct parent_id
        // <div><span></span></div>
        // - div's parent is root (NO_PARENT)
        // - span's parent is div (0)
        // When span closes, parent_id should be div's start (0)
        // When div closes, parent_id should be root (NO_PARENT)
        process_through_syntax("<div><span></span></div>", |events| {
            let close_tags: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::CloseTag(c) => Some((c.start, c.parent_id)),
                    _ => None,
                })
                .collect();

            assert_eq!(close_tags.len(), 2, "Expected 2 CloseTag events");

            // First close is </span> at position 11, parent is <div> at position 0
            assert_eq!(
                close_tags[0].0, 11,
                "First CloseTag should be at position 11"
            );
            assert_eq!(
                close_tags[0].1, 0,
                "span's parent should be div at position 0"
            );

            // Second close is </div> at position 18, parent is root (NO_PARENT)
            assert_eq!(
                close_tags[1].0, 18,
                "Second CloseTag should be at position 18"
            );
            assert_eq!(
                close_tags[1].1, NO_PARENT,
                "div's parent should be root (NO_PARENT)"
            );
        });
    }

    #[test]
    fn test_close_tag_ids_deeply_nested() {
        // Test deeply nested structure: <a><b><c></c></b></a>
        // Positions: a=0, b=3, c=6
        // When c closes: element_id=6 (c's start), parent_id=3 (b's start)
        // When b closes: element_id=3 (b's start), parent_id=0 (a's start)
        // When a closes: element_id=0 (a's start), parent_id=NO_PARENT (root)
        process_through_syntax("<a><b><c></c></b></a>", |events| {
            let open_tags: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagStart(o) => Some(o.start),
                    _ => None,
                })
                .collect();

            let close_tags: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::CloseTag(c) => Some((c.element_id, c.parent_id)),
                    _ => None,
                })
                .collect();

            assert_eq!(open_tags.len(), 3, "Expected 3 OpenTagStart events");
            assert_eq!(close_tags.len(), 3, "Expected 3 CloseTag events");

            let a_start = open_tags[0]; // 0
            let b_start = open_tags[1]; // 3
            let c_start = open_tags[2]; // 6

            // c closes first: element_id=c, parent_id=b
            assert_eq!(
                close_tags[0].0, c_start,
                "c's element_id should be c's start"
            );
            assert_eq!(close_tags[0].1, b_start, "c's parent_id should be b");

            // b closes second: element_id=b, parent_id=a
            assert_eq!(
                close_tags[1].0, b_start,
                "b's element_id should be b's start"
            );
            assert_eq!(close_tags[1].1, a_start, "b's parent_id should be a");

            // a closes last: element_id=a, parent_id=NO_PARENT (root)
            assert_eq!(
                close_tags[2].0, a_start,
                "a's element_id should be a's start"
            );
            assert_eq!(
                close_tags[2].1, NO_PARENT,
                "a's parent_id should be root (NO_PARENT)"
            );
        });
    }

    // ==================== Element content offset tests ====================

    // TODO: Re-enable when ElementContentEnd variant is restored
    // #[test]
    // fn test_element_content_offsets() {
    //     process_through_syntax("<div>content</div>", |events| {
    //         let content_ends: Vec<_> = events
    //             .iter()
    //             .filter_map(|e| match e {
    //                 SyntaxEvent::ElementContentEnd(content) => Some((content.start, content.end)),
    //                 _ => None,
    //             })
    //             .collect();
    //
    //         assert_eq!(content_ends.len(), 1, "Expected 1 ElementContentEnd");
    //         let (start, end) = content_ends[0];
    //
    //         let input = "<div>content</div>";
    //         assert_eq!(
    //             &input[start as usize..end as usize],
    //             "content",
    //             "Element content should be 'content'"
    //         );
    //     });
    // }

    // ==================== Complex scenario tests ====================

    #[test]
    fn test_complex_template_offsets() {
        let input = r#"<div class="container" v-if="show"><span @click="handler">{{ message }}</span></div>"#;
        process_through_syntax(input, |events| {
            // Verify we have the expected number of events
            let prop_count = events
                .iter()
                .filter(|e| matches!(e, SyntaxEvent::Prop(_)))
                .count();
            assert_eq!(prop_count, 3, "Expected 3 props (class, v-if, @click)");

            // Verify each prop's offsets match the source
            for event in events {
                if let SyntaxEvent::Prop(prop) = event {
                    let full_prop = &input[prop.start as usize..prop.end as usize];

                    // Verify the name is extractable
                    let name = &input[prop.start as usize..prop.name_end as usize];
                    assert!(
                        !name.is_empty(),
                        "Prop name should not be empty for: {}",
                        full_prop
                    );

                    // If there's a value, verify it's extractable
                    if let Some(value) = &prop.value {
                        let value_str = &input[value.start as usize..value.end as usize];
                        assert!(
                            !value_str.is_empty(),
                            "Prop value should not be empty for: {}",
                            full_prop
                        );
                    }
                }
            }
        });
    }

    // ==================== Tag Type Tests ====================

    #[test]
    fn test_resolve_tag_type_root_level() {
        let input = "<script></script>";
        let options = SyntaxPluginOptions::default();
        let ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);

        // At root level (nested_level = 0), script should be RootScript
        assert_eq!(
            Syntax::resolve_tag_type(b"script", 0, &ctx),
            SyntaxTagType::RootScript
        );
        assert_eq!(
            Syntax::resolve_tag_type(b"template", 0, &ctx),
            SyntaxTagType::RootTemplate
        );
        assert_eq!(
            Syntax::resolve_tag_type(b"style", 0, &ctx),
            SyntaxTagType::RootStyle
        );
        assert_eq!(
            Syntax::resolve_tag_type(b"unknown", 0, &ctx),
            SyntaxTagType::RootUnknown
        );
    }

    #[test]
    fn test_resolve_tag_type_nested_level() {
        let input = "<div></div>";
        let options = SyntaxPluginOptions::default();
        let ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);

        // At nested level > 0, regular elements should be Element
        assert_eq!(
            Syntax::resolve_tag_type(b"div", 1, &ctx),
            SyntaxTagType::Element
        );
        assert_eq!(
            Syntax::resolve_tag_type(b"span", 1, &ctx),
            SyntaxTagType::Element
        );

        // Components (start with uppercase) should be Component
        assert_eq!(
            Syntax::resolve_tag_type(b"MyComponent", 1, &ctx),
            SyntaxTagType::Component
        );
        assert_eq!(
            Syntax::resolve_tag_type(b"Button", 1, &ctx),
            SyntaxTagType::Component
        );

        // slot should be Slot
        assert_eq!(
            Syntax::resolve_tag_type(b"slot", 1, &ctx),
            SyntaxTagType::Slot
        );

        // template (nested) should be Template
        assert_eq!(
            Syntax::resolve_tag_type(b"template", 1, &ctx),
            SyntaxTagType::Template
        );
    }

    #[test]
    fn test_close_tag_has_tag_type() {
        // Test that CloseTag events include the correct tag_type
        // Wrap in <template> so nested elements get proper types
        process_through_syntax("<template><div><slot></slot></div></template>", |events| {
            let close_tags: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::CloseTag(c) => Some(c.tag_type.clone()),
                    _ => None,
                })
                .collect();

            assert_eq!(close_tags.len(), 3, "Expected 3 CloseTag events");

            // First close is </slot> - should be Slot type
            assert_eq!(close_tags[0], SyntaxTagType::Slot);

            // Second close is </div> - should be Element type
            assert_eq!(close_tags[1], SyntaxTagType::Element);

            // Third close is </template> - should be RootTemplate type
            assert_eq!(close_tags[2], SyntaxTagType::RootTemplate);
        });
    }

    #[test]
    fn test_open_and_close_tag_type_consistency() {
        // Test that OpenTagStart and CloseTag have matching tag_types
        // Use <template> at root level so nested elements get proper types
        process_through_syntax(
            "<template><div><MyComponent></MyComponent></div></template>",
            |events| {
                let open_types: Vec<_> = events
                    .iter()
                    .filter_map(|e| match e {
                        SyntaxEvent::OpenTagStart(o) => Some(o.tag_type.clone()),
                        _ => None,
                    })
                    .collect();

                let close_types: Vec<_> = events
                    .iter()
                    .filter_map(|e| match e {
                        SyntaxEvent::CloseTag(c) => Some(c.tag_type.clone()),
                        _ => None,
                    })
                    .collect();

                assert_eq!(open_types.len(), 3, "Expected 3 OpenTagStart events");
                assert_eq!(close_types.len(), 3, "Expected 3 CloseTag events");

                // template opens first, div opens second, MyComponent opens third
                assert_eq!(
                    open_types[0],
                    SyntaxTagType::RootTemplate,
                    "template should be RootTemplate"
                );
                assert_eq!(
                    open_types[1],
                    SyntaxTagType::Element,
                    "div should be Element"
                );
                assert_eq!(
                    open_types[2],
                    SyntaxTagType::Component,
                    "MyComponent should be Component"
                );

                // MyComponent closes first, div closes second, template closes third
                assert_eq!(
                    close_types[0],
                    SyntaxTagType::Component,
                    "MyComponent close should be Component"
                );
                assert_eq!(
                    close_types[1],
                    SyntaxTagType::Element,
                    "div close should be Element"
                );
                assert_eq!(
                    close_types[2],
                    SyntaxTagType::RootTemplate,
                    "template close should be RootTemplate"
                );
            },
        );
    }

    #[test]
    fn test_root_script_tag_type() {
        // Test root-level script tag type
        process_through_syntax("<script>const x = 1;</script>", |events| {
            let open_types: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagStart(o) => Some(o.tag_type.clone()),
                    _ => None,
                })
                .collect();

            let close_types: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::CloseTag(c) => Some(c.tag_type.clone()),
                    _ => None,
                })
                .collect();

            assert_eq!(open_types.len(), 1);
            assert_eq!(close_types.len(), 1);

            assert_eq!(
                open_types[0],
                SyntaxTagType::RootScript,
                "Root script open should be RootScript"
            );
            assert_eq!(
                close_types[0],
                SyntaxTagType::RootScript,
                "Root script close should be RootScript"
            );
        });
    }

    #[test]
    fn test_root_template_tag_type() {
        // Test root-level template tag type
        process_through_syntax("<template><div></div></template>", |events| {
            let open_types: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagStart(o) => Some(o.tag_type.clone()),
                    _ => None,
                })
                .collect();

            assert_eq!(open_types.len(), 2);

            assert_eq!(
                open_types[0],
                SyntaxTagType::RootTemplate,
                "Root template should be RootTemplate"
            );
            assert_eq!(
                open_types[1],
                SyntaxTagType::Element,
                "Nested div should be Element"
            );
        });
    }

    // ==================== Void tag self_closing tests ====================

    /// @ai-generated - Tests that implicit void tags (no />) get self_closing: true
    #[test]
    fn test_void_tag_br_implicit_self_closing() {
        process_through_syntax("<template><br></template>", |events| {
            let open_ends: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagEnd(ev) => {
                        Some((ev.start, ev.end, ev.self_closing, ev.is_void_element))
                    }
                    _ => None,
                })
                .collect();

            // Two OpenTagEnd events: one for <template>, one for <br>
            assert_eq!(open_ends.len(), 2, "Expected 2 OpenTagEnd events");

            // <template> is not self-closing and not void
            assert!(!open_ends[0].2, "template should not be self_closing");
            assert!(!open_ends[0].3, "template should not be is_void_element");

            // <br> (no />) should still be self_closing because it's a void element
            assert!(open_ends[1].2, "br should be self_closing (void element)");
            assert!(open_ends[1].3, "br should be is_void_element");
        });
    }

    /// @ai-generated - Tests that void tags with attributes get self_closing: true
    #[test]
    fn test_void_tag_img_with_attributes_self_closing() {
        process_through_syntax(r#"<template><img src="test.png"></template>"#, |events| {
            let open_ends: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagEnd(ev) if ev.is_void_element => Some(ev.self_closing),
                    _ => None,
                })
                .collect();

            assert_eq!(open_ends.len(), 1, "Expected 1 void OpenTagEnd event");
            assert!(open_ends[0], "img with attributes should be self_closing");
        });
    }

    /// @ai-generated - Tests that non-void tags do NOT get self_closing: true
    #[test]
    fn test_non_void_tag_not_self_closing() {
        process_through_syntax("<template><div></div></template>", |events| {
            let open_ends: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SyntaxEvent::OpenTagEnd(ev) if ev.tag_type == SyntaxTagType::Element => {
                        Some(ev.self_closing)
                    }
                    _ => None,
                })
                .collect();

            assert_eq!(open_ends.len(), 1, "Expected 1 Element OpenTagEnd event");
            assert!(!open_ends[0], "div should NOT be self_closing");
        });
    }

    // ==================== Custom delimiter interpolation tests ====================

    /// @ai-generated - Tests interpolation with custom 3-byte delimiters
    #[test]
    fn test_interpolation_custom_delimiters() {
        use crate::tokenizer::byte::tokenize_with_delimiters;

        let input = "<div>[[[message]]]</div>";
        let mut events = Vec::new();
        tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"[[[", b"]]]");

        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);

        let collected_events: Rc<RefCell<Vec<SyntaxEvent<'_>>>> = Rc::new(RefCell::new(Vec::new()));
        let mut collector = CollectorPlugin::new(collected_events.clone());

        let mut syntax = Syntax::new(vec![&mut collector]);
        syntax.start(&mut ctx);

        for event in &events {
            syntax.handle(event, &mut ctx);
        }

        syntax.end(&mut ctx);

        let events_vec = collected_events.borrow();
        let interpolations: Vec<_> = events_vec
            .iter()
            .filter_map(|e| match e {
                SyntaxEvent::Interpolation(interp) => Some((
                    interp.start,
                    interp.end,
                    interp.content_start,
                    interp.content_end,
                )),
                _ => None,
            })
            .collect();

        assert_eq!(interpolations.len(), 1, "Expected 1 interpolation");
        let (start, end, content_start, content_end) = interpolations[0];

        assert_eq!(
            &input[start as usize..end as usize],
            "[[[message]]]",
            "Full interpolation should be '[[[message]]]'"
        );
        // content_start = start + 3 (delimiter_open_len), content_end = end - 3 (delimiter_close_len)
        assert_eq!(
            content_start,
            start + 3,
            "content_start should be start + 3 for 3-byte delimiter"
        );
        assert_eq!(
            content_end,
            end - 3,
            "content_end should be end - 3 for 3-byte delimiter"
        );
        assert_eq!(
            &input[content_start as usize..content_end as usize],
            "message",
            "Interpolation content should be 'message'"
        );
    }
}
