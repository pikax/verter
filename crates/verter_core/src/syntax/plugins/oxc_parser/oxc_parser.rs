use oxc_parser::ParserReturn;
use oxc_span::SourceType;

use crate::{
    common::Span,
    cursor::ScriptLanguage,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{
            OxcInterpolation, OxcProp, OxcPropProcessed, OxcScriptContent, OxcVConditionType,
            OxcVConditional, OxcVForProp, OxcVSlotProp, SyntaxEvent, SyntaxProp,
        },
    },
    syntax_kai::plugins::oxc_parser::script::parse_script,
    utils::oxc::{
        extract_bindings_from_expression,
        vue::{parse_generic, parse_vfor_with_bindings, parse_vslot_with_bindings, ScriptMode},
        BindingContext,
    },
};

pub struct OxcParserPlugin<'a> {
    source_type: SourceType,

    alloc: &'a oxc_allocator::Allocator,

    script_tag_span: Option<Span>,
    /// Tracks whether the current script tag has the `setup` attribute
    script_setup_span: Option<Span>,
    script_generic_span: Option<Span>,
    script_lang_span: Option<Span>,
    script_attrs_span: Option<Span>,
    /// The element_id of the current script tag (for matching Prop events)
    script_element_id: Option<u32>,
    /// Collected props within the current script element
    script_element_props: Vec<SyntaxProp>,
}

impl<'a> OxcParserPlugin<'a> {
    pub fn new(alloc: &'a oxc_allocator::Allocator, source_type: SourceType) -> Self {
        Self {
            source_type,
            alloc,
            script_tag_span: None,
            script_setup_span: None,
            script_generic_span: None,
            script_lang_span: None,
            script_attrs_span: None,
            script_element_id: None,
            script_element_props: Vec::new(),
        }
    }

    fn parse_expression(
        &self,
        source: &'a str,
    ) -> Result<oxc_ast::ast::Expression<'a>, Vec<oxc_diagnostics::OxcDiagnostic>> {
        let parser = oxc_parser::Parser::new(self.alloc, source, self.source_type);
        parser.parse_expression()
    }
    fn parse_program(&self, source: &'a str) -> ParserReturn<'a> {
        let parser = oxc_parser::Parser::new(self.alloc, source, self.source_type);
        parser.parse()
    }
}

