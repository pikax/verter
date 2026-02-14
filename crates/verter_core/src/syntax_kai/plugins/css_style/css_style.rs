use crate::{
    common::Span,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{
            CssModuleClassMapping, CssModuleInfo, CssParsedStyleBlock, Event, ProcessedCssVBind,
            ProcessedStyleBlock, StyleLang,
        },
    },
};

/// Compiled CSS output for a single `<style>` block.
///
/// Produced by [`CssStylePlugin`] after processing scoped selectors, v-bind
/// replacement, and CSS module hashing.
#[derive(Debug, Clone)]
pub struct CssStyleOutput {
    /// Compiled CSS code. For transformed blocks this includes scoped selectors,
    /// v-bind replacements, and module-hashed class names. For plain unscoped
    /// blocks this is the original CSS content.
    pub code: String,
    /// Whether this block has the `scoped` attribute.
    pub scoped: bool,
    /// Style language (css, scss, less, stylus).
    pub lang: Option<StyleLang>,
    /// CSS module info (class name mappings). None if not a module block.
    pub module: Option<CssModuleInfo>,
    /// CSS processing errors (e.g., lightningcss parse failures).
    pub errors: Vec<String>,
}

/// CSS style plugin for the syntax_kai pipeline.
///
/// Consumes `CssParsedStyle` events (from css_parser) and applies transformations:
/// - **Scoped CSS**: Inserts `[data-v-{scope_id}]` attribute selectors
/// - **CSS v-bind()**: Replaces `v-bind(expr)` with `var(--{scope_id}-{sanitized})`
/// - **CSS Modules**: Hashes class names and builds runtime mappings
///
/// Uses lightningcss for CSS parsing, normalization, and transformation.
pub struct CssStylePlugin {
    /// Scope ID for scoped styles, pre-computed by builder from component name hash.
    scope_id: Option<[u8; 8]>,
    /// Component ID for CSS module class hashing.
    component_id: Option<[u8; 8]>,
    /// Collected CSS outputs for all processed style blocks.
    styles: Vec<CssStyleOutput>,
}

impl Default for CssStylePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CssStylePlugin {
    pub fn new() -> Self {
        Self {
            scope_id: None,
            component_id: None,
            styles: Vec::new(),
        }
    }

    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    pub fn set_component_id(&mut self, component_id: [u8; 8]) {
        self.component_id = Some(component_id);
    }

    /// Take the collected CSS outputs, leaving the internal buffer empty.
    pub fn take_styles(&mut self) -> Vec<CssStyleOutput> {
        std::mem::take(&mut self.styles)
    }

