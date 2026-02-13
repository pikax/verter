use crate::syntax_kai::{
    plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
    types::{CompiledRootStyleStart, CssParsedStyleBlock, Event},
};

/// CSS Parser Plugin for the syntax_kai pipeline.
///
/// Processes `CompiledStyleStart`/`CompiledStyleEnd` events to parse CSS content
/// and extract structural information (selectors, v-bind expressions, class names).
///
/// Follows the same pattern as `OxcParserPlugin`:
/// - Buffers `CompiledStyleStart` on receipt
/// - On `CompiledStyleEnd`, parses the CSS content and emits `CssParsedStyle`
///
/// Dispatches to the correct per-language scanner (CSS, SCSS, Less, Stylus)
/// via [`crate::utils::css::scan_style`].
pub struct CssParserPlugin {
    /// Buffered CompiledStyleStart (set on Start, consumed on End).
    current_start: Option<CompiledRootStyleStart>,
}

impl CssParserPlugin {
    pub fn new() -> Self {
        Self {
            current_start: None,
        }
    }
}

impl Default for CssParserPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for CssParserPlugin {
    fn name(&self) -> &str {
        "css_parser"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::CompiledStyleStart(start) => {
                self.current_start = Some(start);
                SyntaxResult::Drop
            }
            Event::CompiledStyleEnd(end) => {
                if let Some(start) = self.current_start.take() {
                    let content = end.content;
                    let mut rules = Vec::new();
                    let mut all_v_binds = Vec::new();
                    let mut all_classes = Vec::new();

                    if let Some(content_span) = content {
                        let css =
                            &ctx.bytes[content_span.start as usize..content_span.end as usize];
                        let offset = content_span.start;

                        crate::utils::css::scan_style(
                            start.lang,
                            css,
                            offset,
                            &mut rules,
                            &mut all_v_binds,
                            &mut all_classes,
                        );
                    }

                    let parsed = CssParsedStyleBlock {
                        lang: start.lang,
                        scoped: start.scoped,
                        module: start.module,
                        content,
                        rules,
                        v_binds: all_v_binds,
                        classes: all_classes,
                        compiled_start: start,
                        compiled_end: end,
                    };

                    SyntaxResult::Replace(Event::CssParsedStyle(Box::new(parsed)))
                } else {
                    SyntaxResult::Keep(Event::CompiledStyleEnd(end))
                }
            }
            other => SyntaxResult::Keep(other),
        }
    }
}

// =============================================================================
// Plugin integration tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::syntax_kai::types::CssParsedSpecialPseudoKind;
    use crate::tokenizer::byte::tokenize;

    /// Run input through tokenizer → syntax → element_compiler → css_parser pipeline.
    fn parse_css_events(input: &str) -> Vec<CssParsedStyleBlock> {
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

        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut parser = CssParserPlugin::new();
        let mut result_events = Vec::new();
        for event in compiled {
            match parser.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => result_events.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut parsed_blocks = Vec::new();
        for event in result_events {
            if let Event::CssParsedStyle(block) = event {
                parsed_blocks.push(*block);
            }
        }
        parsed_blocks
    }

    // --- Basic pipeline integration ---

    #[test]
    fn test_plain_style_produces_parsed_event() {
        let blocks = parse_css_events("<style>.box { color: red; }</style>");
        assert_eq!(blocks.len(), 1);
        assert!(!blocks[0].scoped);
        assert!(blocks[0].module.is_none());
    }

    #[test]
    fn test_scoped_flag_preserved() {
        let blocks = parse_css_events("<style scoped>.box { color: red; }</style>");
        assert!(blocks[0].scoped);
    }

    #[test]
    fn test_module_flag_preserved() {
        let blocks = parse_css_events("<style module>.box { color: red; }</style>");
        assert!(blocks[0].module.is_some());
    }

    #[test]
    fn test_content_span_preserved() {
        let input = "<style>.box { color: red; }</style>";
        let blocks = parse_css_events(input);
        let content = blocks[0].content.unwrap();
        let css = &input[content.start as usize..content.end as usize];
        assert_eq!(css, ".box { color: red; }");
    }

    #[test]
    fn test_multiple_style_blocks() {
        let blocks = parse_css_events(
            "<style scoped>.a { color: red; }</style><style>.b { color: blue; }</style>",
        );
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].scoped);
        assert!(!blocks[1].scoped);
    }

    #[test]
    fn test_non_style_events_pass_through() {
        let input = "<template>hello</template>";
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

        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut parser = CssParserPlugin::new();
        let mut result = Vec::new();
        for event in compiled {
            match parser.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => result.push(e),
                SyntaxResult::Drop => {}
            }
        }

        assert!(result.iter().any(|e| matches!(e, Event::Text(_))));
    }

    // --- Rule extraction via pipeline ---

    #[test]
    fn test_rule_extracted() {
        let blocks = parse_css_events("<style>.box { color: red; }</style>");
        assert_eq!(blocks[0].rules.len(), 1);
    }

    #[test]
    fn test_selector_span_correct() {
        let input = "<style>.box { color: red; }</style>";
        let blocks = parse_css_events(input);
        let sel = &blocks[0].rules[0].selectors[0];
        let sel_text = &input[sel.span.start as usize..sel.span.end as usize];
        assert_eq!(sel_text, ".box");
    }

    #[test]
    fn test_v_bind_extracted() {
        let input = "<style>.box { color: v-bind(color); }</style>";
        let blocks = parse_css_events(input);
        assert_eq!(blocks[0].v_binds.len(), 1);
        let vb = &blocks[0].v_binds[0];
        let full = &input[vb.full_span.start as usize..vb.full_span.end as usize];
        assert_eq!(full, "v-bind(color)");
    }

    #[test]
    fn test_deep_pseudo_via_pipeline() {
        let input = "<style>:deep(.inner) { color: red; }</style>";
        let blocks = parse_css_events(input);
        let sel = &blocks[0].rules[0].selectors[0];
        assert_eq!(sel.specials.len(), 1);
        assert_eq!(sel.specials[0].kind, CssParsedSpecialPseudoKind::Deep);
    }

    #[test]
    fn test_empty_style() {
        let blocks = parse_css_events("<style></style>");
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].rules.is_empty());
    }
}
