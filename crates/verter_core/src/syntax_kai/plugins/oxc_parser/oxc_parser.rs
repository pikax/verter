use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::syntax_kai::{
    plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
    plugins::oxc_parser::{
        interpolation::parse_interpolation, props::parse_element_props, script::parse_script,
    },
    types::*,
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

    stack_provided_bindings: Vec<Vec<&'alloc str>>,
}

impl<'alloc> OxcParserPlugin<'alloc> {
    pub fn new(alloc: &'alloc Allocator) -> Self {
        Self {
            source_type: SourceType::mjs(),
            alloc,
            current_script_start: None,
            stack_provided_bindings: Vec::new(),
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
            Event::Lang(ev) => {
                self.source_type = ev.lang.to_source_type();
                SyntaxResult::Keep(Event::Lang(ev))
            }
            Event::ElementStart(compiled) => {
                let current_bindings: &[&str] = self
                    .stack_provided_bindings
                    .last()
                    .map_or(&[], |v| v.as_slice());

                let oxc_compiled = parse_element_props(
                    compiled,
                    ctx.input,
                    self.alloc,
                    self.source_type,
                    current_bindings,
                );

                self.stack_provided_bindings
                    .push(oxc_compiled.provided_locals.clone());

                SyntaxResult::Replace(Event::OxcCompiledElementStart(oxc_compiled))
            }

            Event::ElementClosed(closed) => {
                self.stack_provided_bindings.pop();
                let oxc_closed = OxcCompiledElementClosed { event: closed };
                SyntaxResult::Replace(Event::OxcCompiledElementClosed(oxc_closed))
            }

            Event::Interpolation(interp) => {
                let current_bindings: &[&str] = self
                    .stack_provided_bindings
                    .last()
                    .map_or(&[], |v| v.as_slice());

                let oxc_interp = parse_interpolation(
                    interp,
                    ctx.input,
                    self.alloc,
                    self.source_type,
                    current_bindings,
                );
                SyntaxResult::Replace(Event::OxcInterpolation(oxc_interp))
            }

            Event::CompiledScriptStart(start) => {
                // Buffer script start, wait for end
                self.current_script_start = Some(start);
                SyntaxResult::Drop
            }

            Event::CompiledScriptEnd(end) => {
                if let Some(start) = self.current_script_start.take() {
                    let script = parse_script(start, end, ctx.input, self.alloc, self.source_type);
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

        let mut syntax = Syntax::new(false);
        for event in &tokenizer_events {
            syntax.handle(event, &mut ctx);
        }
        let events_storage = syntax.events();

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