    fn process_parsed_style(
        &mut self,
        parsed: CssParsedStyleBlock,
        ctx: &SyntaxPluginContext,
    ) -> ProcessedStyleBlock {
        let content = parsed.content;
        let mut transformed_css: Option<Vec<u8>> = None;
        let mut v_bind_expressions: Vec<ProcessedCssVBind> = Vec::new();
        let mut module_info: Option<CssModuleInfo> = None;
        let mut errors: Vec<String> = Vec::new();

        let is_module = parsed.module.is_some();
        let needs_transform =
            (parsed.scoped && self.scope_id.is_some()) || !parsed.v_binds.is_empty() || is_module;

        if let Some(content_span) = content {
            if needs_transform {
                let css_str = &ctx.input[content_span.start as usize..content_span.end as usize];

                // Get scope ID as &str — falls back to component_id or "00000000"
                let scope_id_bytes = self.scope_id.or(self.component_id).unwrap_or([b'0'; 8]);
                let scope_id_str = std::str::from_utf8(&scope_id_bytes).unwrap_or("00000000");

                let options = crate::css::ProcessStyleOptions {
                    scope_id: scope_id_str,
                    scoped: parsed.scoped,
                    is_module,
                    filename: None,
                    sourcemap: false,
                };

                match crate::css::process_style(css_str, &options) {
                    Ok(result) => {
                        transformed_css = Some(result.code.into_bytes());

                        // Map v_bind_vars to ProcessedCssVBind
                        for vb in &result.v_bind_vars {
                            v_bind_expressions.push(ProcessedCssVBind {
                                expression: Span { start: 0, end: 0 },
                                var_name: vb.var_name.clone(),
                                css_start: 0,
                                css_end: 0,
                            });
                        }

                        // Map module_classes to CssModuleInfo
                        if is_module {
                            let custom_name = parsed.module.and_then(|span| {
                                if span.start == 0 && span.end == 0 {
                                    None
                                } else {
                                    Some(span)
                                }
                            });
                            module_info = Some(CssModuleInfo {
                                custom_name,
                                classes: result
                                    .module_classes
                                    .iter()
                                    .map(|(_orig, hashed)| CssModuleClassMapping {
                                        original: Span { start: 0, end: 0 },
                                        hashed: hashed.clone(),
                                    })
                                    .collect(),
                            });
                        }
                    }
                    Err(e) => {
                        errors.push(e);
                    }
                }
            }
        }

        ProcessedStyleBlock {
            lang: parsed.lang,
            scoped: parsed.scoped,
            module: module_info,
            transformed_css,
            v_bind_expressions,
            errors,
            compiled_start: parsed.compiled_start,
            compiled_end: parsed.compiled_end,
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for CssStylePlugin {
    fn name(&self) -> &str {
        "css_style"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::CssParsedStyle(parsed) => {
                let content_span = parsed.content;
                let lang = parsed.lang;
                let processed = self.process_parsed_style(*parsed, ctx);

                // Collect CSS output: use transformed CSS if available, else original content
                let code = if let Some(ref css) = processed.transformed_css {
                    String::from_utf8_lossy(css).to_string()
                } else if let Some(span) = content_span {
                    ctx.input[span.start as usize..span.end as usize].to_string()
                } else {
                    String::new()
                };

                self.styles.push(CssStyleOutput {
                    code,
                    scoped: processed.scoped,
                    lang,
                    module: processed.module.clone(),
                    errors: processed.errors.clone(),
                });

                SyntaxResult::Replace(Event::ProcessedStyle(Box::new(processed)))
            }
            other => SyntaxResult::Keep(other),
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::css_parser::css_parser::CssParserPlugin;
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;

    /// Run input through tokenizer → syntax → element_compiler → css_parser → css_style pipeline.
    fn process_style_events(
        input: &str,
        scope_id: Option<[u8; 8]>,
        component_id: Option<[u8; 8]>,
    ) -> Vec<String> {
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

        // element_compiler
        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // css_parser
        let mut parser = CssParserPlugin::new();
        let mut parsed = Vec::new();
        for event in compiled {
            match parser.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => parsed.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // css_style
        let mut css = CssStylePlugin::new();
        if let Some(sid) = scope_id {
            css.set_scope_id(sid);
        }
        if let Some(cid) = component_id {
            css.set_component_id(cid);
        }

        let mut result = Vec::new();
        for event in parsed {
            match css.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => result.push(e),
                SyntaxResult::Drop => {}
            }
        }

        result
            .iter()
            .map(|e| match e {
                Event::ProcessedStyle(ps) => {
                    let css_str = ps
                        .transformed_css
                        .as_ref()
                        .map(|c| String::from_utf8_lossy(c).to_string())
                        .unwrap_or_else(|| "None".to_string());
                    format!(
                        "ProcessedStyle(scoped={}, module={}, vbinds={}, css={})",
                        ps.scoped,
                        ps.module.is_some(),
                        ps.v_bind_expressions.len(),
                        css_str
                    )
                }
                Event::Text(_) => "Text".to_string(),
                _ => format!("{:?}", std::mem::discriminant(e)),
            })
            .collect()
    }

    #[test]
    fn test_plain_style_produces_processed_style() {
        let events = process_style_events("<style>.box { color: red; }</style>", None, None);
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .expect("Expected ProcessedStyle");
        assert!(ps.contains("scoped=false"));
        assert!(ps.contains("css=None"));
    }

    #[test]
    fn test_non_style_events_pass_through() {
        let events = process_style_events("<template>hello</template>", None, None);
        assert!(events.iter().any(|e| e == "Text"));
    }

    #[test]
    fn test_scoped_class_selector() {
        let events = process_style_events(
            "<style scoped>.box { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("[data-v-a1b2c3d4]"), "got: {}", ps);
    }

    #[test]
    fn test_scoped_element_selector() {
        let events = process_style_events(
            "<style scoped>div { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("div[data-v-a1b2c3d4]"), "got: {}", ps);
    }

    #[test]
    fn test_scoped_pseudo_class_ordering() {
        let events = process_style_events(
            "<style scoped>.btn:hover { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains(".btn[data-v-a1b2c3d4]:hover"), "got: {}", ps);
    }

    #[test]
    fn test_scoped_pseudo_element_ordering() {
        let events = process_style_events(
            "<style scoped>.text::before { content: ''; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        // lightningcss may normalize ::before to :before
        assert!(
            ps.contains(".text[data-v-a1b2c3d4]::before")
                || ps.contains(".text[data-v-a1b2c3d4]:before"),
            "got: {}",
            ps
        );
    }

    #[test]
    fn test_no_scope_id_no_transform() {
        let events = process_style_events("<style scoped>.box { color: red; }</style>", None, None);
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("css=None"), "got: {}", ps);
    }

    #[test]
    fn test_v_bind_simple() {
        let events = process_style_events(
            "<style scoped>.box { color: v-bind(color); }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("vbinds=1"), "got: {}", ps);
        assert!(ps.contains("var(--a1b2c3d4-color)"), "got: {}", ps);
    }

    #[test]
    fn test_v_bind_dotted() {
        let events = process_style_events(
            "<style scoped>.box { color: v-bind('theme.color'); }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("var(--a1b2c3d4-theme_color)"), "got: {}", ps);
    }

    #[test]
    fn test_module_default_name() {
        let events = process_style_events(
            "<style module>.btn { color: red; }</style>",
            None,
            Some(*b"comp1234"),
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("module=true"), "got: {}", ps);
    }

    #[test]
    fn test_module_class_hashing() {
        let events = process_style_events(
            "<style module>.btn { color: red; }</style>",
            None,
            Some(*b"comp1234"),
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains(".btn_comp1234_"), "got: {}", ps);
    }

    #[test]
    fn test_empty_style_block() {
        let events = process_style_events("<style scoped></style>", Some(*b"a1b2c3d4"), None);
        assert!(
            events.iter().any(|e| e.starts_with("ProcessedStyle(")),
            "{:?}",
            events
        );
    }

    #[test]
    fn test_scoped_selector_list() {
        let events = process_style_events(
            "<style scoped>.a, .b { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        let scope_count = ps.matches("[data-v-a1b2c3d4]").count();
        assert!(
            scope_count >= 2,
            "Both selectors should be scoped, found {} in: {}",
            scope_count,
            ps
        );
    }
}
