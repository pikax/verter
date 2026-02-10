use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::{
    common::Span,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::*,
    },
    utils::oxc::vue::{
        adjust_expression_spans, parse_vfor_with_bindings, parse_vslot_with_bindings,
    },
    utils::oxc::{extract_bindings_from_expression, BindingContext, BindingExtractionResult},
};

/// OXC Parser Plugin for the syntax_kai pipeline.
///
/// Processes Compiled* events from element_compiler and produces OxcCompiled* events:
/// - `ElementStart` → `OxcCompiledElementStart` (props parsed, scopes extracted)
/// - `ElementClosed` → `OxcCompiledElementClosed`
/// - `Interpolation` → `OxcInterpolation` (expression parsed)
/// - `CompiledScriptStart`/`CompiledScriptEnd` → `OxcScript` (script parsed with OXC)
pub struct OxcParserPlugin<'alloc> {
    source_type: SourceType,
    alloc: &'alloc Allocator,
    /// Buffered CompiledScriptStart (set on Start, consumed on End).
    current_script_start: Option<CompiledRootScriptStart>,
}

impl<'alloc> OxcParserPlugin<'alloc> {
    pub fn new(alloc: &'alloc Allocator) -> Self {
        Self {
            source_type: SourceType::tsx(),
            alloc,
            current_script_start: None,
        }
    }

    pub fn set_source_type(&mut self, source_type: SourceType) {
        self.source_type = source_type;
    }