impl<'a> SyntaxPlugin<'a> for OxcParserPlugin<'a> {
    fn name(&self) -> &str {
        "oxc_parser"
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match event {
            SyntaxEvent::OpenTagStart(e) => {
                // Track root-level script tags so we can detect `setup` attribute
                if e.nested_level == 0
                    && ctx.bytes[e.start as usize..e.name_end as usize].starts_with(b"<script")
                {
                    self.script_element_id = Some(e.element_id);
                    self.script_setup_span = None; // Reset for new script tag
                }
                SyntaxResult::Keep(SyntaxEvent::OpenTagStart(e))
            }

            SyntaxEvent::OpenTagEnd(e) => {
                // Only process top-level script tags
                if e.nested_level == 0
                    && !e.self_closing
                    && ctx.bytes[e.start as usize..e.end as usize].starts_with(b"<script")
                {
                    self.script_tag_span = Some(Span {
                        start: e.start,
                        end: e.end,
                    });
                }
                SyntaxResult::Keep(SyntaxEvent::OpenTagEnd(e))
            }

            SyntaxEvent::CloseTag(e) => {
                if e.nested_level == 0
                    && ctx.bytes[e.start as usize..e.name_end as usize].starts_with(b"</script")
                {
                    if let Some(script_span) = self.script_tag_span.take() {
                        let content_start = script_span.end;
                        let content_end = e.start;

                        let source = ctx.slice(content_start, content_end);
                        let parsed = self.parse_program(source);

                        let lang = if let Some(lang_span) = &self.script_lang_span {
                            let lang = &ctx.bytes[lang_span.start as usize..lang_span.end as usize];

                            match lang {
                                b"js" => Some(ScriptLanguage::JavaScript),
                                b"ts" => Some(ScriptLanguage::TypeScript),
                                b"jsx" => Some(ScriptLanguage::JSX),
                                b"tsx" => Some(ScriptLanguage::TSX),
                                _ => Some(ScriptLanguage::Unknown),
                            }
                        } else {
                            None
                        };

                        let generic = if let Some(generic_span) = &self.script_generic_span {
                            let generic =
                                &ctx.input[generic_span.start as usize..generic_span.end as usize];
                            Some(parse_generic(self.alloc, generic, generic_span.start))
                        } else {
                            None
                        };

                        let setup = self.script_setup_span.take();

                        let script_content = OxcScriptContent {
                            element_id: e.element_id,
                            parent_id: 0,

                            tag_open_start: script_span.start,
                            tag_open_end: script_span.end,

                            tag_close_start: e.start,
                            tag_close_end: e.end,

                            content_start,
                            content_end,

                            program: parsed.program,
                            errors: parsed.errors,

                            lang,
                            setup,
                            generic,
                            attrs: self.script_attrs_span.take(),
                            attributes: self.script_element_props.drain(..).collect(),
                        };

                        let ev = SyntaxEvent::OxcScriptContent(script_content);
                        self.script_tag_span = None;
                        self.script_element_id = None;

                        return SyntaxResult::Replace(ev);
                    }
                }
                SyntaxResult::Keep(SyntaxEvent::CloseTag(e))
            }
            SyntaxEvent::Interpolation(e) => {
                // Use content_start/content_end to get the actual expression content
                // (excludes the {{ and }} delimiters)
                let source = &ctx.input[e.content_start as usize..e.content_end as usize];
                let result = self.parse_expression(source);

                let mut errors = None;
                let mut expression = None;
                let mut bindings = None;

                match result {
                    Ok(expr) => {
                        // Use content_start as offset so binding positions are absolute
                        let binding_ctx = BindingContext::new(e.content_start);
                        bindings = Some(extract_bindings_from_expression(
                            &expr,
                            source,
                            &binding_ctx,
                        ));
                        expression = Some(expr);
                    }
                    Err(errs) => {
                        errors = Some(errs);
                    }
                }

                let interpolation: OxcInterpolation<'a> = OxcInterpolation {
                    parent_id: e.parent_id,

                    start: e.start,
                    end: e.end,

                    errors,
                    expression,
                    event: e,

                    bindings,
                };

                SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(interpolation))
            }
            SyntaxEvent::Prop(e) => {
                // Check if this prop belongs to a root-level script tag we're tracking
                if let Some(script_id) = self.script_element_id {
                    if e.element_id == script_id {
                        // Check if this is the `setup` attribute
                        let name = &ctx.bytes[e.start as usize..e.name_end as usize];
                        if name == b"setup" {
                            self.script_setup_span = Some(Span {
                                start: e.start,
                                end: e.end,
                            });
                        } else if name == b"lang" {
                            if let Some(value) = &e.value {
                                self.script_lang_span = Some(Span {
                                    start: value.start,
                                    end: value.end,
                                });
                            }
                        } else if name == b"generic" {
                            if let Some(value) = &e.value {
                                self.script_generic_span = Some(Span {
                                    start: value.start,
                                    end: value.end,
                                });
                            }
                        } else if name == b"attrs" {
                            if let Some(value) = &e.value {
                                self.script_attrs_span = Some(Span {
                                    start: value.start,
                                    end: value.end,
                                });
                            }
                        }
                    }
                    return SyntaxResult::Drop;
                }
                if e.is_directive {
                    let name = &ctx.bytes[e.start as usize..e.name_end as usize];

                    if name == b"v-for" {
                        if let Some(value) = &e.value {
                            let source = &ctx.input[value.start as usize..value.end as usize];
                            let parsed =
                                parse_vfor_with_bindings(self.alloc, source, self.source_type);

                            let vfor_binding = OxcVForProp {
                                element_id: e.element_id,
                                parent_id: e.parent_id,
                                start: e.start,
                                parsed,
                                event: e,
                            };

                            return SyntaxResult::Replace(SyntaxEvent::OxcVFor(vfor_binding));
                        } else {
                            // TODO add error - v-for requires a value
                            return SyntaxResult::Keep(SyntaxEvent::Prop(e));
                        }
                    } else if name == b"v-slot" || name == b"#" {
                        // v-slot can have a value (params) or not: #default vs #default="{ item }"
                        let source = if let Some(value) = &e.value {
                            &ctx.input[value.start as usize..value.end as usize]
                        } else {
                            "" // No params - parse_vslot handles empty strings
                        };
                        let parsed =
                            parse_vslot_with_bindings(self.alloc, source, self.source_type);

                        let vslot_binding = OxcVSlotProp {
                            element_id: e.element_id,
                            parent_id: e.parent_id,
                            start: e.start,
                            parsed,
                            event: e,
                        };

                        return SyntaxResult::Replace(SyntaxEvent::OxcVSlot(vslot_binding));
                    } else if name == b"v-if" || name == b"v-else-if" || name == b"v-else" {
                        let mut errors = None;
                        let expression = match &e.value {
                            Some(value) if name != b"v-else" => {
                                let source = &ctx.input[value.start as usize..value.end as usize];
                                match self.parse_expression(source) {
                                    Ok(expr) => Some(expr),
                                    Err(err) => {
                                        errors = Some(err);
                                        None
                                    }
                                }
                            }
                            _ => None,
                        };

                        let bindings = if let (Some(expr), Some(value)) = (&expression, &e.value) {
                            let binding_ctx = BindingContext::new(value.start);
                            Some(extract_bindings_from_expression(
                                expr,
                                &ctx.input[value.start as usize..value.end as usize],
                                &binding_ctx,
                            ))
                        } else {
                            None
                        };

                        let vconditional_binding = OxcVConditional {
                            element_id: e.element_id,
                            parent_id: e.parent_id,
                            condition_type: if name == b"v-if" {
                                OxcVConditionType::If
                            } else if name == b"v-else-if" {
                                OxcVConditionType::ElseIf
                            } else {
                                OxcVConditionType::Else
                            },
                            start: e.start,
                            end: e.end,
                            expression,
                            errors,
                            event: e,
                            bindings,
                        };

                        return SyntaxResult::Replace(SyntaxEvent::OxcVConditional(
                            vconditional_binding,
                        ));
                    }
                    // Fall through to emit OxcProp for other directives (e.g., :class, @click)
                }

                // Emit OxcProp for all props (directives and non-directives)
                // that aren't handled by the special cases above
                let exp = match &e.value {
                    Some(v) => {
                        let source = &ctx.input[v.start as usize..v.end as usize];
                        match self.parse_expression(source) {
                            Ok(expr) => {
                                let binding_ctx = BindingContext::new(v.start);
                                let bindings = Some(extract_bindings_from_expression(
                                    &expr,
                                    source,
                                    &binding_ctx,
                                ));
                                Some(OxcPropProcessed {
                                    start: v.start,
                                    end: v.end,
                                    expression: Some(expr),
                                    errors: None,
                                    bindings,
                                })
                            }
                            Err(errs) => Some(OxcPropProcessed {
                                start: v.start,
                                end: v.end,
                                expression: None,
                                errors: Some(errs),
                                bindings: None,
                            }),
                        }
                    }
                    None => None,
                };

                let arg = match &e.arg {
                    Some(arg) => {
                        if arg.is_dynamic {
                            let source = &ctx.input[arg.start as usize..arg.end as usize];
                            match self.parse_expression(source) {
                                Ok(expr) => {
                                    let binding_ctx = BindingContext::new(arg.start);
                                    let bindings = Some(extract_bindings_from_expression(
                                        &expr,
                                        source,
                                        &binding_ctx,
                                    ));
                                    Some(OxcPropProcessed {
                                        start: arg.start,
                                        end: arg.end,
                                        expression: Some(expr),
                                        errors: None,
                                        bindings,
                                    })
                                }
                                Err(errs) => Some(OxcPropProcessed {
                                    start: arg.start,
                                    end: arg.end,
                                    expression: None,
                                    errors: Some(errs),
                                    bindings: None,
                                }),
                            }
                        } else {
                            None
                        }
                    }
                    None => None,
                };

                let modifiers = e.modifiers.clone();
                let prop_event = OxcProp {
                    element_id: e.element_id,
                    parent_id: e.parent_id,
                    start: e.start,
                    exp,
                    arg,
                    modifiers,
                    event: e,
                };

                SyntaxResult::Replace(SyntaxEvent::OxcProp(prop_event))
            }
            other => SyntaxResult::Keep(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;
    use crate::syntax::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax::syntax::Syntax;
    use crate::syntax::types::*;
    use crate::tokenizer::byte::tokenize;
    use oxc_ast::ast::Expression;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Test plugin that collects all emitted SyntaxEvents after OxcParserPlugin processing
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
            // Store a marker based on event type - we can't clone the full event due to lifetimes
            // Instead we'll check the event type directly in tests
            match &event {
                SyntaxEvent::OxcInterpolation(interp) => {
                    // Store info about the interpolation
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Interpolation(interp.event.clone()));
                }
                SyntaxEvent::OxcVFor(vfor) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Prop(vfor.event.clone()));
                }
                SyntaxEvent::OxcVSlot(vslot) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Prop(vslot.event.clone()));
                }
                SyntaxEvent::OxcVConditional(vcond) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Prop(vcond.event.clone()));
                }
                SyntaxEvent::OxcProp(prop) => {
                    self.events
                        .borrow_mut()
                        .push(SyntaxEvent::Prop(prop.event.clone()));
                }
                _ => {}
            }
            SyntaxResult::Replace(event)
        }
    }

    /// Helper: process input through tokenizer -> syntax -> OxcParserPlugin pipeline
    /// Returns counts of different event types for verification
    struct EventCounts {
        interpolations: usize,
        directives: usize,
        props: usize,
    }

    fn process_with_oxc_plugin(input: &str, alloc: &oxc_allocator::Allocator) -> EventCounts {
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);

        let collected_events: Rc<RefCell<EventCounts>> = Rc::new(RefCell::new(EventCounts {
            interpolations: 0,
            directives: 0,
            props: 0,
        }));

        struct CounterPlugin {
            counts: Rc<RefCell<EventCounts>>,
        }

        impl<'a> SyntaxPlugin<'a> for CounterPlugin {
            fn name(&self) -> &str {
                "counter"
            }

            fn process_event(
                &mut self,
                event: SyntaxEvent<'a>,
                _ctx: &mut SyntaxPluginContext<'a>,
            ) -> SyntaxResult<SyntaxEvent<'a>> {
                let mut counts = self.counts.borrow_mut();
                match &event {
                    SyntaxEvent::OxcInterpolation(_) => counts.interpolations += 1,
                    SyntaxEvent::OxcVFor(_)
                    | SyntaxEvent::OxcVSlot(_)
                    | SyntaxEvent::OxcVConditional(_) => counts.directives += 1,
                    SyntaxEvent::OxcProp(_) => counts.props += 1,
                    _ => {}
                }
                SyntaxResult::Replace(event)
            }
        }

        let mut counter = CounterPlugin {
            counts: collected_events.clone(),
        };
        let mut oxc_plugin = OxcParserPlugin::new(alloc, SourceType::tsx());

        let mut syntax = Syntax::new(vec![&mut oxc_plugin, &mut counter]);
        syntax.start(&mut ctx);

        tokenize(input.as_bytes(), |e| {
            syntax.handle(&e, &mut ctx);
        });

        syntax.end(&mut ctx);

        let counts = collected_events.borrow();
        EventCounts {
            interpolations: counts.interpolations,
            directives: counts.directives,
            props: counts.props,
        }
    }

    // ==================== Interpolation Tests ====================

    #[test]
    fn test_interpolation_simple_expression() {
        let alloc = oxc_allocator::Allocator::default();
        let input = "<div>{{ message }}</div>";
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        // Create interpolation event for "{{ message }}"
        let interp_event = SyntaxInterpolation {
            parent_id: 0,
            start: 5,
            end: 18,
            content_start: 7,
            content_end: 16,
        };

        let result = plugin.process_event(SyntaxEvent::Interpolation(interp_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(oxc_interp)) = result {
            assert!(
                oxc_interp.expression.is_some(),
                "Expression should be parsed"
            );
            assert!(oxc_interp.errors.is_none(), "Should have no errors");
            assert_eq!(oxc_interp.start, 5, "Start offset should be preserved");

            // Verify the expression is an identifier
            if let Some(Expression::Identifier(id)) = &oxc_interp.expression {
                assert_eq!(id.name.as_str(), "message");
            } else {
                panic!("Expected Identifier expression");
            }
        } else {
            panic!("Expected Replace with OxcInterpolation");
        }
    }

    #[test]
    fn test_interpolation_member_expression() {
        let alloc = oxc_allocator::Allocator::default();
        let input = "<div>{{ user.name }}</div>";
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let interp_event = SyntaxInterpolation {
            parent_id: 0,
            start: 5,
            end: 20,
            content_start: 7,
            content_end: 18,
        };

        let result = plugin.process_event(SyntaxEvent::Interpolation(interp_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(oxc_interp)) = result {
            assert!(oxc_interp.expression.is_some());
            // Should be a member expression
            if let Some(Expression::StaticMemberExpression(_)) = &oxc_interp.expression {
                // OK
            } else {
                panic!(
                    "Expected StaticMemberExpression, got {:?}",
                    oxc_interp.expression
                );
            }
        } else {
            panic!("Expected Replace with OxcInterpolation");
        }
    }

    #[test]
    fn test_interpolation_call_expression() {
        let alloc = oxc_allocator::Allocator::default();
        let input = "<div>{{ formatDate(date) }}</div>";
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let interp_event = SyntaxInterpolation {
            parent_id: 0,
            start: 5,
            end: 27,
            content_start: 7,
            content_end: 25,
        };

        let result = plugin.process_event(SyntaxEvent::Interpolation(interp_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(oxc_interp)) = result {
            assert!(oxc_interp.expression.is_some());
            if let Some(Expression::CallExpression(_)) = &oxc_interp.expression {
                // OK
            } else {
                panic!("Expected CallExpression");
            }
        } else {
            panic!("Expected Replace with OxcInterpolation");
        }
    }

    #[test]
    fn test_interpolation_ternary_expression() {
        let alloc = oxc_allocator::Allocator::default();
        let input = "<div>{{ show ? 'yes' : 'no' }}</div>";
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let interp_event = SyntaxInterpolation {
            parent_id: 0,
            start: 5,
            end: 30,
            content_start: 7,
            content_end: 28,
        };

        let result = plugin.process_event(SyntaxEvent::Interpolation(interp_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(oxc_interp)) = result {
            assert!(oxc_interp.expression.is_some());
            if let Some(Expression::ConditionalExpression(_)) = &oxc_interp.expression {
                // OK
            } else {
                panic!("Expected ConditionalExpression");
            }
        } else {
            panic!("Expected Replace with OxcInterpolation");
        }
    }

    #[test]
    fn test_interpolation_invalid_expression() {
        let alloc = oxc_allocator::Allocator::default();
        // Use truly invalid JS syntax: unclosed parenthesis
        let input = "<div>{{ (a + }}</div>";
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        // {{ (a + }} - content is " (a + " (positions 7-13)
        let interp_event = SyntaxInterpolation {
            parent_id: 0,
            start: 5,
            end: 15,
            content_start: 7,
            content_end: 13,
        };

        let result = plugin.process_event(SyntaxEvent::Interpolation(interp_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(oxc_interp)) = result {
            assert!(
                oxc_interp.expression.is_none(),
                "Invalid expression should fail to parse"
            );
            assert!(oxc_interp.errors.is_some(), "Should have parse errors");
        } else {
            panic!("Expected Replace with OxcInterpolation");
        }
    }

    // ==================== v-for Directive Tests ====================

    #[test]
    fn test_vfor_simple() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-for="item of items"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        // v-for="item of items" at position 5
        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 26,
            name_end: 10,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 12, end: 25 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVFor(vfor)) = result {
            assert!(vfor.left().is_some(), "Left side should be parsed");
            assert!(vfor.right().is_some(), "Right side should be parsed");
            assert!(vfor.is_of(), "Should use 'of' keyword");
            assert!(!vfor.has_errors(), "Should have no errors");

            // Check left is identifier "item"
            if let Some(Expression::Identifier(id)) = &vfor.left() {
                assert_eq!(id.name.as_str(), "item");
            } else {
                panic!("Expected Identifier for left side");
            }

            // Check right is identifier "items"
            if let Some(Expression::Identifier(id)) = &vfor.right() {
                assert_eq!(id.name.as_str(), "items");
            } else {
                panic!("Expected Identifier for right side");
            }
        } else {
            panic!("Expected Replace with OxcVFor");
        }
    }

    #[test]
    fn test_vfor_with_index() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-for="(item, index) in items"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 35,
            name_end: 10,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 12, end: 34 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVFor(vfor)) = result {
            assert!(vfor.left().is_some());
            assert!(vfor.right().is_some());
            assert!(!vfor.is_of(), "Should use 'in' keyword");

            // Left should be parenthesized sequence expression
            if let Some(Expression::ParenthesizedExpression(paren)) = &vfor.left() {
                if let Expression::SequenceExpression(seq) = &paren.expression {
                    assert_eq!(seq.expressions.len(), 2);
                } else {
                    panic!("Expected SequenceExpression");
                }
            } else {
                panic!("Expected ParenthesizedExpression");
            }
        } else {
            panic!("Expected Replace with OxcVFor");
        }
    }

    #[test]
    fn test_vfor_destructuring() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-for="{ id, name } of users"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 34,
            name_end: 10,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 12, end: 33 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVFor(vfor)) = result {
            assert!(vfor.left().is_some());
            // Left should be object expression (destructuring pattern)
            if let Some(Expression::ObjectExpression(_)) = &vfor.left() {
                // OK
            } else {
                panic!("Expected ObjectExpression for destructuring");
            }
        } else {
            panic!("Expected Replace with OxcVFor");
        }
    }

    // ==================== v-slot Directive Tests ====================

    #[test]
    fn test_vslot_simple() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<template v-slot="{ data }"></template>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 10,
            end: 27,
            name_end: 16,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 18, end: 26 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVSlot(vslot)) = result {
            assert!(vslot.params().is_some(), "Params should be parsed");
            assert!(!vslot.has_errors(), "Should have no errors");

            let params = vslot.params().unwrap();
            assert_eq!(params.items.len(), 1, "Should have 1 parameter");
        } else {
            panic!("Expected Replace with OxcVSlot");
        }
    }

    #[test]
    fn test_vslot_multiple_params() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<template v-slot="item, index"></template>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 10,
            end: 30,
            name_end: 16,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 18, end: 29 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVSlot(vslot)) = result {
            assert!(vslot.params().is_some());
            let params = vslot.params().unwrap();
            assert_eq!(params.items.len(), 2, "Should have 2 parameters");
        } else {
            panic!("Expected Replace with OxcVSlot");
        }
    }

    // ==================== v-if/v-else-if/v-else Directive Tests ====================

    #[test]
    fn test_vif_simple() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-if="show"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 16,
            name_end: 9,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 11, end: 15 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVConditional(vcond)) = result {
            assert!(matches!(vcond.condition_type, OxcVConditionType::If));
            assert!(vcond.expression.is_some(), "Expression should be parsed");
            assert!(vcond.errors.is_none(), "Should have no errors");

            if let Some(Expression::Identifier(id)) = &vcond.expression {
                assert_eq!(id.name.as_str(), "show");
            } else {
                panic!("Expected Identifier expression");
            }
        } else {
            panic!("Expected Replace with OxcVConditional");
        }
    }

    #[test]
    fn test_vif_complex_expression() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-if="count > 0 && isActive"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 33,
            name_end: 9,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 11, end: 32 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVConditional(vcond)) = result {
            assert!(vcond.expression.is_some());
            // Should be a logical expression
            if let Some(Expression::LogicalExpression(_)) = &vcond.expression {
                // OK
            } else {
                panic!("Expected LogicalExpression");
            }
        } else {
            panic!("Expected Replace with OxcVConditional");
        }
    }

    #[test]
    fn test_velseif() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-else-if="other"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 22,
            name_end: 14,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 16, end: 21 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVConditional(vcond)) = result {
            assert!(matches!(vcond.condition_type, OxcVConditionType::ElseIf));
            assert!(vcond.expression.is_some());
        } else {
            panic!("Expected Replace with OxcVConditional");
        }
    }

    #[test]
    fn test_velse() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-else></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 11,
            name_end: 11,
            is_directive: true,
            value: None, // v-else has no value
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVConditional(vcond)) = result {
            assert!(matches!(vcond.condition_type, OxcVConditionType::Else));
            assert!(
                vcond.expression.is_none(),
                "v-else should have no expression"
            );
        } else {
            panic!("Expected Replace with OxcVConditional");
        }
    }

    // ==================== Regular Prop Tests ====================

    #[test]
    fn test_regular_attribute() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div class="container"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 22,
            name_end: 10,
            is_directive: false,
            value: Some(SyntaxPropValue { start: 12, end: 21 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcProp(prop)) = result {
            assert!(prop.exp.is_some(), "Expression should be parsed");
            let exp = prop.exp.unwrap();
            assert!(exp.expression.is_some());
            // Value is parsed as identifier (without quotes from the tokenizer)
            if let Some(Expression::Identifier(id)) = &exp.expression {
                assert_eq!(id.name.as_str(), "container");
            } else {
                panic!("Expected Identifier, got {:?}", exp.expression);
            }
        } else {
            panic!("Expected Replace with OxcProp");
        }
    }

    #[test]
    fn test_attribute_no_value() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<input disabled>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 7,
            end: 15,
            name_end: 15,
            is_directive: false,
            value: None, // boolean attribute, no value
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcProp(prop)) = result {
            assert!(prop.exp.is_none(), "Boolean attribute should have no exp");
        } else {
            panic!("Expected Replace with OxcProp");
        }
    }

    // ==================== Integration Tests with Full Pipeline ====================

    #[test]
    fn test_full_pipeline_interpolation() {
        let alloc = oxc_allocator::Allocator::default();
        let counts = process_with_oxc_plugin("<div>{{ message }}</div>", &alloc);
        assert_eq!(
            counts.interpolations, 1,
            "Should have 1 interpolation event"
        );
    }

    #[test]
    fn test_full_pipeline_vfor() {
        let alloc = oxc_allocator::Allocator::default();
        let counts = process_with_oxc_plugin(r#"<div v-for="item of items"></div>"#, &alloc);
        assert_eq!(counts.directives, 1, "Should have 1 v-for directive event");
    }

    #[test]
    fn test_full_pipeline_multiple_directives() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-if="show" v-for="item of items">{{ item }}</div>"#;
        let counts = process_with_oxc_plugin(input, &alloc);
        assert_eq!(counts.directives, 2, "Should have 2 directive events");
        assert_eq!(counts.interpolations, 1, "Should have 1 interpolation");
    }

    #[test]
    fn test_full_pipeline_nested_elements() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-if="show"><span>{{ msg }}</span></div>"#;
        let counts = process_with_oxc_plugin(input, &alloc);
        assert_eq!(
            counts.interpolations, 1,
            "Should have 1 interpolation in nested span"
        );
    }

    // ==================== Offset Verification Tests ====================

    #[test]
    fn test_interpolation_offset_preservation() {
        let alloc = oxc_allocator::Allocator::default();
        let input = "prefix {{ expr }} suffix";
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        // Interpolation at position 7-17
        let interp_event = SyntaxInterpolation {
            parent_id: 0,
            start: 7,
            end: 17,
            content_start: 9,
            content_end: 15,
        };

        let result = plugin.process_event(SyntaxEvent::Interpolation(interp_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcInterpolation(oxc_interp)) = result {
            assert_eq!(oxc_interp.start, 7, "Start offset should be 7");
            assert_eq!(oxc_interp.event.start, 7, "Event start should be 7");
            assert_eq!(oxc_interp.event.end, 17, "Event end should be 17");

            // Verify the source slice matches
            let source_slice =
                &input[oxc_interp.event.start as usize..oxc_interp.event.end as usize];
            assert_eq!(
                source_slice, "{{ expr }}",
                "Source slice should match interpolation"
            );
        } else {
            panic!("Expected Replace with OxcInterpolation");
        }
    }

    #[test]
    fn test_vfor_offset_preservation() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<ul><li v-for="item of items">{{ item }}</li></ul>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        // v-for at position 8-29
        let prop_event = SyntaxProp {
            element_id: 4, // element is <li>
            parent_id: 0,  // parent is <ul>
            start: 8,
            end: 29,
            name_end: 13,
            is_directive: true,
            value: Some(SyntaxPropValue { start: 15, end: 28 }),
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVFor(vfor)) = result {
            assert_eq!(vfor.start, 8, "v-for start should be 8");
            assert_eq!(vfor.event.start, 8, "Event start should be 8");
            assert_eq!(vfor.event.end, 29, "Event end should be 29");

            // Verify value offset
            assert_eq!(vfor.event.value.as_ref().unwrap().start, 15);
            assert_eq!(vfor.event.value.as_ref().unwrap().end, 28);

            // Verify the value slice
            let value_slice = &input[15..28];
            assert_eq!(value_slice, "item of items", "Value slice should match");
        } else {
            panic!("Expected Replace with OxcVFor");
        }
    }

    #[test]
    fn test_vif_offset_preservation() {
        let alloc = oxc_allocator::Allocator::default();
        // Input: <div v-if="count > 0"></div>
        // Positions: v-if starts at 5, name_end at 9, value "count > 0" at 11-20
        let input = r#"<div v-if="count > 0"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 21,     // After closing quote
            name_end: 9, // After "v-if"
            is_directive: true,
            value: Some(SyntaxPropValue { start: 11, end: 20 }), // "count > 0" is 9 chars
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcVConditional(vcond)) = result {
            assert_eq!(vcond.start, 5, "v-if start should be 5");

            // Verify the value slice
            let value_slice = &input[11..20];
            assert_eq!(
                value_slice, "count > 0",
                "Value slice should match expression"
            );
        } else {
            panic!("Expected Replace with OxcVConditional");
        }
    }

    // ==================== Script Content Tests ====================

    #[test]
    fn test_script_content_parsing() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<script>const x = 1;</script>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        // First, process the OpenTagEnd event for <script>
        let open_tag_event = SyntaxOpenTagEnd {
            start: 0,
            end: 8,
            name_end: 7,
            tag_type: SyntaxTagType::Element,
            element_id: 0,
            parent_id: 0,
            self_closing: false,
            nested_level: 0,
            is_void_element: false,
        };

        let _ = plugin.process_event(SyntaxEvent::OpenTagEnd(open_tag_event), &mut ctx);

        // Then process the CloseTag event
        let close_tag_event = SyntaxCloseTag {
            element_id: 0, // script element starts at 0
            parent_id: 0,
            tag_type: SyntaxTagType::Element,
            start: 20,
            name_end: 28,
            end: 29,
            nested_level: 0,
            is_void_element: false,
        };

        let result = plugin.process_event(SyntaxEvent::CloseTag(close_tag_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcScriptContent(script)) = result {
            assert_eq!(
                script.content_start, 8,
                "Script content should start after <script>"
            );
            assert!(script.errors.is_empty(), "Should have no parse errors");

            // Check that the program has statements
            assert!(
                !script.program.body.is_empty(),
                "Program should have statements"
            );
        } else {
            panic!("Expected Replace with OxcScriptContent");
        }
    }

    #[test]
    fn test_script_content_with_typescript() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<script>const x: number = 1;</script>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let open_tag_event = SyntaxOpenTagEnd {
            start: 0,
            end: 8,
            name_end: 7,
            tag_type: SyntaxTagType::Element,
            element_id: 0,
            parent_id: 0,
            self_closing: false,
            nested_level: 0,
            is_void_element: false,
        };

        let _ = plugin.process_event(SyntaxEvent::OpenTagEnd(open_tag_event), &mut ctx);

        let close_tag_event = SyntaxCloseTag {
            element_id: 0, // script element starts at 0
            parent_id: 0,
            tag_type: SyntaxTagType::Element,
            start: 28,
            name_end: 36,
            end: 37,
            nested_level: 0,
            is_void_element: false,
        };

        let result = plugin.process_event(SyntaxEvent::CloseTag(close_tag_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcScriptContent(script)) = result {
            assert!(
                script.errors.is_empty(),
                "TypeScript should parse without errors"
            );
        } else {
            panic!("Expected Replace with OxcScriptContent");
        }
    }

    // ==================== Edge Case Tests ====================

    #[test]
    fn test_vfor_without_value() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div v-for></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 10,
            name_end: 10,
            is_directive: true,
            value: None, // No value - invalid v-for
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        // Should return the original Prop event since v-for without value is invalid
        assert!(matches!(result, SyntaxResult::Keep(SyntaxEvent::Prop(_))));
    }

    #[test]
    fn test_vslot_without_value() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<template v-slot></template>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 10,
            end: 16,
            name_end: 16,
            is_directive: true,
            value: None, // No value - v-slot without params like #default
            arg: None,
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        // v-slot without value is valid (e.g., #default) and should be converted to OxcVSlot
        assert!(matches!(
            result,
            SyntaxResult::Replace(SyntaxEvent::OxcVSlot(_))
        ));
    }

    #[test]
    fn test_non_directive_with_dynamic_arg() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<div :[key]="value"></div>"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 5,
            end: 19,
            name_end: 6,
            is_directive: false,
            value: Some(SyntaxPropValue { start: 13, end: 18 }),
            arg: Some(SyntaxPropArg {
                start: 6,
                end: 11,
                is_dynamic: true,
            }),
            modifiers: None,
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcProp(prop)) = result {
            // Dynamic arg should be parsed
            assert!(prop.arg.is_some(), "Dynamic arg should be parsed");
            let arg = prop.arg.unwrap();
            assert!(arg.expression.is_some(), "Arg expression should be parsed");

            // Value should also be parsed
            assert!(prop.exp.is_some(), "Value should be parsed");
        } else {
            panic!("Expected Replace with OxcProp");
        }
    }

    #[test]
    fn test_prop_with_modifiers() {
        let alloc = oxc_allocator::Allocator::default();
        let input = r#"<input @click.prevent.stop="handler">"#;
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);
        let mut plugin = OxcParserPlugin::new(&alloc, SourceType::tsx());

        let prop_event = SyntaxProp {
            element_id: 0,
            parent_id: NO_PARENT,
            start: 7,
            end: 36,
            name_end: 8,
            is_directive: false, // @ is shorthand but not full directive name
            value: Some(SyntaxPropValue { start: 28, end: 35 }),
            arg: Some(SyntaxPropArg {
                start: 8,
                end: 13,
                is_dynamic: false,
            }),
            modifiers: Some(vec![
                Span { start: 14, end: 21 }, // "prevent"
                Span { start: 22, end: 26 }, // "stop"
            ]),
            quote: None,
        };

        let result = plugin.process_event(SyntaxEvent::Prop(prop_event), &mut ctx);

        if let SyntaxResult::Replace(SyntaxEvent::OxcProp(prop)) = result {
            // Modifiers should be preserved
            assert!(prop.modifiers.is_some());
            let modifiers = prop.modifiers.unwrap();
            assert_eq!(modifiers.len(), 2);

            // Verify modifier offsets
            assert_eq!(
                &input[modifiers[0].start as usize..modifiers[0].end as usize],
                "prevent"
            );
            assert_eq!(
                &input[modifiers[1].start as usize..modifiers[1].end as usize],
                "stop"
            );
        } else {
            panic!("Expected Replace with OxcProp");
        }
    }
}