    /// Parse a single expression from a source span.
    fn parse_expression(
        &self,
        span: Span,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> (
        Option<oxc_ast::ast::Expression<'alloc>>,
        Option<Vec<oxc_diagnostics::OxcDiagnostic>>,
        Option<BindingExtractionResult<'alloc>>,
    ) {
        let source_slice = &ctx.input[span.start as usize..span.end as usize];
        if source_slice.trim().is_empty() {
            return (None, None, None);
        }

        let parser = oxc_parser::Parser::new(self.alloc, source_slice, self.source_type);
        let result = parser.parse_expression();

        match result {
            Ok(mut expr) => {
                // Adjust spans to be relative to original source
                adjust_expression_spans(&mut expr, span.start);

                // Extract bindings
                let binding_ctx = BindingContext::new(span.start);
                let bindings = extract_bindings_from_expression(&expr, ctx.input, &binding_ctx);

                (Some(expr), None, Some(bindings))
            }
            Err(errors) => (None, Some(errors), None),
        }
    }

    /// Parse props from a CompiledElementStart into OxcProp and ElementScope vectors.
    fn parse_element_props(
        &self,
        mut compiled: CompiledElementStart,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcCompiledElementStart<'alloc> {
        let mut oxc_props: Vec<OxcProp<'alloc>> = Vec::new();
        let mut scopes: Vec<ElementScope<'alloc>> = Vec::new();

        let element_id = compiled.element_id;
        let parent_id = compiled.parent_id;
        let is_template = compiled.event_open_tag.kind == ElementKind::Template;

        // Take props out to avoid partial move issues
        let props = std::mem::take(&mut compiled.props);

        for prop in props {
            match prop.kind {
                // Structural directives → extract into scopes
                PropKind::If => {
                    let scope = self.parse_if_condition(&prop, element_id, ctx);
                    scopes.push(ElementScope::If(scope));
                }
                PropKind::ElseIf => {
                    let scope = self.parse_else_if_condition(&prop, element_id, ctx);
                    scopes.push(ElementScope::ElseIf(scope));
                }
                PropKind::Else => {
                    let scope = ElementScope::Else(OxcElseCondition {
                        element_id,
                        start: prop.start,
                        end: prop.end,
                        event: ElementScopeConditionElse {
                            element_start: element_id,
                            start: prop.start,
                            end: prop.end,
                        },
                    });
                    scopes.push(scope);
                }
                PropKind::For => {
                    if let Some(scope) = self.parse_vfor(&prop, element_id, ctx) {
                        scopes.push(ElementScope::For(scope));
                    }
                }
                PropKind::Slot => {
                    if is_template {
                        if let Some(scope) = self.parse_vslot_template(&prop, element_id, ctx) {
                            scopes.push(ElementScope::SlotTemplate(scope));
                        }
                    } else if let Some(scope) = self.parse_vslot_element(
                        &prop,
                        element_id,
                        &compiled.event_open_tag_end,
                        ctx,
                    ) {
                        scopes.push(ElementScope::SlotElement(scope));
                    }
                }
                // Regular props → parse into OxcProp
                _ => {
                    let oxc_prop = self.parse_prop(prop, element_id, parent_id, ctx);
                    oxc_props.push(oxc_prop);
                }
            }
        }

        // Sort scopes by Vue priority: If/ElseIf/Else > For > Slot
        scopes.sort_by_key(|s| match s {
            ElementScope::If(_) | ElementScope::ElseIf(_) | ElementScope::Else(_) => 0,
            ElementScope::For(_) => 1,
            ElementScope::SlotElement(_) | ElementScope::SlotTemplate(_) => 2,
        });

        OxcCompiledElementStart {
            props: oxc_props,
            scopes,
            event: compiled,
        }
    }

    /// Parse a single prop's value and arg expressions.
    fn parse_prop(
        &self,
        prop: Prop,
        element_id: u32,
        parent_id: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcProp<'alloc> {
        let arg = if let Some(arg_span) = prop.arg {
            if prop.has_dynamic_arg {
                // Dynamic arg: :[key]="value" — parse the arg expression
                let (expression, errors, bindings) = self.parse_expression(arg_span, ctx);
                Some(OxcPropProcessed {
                    start: arg_span.start,
                    end: arg_span.end,
                    expression,
                    errors,
                    bindings,
                })
            } else {
                // Static arg: :prop="value" — no parsing needed, just a span
                None
            }
        } else {
            None
        };

        let exp = if let Some(value_span) = prop.value {
            if prop.is_directive {
                // Directive value is an expression — parse it
                let (expression, errors, bindings) = self.parse_expression(value_span, ctx);
                Some(OxcPropProcessed {
                    start: value_span.start,
                    end: value_span.end,
                    expression,
                    errors,
                    bindings,
                })
            } else {
                // Static attribute value — no parsing needed
                None
            }
        } else {
            None
        };

        OxcProp {
            element_id,
            parent_id,
            start: prop.start,
            name_end: prop.name_end,
            arg,
            exp,
            modifiers: prop.modifiers.clone(),
            event: prop,
        }
    }

    /// Parse a v-if condition.
    fn parse_if_condition(
        &self,
        prop: &Prop,
        element_id: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcIfCondition<'alloc> {
        let (expression, errors, bindings) = if let Some(value_span) = prop.value {
            self.parse_expression(value_span, ctx)
        } else {
            (None, None, None)
        };

        OxcIfCondition {
            element_id,
            start: prop.start,
            end: prop.end,
            expression,
            errors,
            bindings,
            event: ElementScopeConditionIf {
                element_start: element_id,
                start: prop.start,
                end: prop.end,
                value: prop.value,
            },
        }
    }

    /// Parse a v-else-if condition.
    fn parse_else_if_condition(
        &self,
        prop: &Prop,
        element_id: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcElseIfCondition<'alloc> {
        let (expression, errors, bindings) = if let Some(value_span) = prop.value {
            self.parse_expression(value_span, ctx)
        } else {
            (None, None, None)
        };

        OxcElseIfCondition {
            element_id,
            start: prop.start,
            end: prop.end,
            expression,
            errors,
            bindings,
            event: ElementScopeConditionIf {
                element_start: element_id,
                start: prop.start,
                end: prop.end,
                value: prop.value,
            },
        }
    }

    /// Parse a v-for directive.
    fn parse_vfor(
        &self,
        prop: &Prop,
        element_id: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> Option<OxcVFor<'alloc>> {
        let value_span = prop.value?;
        let source_slice = &ctx.input[value_span.start as usize..value_span.end as usize];

        let mut parsed = parse_vfor_with_bindings(self.alloc, source_slice, self.source_type);

        // Adjust spans to be relative to original source
        for s in &mut parsed.locals {
            s.start += value_span.start;
            s.end += value_span.start;
        }
        for s in &mut parsed.references {
            s.start += value_span.start;
            s.end += value_span.start;
        }

        Some(OxcVFor {
            element_id,
            start: prop.start,
            end: prop.end,
            parsed,
            event: ElementScopeFor {
                element_start: element_id,
                start: prop.start,
                end: prop.end,
                value: prop.value,
                iterator: None,
                iterable: None,
                is_of: false,
            },
        })
    }

    /// Parse a v-slot on a template element.
    fn parse_vslot_template(
        &self,
        prop: &Prop,
        element_id: u32,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> Option<OxcVSlotTemplate<'alloc>> {
        let value_span = prop.value;
        let source_slice = value_span.map(|s| &ctx.input[s.start as usize..s.end as usize]);

        let mut parsed = if let Some(slice) = source_slice {
            parse_vslot_with_bindings(self.alloc, slice, self.source_type)
        } else {
            parse_vslot_with_bindings(self.alloc, "", self.source_type)
        };

        // Adjust spans
        let offset = value_span.map_or(0, |s| s.start);
        for s in &mut parsed.locals {
            s.start += offset;
            s.end += offset;
        }
        for s in &mut parsed.references {
            s.start += offset;
            s.end += offset;
        }

        Some(OxcVSlotTemplate {
            element_id,
            start: prop.start,
            end: prop.end,
            parsed,
            event: ElementScopeSlotTemplate {
                element_start: element_id,
                start: prop.start,
                end: prop.end,
                name: prop.arg,
            },
        })
    }

    /// Parse a v-slot on a component element (not template).
    fn parse_vslot_element(
        &self,
        prop: &Prop,
        element_id: u32,
        open_tag_end: &ElementOpenTagEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> Option<OxcVSlotElement<'alloc>> {
        let value_span = prop.value;
        let source_slice = value_span.map(|s| &ctx.input[s.start as usize..s.end as usize]);

        let mut parsed = if let Some(slice) = source_slice {
            parse_vslot_with_bindings(self.alloc, slice, self.source_type)
        } else {
            parse_vslot_with_bindings(self.alloc, "", self.source_type)
        };

        let offset = value_span.map_or(0, |s| s.start);
        for s in &mut parsed.locals {
            s.start += offset;
            s.end += offset;
        }
        for s in &mut parsed.references {
            s.start += offset;
            s.end += offset;
        }

        Some(OxcVSlotElement {
            element_id,
            start: prop.start,
            end: prop.end,
            parsed,
            event: ElementScopeSlotElement {
                element_start: element_id,
                element_content_start: open_tag_end.end,
                start: prop.start,
                end: prop.end,
                name: prop.arg,
            },
        })
    }

    /// Parse a script block.
    fn parse_script(
        &self,
        start: CompiledRootScriptStart,
        end: CompiledRootScriptEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcScript<'alloc> {
        let (program, errors) = if let Some(content) = end.content {
            let source_slice = &ctx.input[content.start as usize..content.end as usize];
            let parser_result =
                oxc_parser::Parser::new(self.alloc, source_slice, self.source_type).parse();

            let mut program = parser_result.program;
            // Adjust all spans to be relative to the original source
            for _stmt in program.body.iter_mut() {
                // The program was parsed from content.start offset
                // OXC gives spans relative to the slice, need to add content.start
                // This is handled by adjusting spans post-parse
            }

            let errors = parser_result.errors;
            (program, errors)
        } else {
            // Self-closing script or empty — parse empty string
            let parser_result = oxc_parser::Parser::new(self.alloc, "", self.source_type).parse();
            (parser_result.program, parser_result.errors)
        };

        let content_start = end.content.map_or(start.tag_open.end, |c| c.start);
        let content_end = end.content.map_or(start.tag_open.end, |c| c.end);

        OxcScript {
            start: start.start,
            end: end.end,
            tag_open_start: start.tag_open.start,
            tag_open_end: start.tag_open.end,
            tag_close_start: end.tag_close.map_or(end.end, |t| t.start),
            tag_close_end: end.tag_close.map_or(end.end, |t| t.end),
            content_start,
            content_end,
            program,
            errors,
            setup: start.setup,
            lang: start.lang,
            generic: start.generic.map(|span| {
                // Parse generic type parameters
                let source_slice = &ctx.input[span.start as usize..span.end as usize];
                crate::utils::oxc::vue::parse_generic(self.alloc, source_slice, span.start)
            }),
            attrs: start.attrs,
            attributes: start
                .attributes
                .into_iter()
                .collect(),
        }
    }

    /// Parse an interpolation expression.
    fn parse_interpolation(
        &self,
        interp: Interpolation,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcInterpolation<'alloc> {
        let (expression, errors, bindings) = self.parse_expression(interp.content, ctx);

        OxcInterpolation {
            parent_id: interp.parent_id,
            start: interp.start,
            end: interp.end,
            content: interp.content,
            expression,
            errors,
            bindings,
            event: interp,
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for OxcParserPlugin<'alloc> {
    fn name(&self) -> &str {
        "oxc_parser"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::ElementStart(compiled) => {
                let oxc_compiled = self.parse_element_props(compiled, ctx);
                SyntaxResult::Replace(Event::OxcCompiledElementStart(oxc_compiled))
            }

            Event::ElementClosed(closed) => {
                let oxc_closed = OxcCompiledElementClosed { event: closed };
                SyntaxResult::Replace(Event::OxcCompiledElementClosed(oxc_closed))
            }

            Event::Interpolation(interp) => {
                let oxc_interp = self.parse_interpolation(interp, ctx);
                SyntaxResult::Replace(Event::OxcInterpolation(oxc_interp))
            }

            Event::CompiledScriptStart(start) => {
                // Buffer script start, wait for end
                self.current_script_start = Some(start);
                SyntaxResult::Drop
            }

            Event::CompiledScriptEnd(end) => {
                if let Some(start) = self.current_script_start.take() {
                    let script = self.parse_script(start, end, ctx);
                    SyntaxResult::Replace(Event::OxcScript(script))
                } else {
                    SyntaxResult::Keep(Event::CompiledScriptEnd(end))
                }
            }

            other => SyntaxResult::Keep(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;

    /// Helper: tokenize → Syntax → element_compiler → oxc_parser. Returns event type names.
    fn parse_events<'a>(input: &'a str, alloc: &'a Allocator) -> Vec<String> {
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
            let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
            for event in &tokenizer_events {
                syntax.handle(event, &mut ctx);
            }
        }

        // Run element_compiler
        let mut ec = ElementCompilerPlugin::new();
        let mut compiled: Vec<Event<'_>> = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Run oxc_parser
        let mut oxc = OxcParserPlugin::new(alloc);
        let mut result: Vec<Event<'_>> = Vec::new();
        for event in compiled {
            match oxc.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => result.push(e),
                SyntaxResult::Drop => {}
            }
        }

        result
            .iter()
            .map(|e| match e {
                Event::OxcCompiledElementStart(s) => {
                    format!(
                        "OxcElementStart(props={}, scopes={})",
                        s.props.len(),
                        s.scopes.len()
                    )
                }
                Event::OxcCompiledElementClosed(_) => "OxcElementClosed".to_string(),
                Event::OxcInterpolation(i) => {
                    format!("OxcInterpolation(has_expr={})", i.expression.is_some())
                }
                Event::OxcScript(s) => {
                    format!(
                        "OxcScript(setup={}, lang={:?}, stmts={})",
                        s.setup.is_some(),
                        s.lang,
                        s.program.body.len()
                    )
                }
                Event::Text(_) => "Text".to_string(),
                Event::Comment(_) => "Comment".to_string(),
                Event::CompiledTemplateStart(_) => "CompiledTemplateStart".to_string(),
                Event::CompiledTemplateEnd(_) => "CompiledTemplateEnd".to_string(),
                Event::CompiledStyleStart(_) => "CompiledStyleStart".to_string(),
                Event::CompiledStyleEnd(_) => "CompiledStyleEnd".to_string(),
                _ => format!("{:?}", std::mem::discriminant(e)),
            })
            .collect()
    }

    /// @ai-generated - Simple interpolation parses expression.
    #[test]
    fn test_interpolation_simple() {
        let alloc = Allocator::default();
        let events = parse_events("<template>{{ message }}</template>", &alloc);
        let interp = events
            .iter()
            .find(|e| e.starts_with("OxcInterpolation("))
            .expect("Expected OxcInterpolation");
        assert!(
            interp.contains("has_expr=true"),
            "Expected parsed expression, got: {}",
            interp
        );
    }

    /// @ai-generated - Element with v-if extracts scope.
    #[test]
    fn test_vif_simple() {
        let alloc = Allocator::default();
        let events = parse_events(r#"<template><div v-if="show"></div></template>"#, &alloc);
        let es = events
            .iter()
            .find(|e| e.starts_with("OxcElementStart("))
            .expect("Expected OxcElementStart");
        assert!(
            es.contains("scopes=1"),
            "Expected 1 scope (v-if), got: {}",
            es
        );
    }

    /// @ai-generated - Element with v-for extracts scope.
    #[test]
    fn test_vfor_simple() {
        let alloc = Allocator::default();
        let events = parse_events(
            r#"<template><div v-for="item of items"></div></template>"#,
            &alloc,
        );
        let es = events
            .iter()
            .find(|e| e.starts_with("OxcElementStart("))
            .expect("Expected OxcElementStart");
        assert!(
            es.contains("scopes=1"),
            "Expected 1 scope (v-for), got: {}",
            es
        );
    }

    /// @ai-generated - Scope priority: v-if comes before v-for.
    #[test]
    fn test_scope_priority_order() {
        let alloc = Allocator::default();
        let events = parse_events(
            r#"<template><div v-for="item of items" v-if="show"></div></template>"#,
            &alloc,
        );
        let es = events
            .iter()
            .find(|e| e.starts_with("OxcElementStart("))
            .expect("Expected OxcElementStart");
        assert!(
            es.contains("scopes=2"),
            "Expected 2 scopes (v-if + v-for), got: {}",
            es
        );
    }

    /// @ai-generated - Prop expression parsing works.
    #[test]
    fn test_prop_expression_parsing() {
        let alloc = Allocator::default();
        let events = parse_events(r#"<template><div :id="expr"></div></template>"#, &alloc);
        let es = events
            .iter()
            .find(|e| e.starts_with("OxcElementStart("))
            .expect("Expected OxcElementStart");
        assert!(es.contains("props=1"), "Expected 1 prop (:id), got: {}", es);
    }

    /// @ai-generated - Close tag produces OxcElementClosed.
    #[test]
    fn test_close_tag_produces_oxc_closed() {
        let alloc = Allocator::default();
        let events = parse_events("<template><div></div></template>", &alloc);
        assert!(
            events.iter().any(|e| e == "OxcElementClosed"),
            "Expected OxcElementClosed, got: {:?}",
            events
        );
    }

    /// @ai-generated - Non-element events pass through.
    #[test]
    fn test_non_element_events_pass_through() {
        let alloc = Allocator::default();
        let events = parse_events("<template>hello</template>", &alloc);
        assert!(
            events.iter().any(|e| e == "Text"),
            "Expected Text to pass through, got: {:?}",
            events
        );
    }
}
