use crate::{
    common::Span,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{CssStyleContent, StyleLang, SyntaxEvent, SyntaxProp, SyntaxTagType},
    },
};

/// Plugin that parses CSS style blocks and transforms them for Vue SFC compilation.
/// Following the same pattern as OxcParserPlugin.
pub struct CssParserPlugin {
    /// The element_id of the current style tag (for matching Prop events)
    style_element_id: Option<u32>,
    /// Collected props within the current style element
    style_element_props: Vec<SyntaxProp>,

    /// Whether the current style has the `scoped` attribute
    style_scoped: bool,
    /// Module name span if style has `module` attribute (None = not module)
    style_module: Option<Span>,
    /// Language for preprocessor (scss, less, etc.)
    style_lang: Option<StyleLang>,

    /// Style tag open start position
    style_tag_open_start: Option<u32>,
    /// Style tag open end position (after `>`)
    style_tag_open_end: Option<u32>,
}

impl CssParserPlugin {
    pub fn new() -> Self {
        Self {
            style_element_id: None,
            style_element_props: Vec::new(),
            style_scoped: false,
            style_module: None,
            style_lang: None,
            style_tag_open_start: None,
            style_tag_open_end: None,
        }
    }

    /// Reset state for a new style tag
    fn reset_style_state(&mut self) {
        self.style_element_id = None;
        self.style_element_props.clear();
        self.style_scoped = false;
        self.style_module = None;
        self.style_lang = None;
        self.style_tag_open_start = None;
        self.style_tag_open_end = None;
    }

    /// Check if a prop belongs to the current style element
    fn is_current_style_element(&self, element_id: u32) -> bool {
        self.style_element_id == Some(element_id)
    }

    /// Collect style attribute (scoped, module, lang)
    fn collect_style_attribute(&mut self, prop: &SyntaxProp, bytes: &[u8]) {
        let name = &bytes[prop.start as usize..prop.name_end as usize];

        match name {
            b"scoped" => {
                self.style_scoped = true;
            }
            b"module" => {
                // module without value = "$style", module="custom" = custom name
                if let Some(value) = &prop.value {
                    self.style_module = Some(Span::new(value.start, value.end));
                } else {
                    // Default module name "$style" - we'll handle this at output time
                    self.style_module = Some(Span::new(0, 0)); // Empty span = default
                }
            }
            b"lang" => {
                if let Some(value) = &prop.value {
                    let lang_value = &bytes[value.start as usize..value.end as usize];
                    self.style_lang = match lang_value {
                        b"css" => Some(StyleLang::Css),
                        b"scss" => Some(StyleLang::Scss),
                        b"sass" => Some(StyleLang::Sass),
                        b"less" => Some(StyleLang::Less),
                        b"stylus" | b"styl" => Some(StyleLang::Stylus),
                        _ => Some(StyleLang::Css), // Default to CSS for unknown
                    };
                }
            }
            _ => {}
        }

        // Store the prop for later
        self.style_element_props.push(prop.clone());
    }

    /// Parse style content and create CssStyleContent event
    fn parse_style_content(
        &mut self,
        close_tag_start: u32,
        close_tag_end: u32,
        element_id: u32,
        _ctx: &SyntaxPluginContext,
    ) -> CssStyleContent {
        let tag_open_start = self.style_tag_open_start.unwrap_or(0);
        let tag_open_end = self.style_tag_open_end.unwrap_or(0);

        let content_start = tag_open_end;
        let content_end = close_tag_start;

        // TODO: Parse CSS with lightningcss and extract v-bind expressions
        // For now, we just collect the metadata and pass through the content

        CssStyleContent {
            element_id,
            parent_id: 0,

            tag_open_start,
            tag_open_end,
            tag_close_start: close_tag_start,
            tag_close_end: close_tag_end,

            content_start,
            content_end,

            scoped: self.style_scoped,
            module: self.style_module,
            lang: self.style_lang,
            attributes: self.style_element_props.drain(..).collect(),

            // CSS parsing results (to be implemented)
            v_bind_expressions: Vec::new(),
            css_module_classes: Vec::new(),
        }
    }
}

impl Default for CssParserPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SyntaxPlugin<'a> for CssParserPlugin {
    fn name(&self) -> &str {
        "css_parser"
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match event {
            // Track root-level style tags
            SyntaxEvent::OpenTagStart(ref e) => {
                if e.nested_level == 0 && e.tag_type == SyntaxTagType::RootStyle {
                    self.reset_style_state();
                    self.style_element_id = Some(e.element_id);
                    self.style_tag_open_start = Some(e.start);
                }
                SyntaxResult::Keep(event)
            }

            // Track style tag end position
            SyntaxEvent::OpenTagEnd(ref e) => {
                if e.nested_level == 0 && e.tag_type == SyntaxTagType::RootStyle {
                    self.style_tag_open_end = Some(e.end);
                }
                SyntaxResult::Keep(event)
            }

            // Collect style attributes
            SyntaxEvent::Prop(ref e) => {
                if self.is_current_style_element(e.element_id) {
                    self.collect_style_attribute(e, ctx.bytes);
                    // Drop style props - they're internal metadata
                    return SyntaxResult::Drop;
                }
                SyntaxResult::Keep(event)
            }

            // Parse CSS on style closing tag
            SyntaxEvent::CloseTag(ref e) => {
                if e.nested_level == 0
                    && e.tag_type == SyntaxTagType::RootStyle
                    && self.style_element_id.is_some()
                {
                    let css_content = self.parse_style_content(e.start, e.end, e.element_id, ctx);

                    self.reset_style_state();

                    return SyntaxResult::Replace(SyntaxEvent::CssStyleContent(css_content));
                }
                SyntaxResult::Keep(event)
            }

            other => SyntaxResult::Keep(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax::syntax::Syntax;
    use crate::syntax::types::*;
    use crate::tokenizer::byte::tokenize;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Helper: process input through tokenizer -> syntax -> CssParserPlugin pipeline
    fn process_with_css_plugin(input: &str) -> Vec<CssStyleContent> {
        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext::new(input, input.as_bytes(), &options);

        let collected: Rc<RefCell<Vec<CssStyleContent>>> = Rc::new(RefCell::new(Vec::new()));

        struct CollectorPlugin {
            collected: Rc<RefCell<Vec<CssStyleContent>>>,
        }

        impl<'a> SyntaxPlugin<'a> for CollectorPlugin {
            fn name(&self) -> &str {
                "collector"
            }

            fn process_event(
                &mut self,
                event: SyntaxEvent<'a>,
                _ctx: &mut SyntaxPluginContext<'a>,
            ) -> SyntaxResult<SyntaxEvent<'a>> {
                if let SyntaxEvent::CssStyleContent(css) = event {
                    self.collected.borrow_mut().push(css);
                    return SyntaxResult::Drop;
                }
                SyntaxResult::Keep(event)
            }
        }

        let collected_clone = collected.clone();

        {
            let mut css_plugin = CssParserPlugin::new();
            let mut collector = CollectorPlugin {
                collected: collected_clone,
            };

            let mut syntax = Syntax::new(vec![&mut css_plugin, &mut collector]);
            syntax.start(&mut ctx);

            tokenize(input.as_bytes(), |e| {
                syntax.handle(&e, &mut ctx);
            });

            syntax.end(&mut ctx);
        } // collector and its Rc reference are dropped here

        Rc::try_unwrap(collected).unwrap().into_inner()
    }

    #[test]
    fn test_scoped_style_detection() {
        let input = r#"<template><div></div></template><style scoped>.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1, "Should have 1 style block");
        assert!(styles[0].scoped, "Style should be scoped");
        assert!(styles[0].module.is_none(), "Style should not be module");
        assert!(styles[0].lang.is_none(), "Style should have no lang");
    }

    #[test]
    fn test_module_style_detection() {
        let input = r#"<template><div></div></template><style module>.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);
        assert!(!styles[0].scoped, "Module style is not scoped by default");
        assert!(styles[0].module.is_some(), "Style should be module");
    }

    #[test]
    fn test_named_module_style() {
        let input = r#"<template><div></div></template><style module="custom">.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);
        assert!(styles[0].module.is_some(), "Style should be module");

        // Named module should have a span with content
        let module_span = styles[0].module.unwrap();
        assert!(module_span.start > 0 || module_span.end > 0);
    }

    #[test]
    fn test_scss_lang_detection() {
        let input =
            r#"<template><div></div></template><style lang="scss">.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].lang, Some(StyleLang::Scss));
    }

    #[test]
    fn test_less_lang_detection() {
        let input =
            r#"<template><div></div></template><style lang="less">.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].lang, Some(StyleLang::Less));
    }

    #[test]
    fn test_scoped_scss_style() {
        let input = r#"<template><div></div></template><style scoped lang="scss">.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);
        assert!(styles[0].scoped);
        assert_eq!(styles[0].lang, Some(StyleLang::Scss));
    }

    #[test]
    fn test_style_content_positions() {
        let input = r#"<style scoped>.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);

        let style = &styles[0];
        assert_eq!(style.tag_open_start, 0, "Tag should start at 0");

        // Content should be between > and </style>
        let content = &input[style.content_start as usize..style.content_end as usize];
        assert_eq!(content, ".box { color: red; }");
    }

    #[test]
    fn test_multiple_style_blocks() {
        let input =
            r#"<template><div></div></template><style scoped>.a {}</style><style>.b {}</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 2, "Should have 2 style blocks");
        assert!(styles[0].scoped, "First style should be scoped");
        assert!(!styles[1].scoped, "Second style should not be scoped");
    }

    #[test]
    fn test_plain_style() {
        let input = r#"<template><div></div></template><style>.box { color: red; }</style>"#;
        let styles = process_with_css_plugin(input);

        assert_eq!(styles.len(), 1);
        assert!(!styles[0].scoped);
        assert!(styles[0].module.is_none());
        assert!(styles[0].lang.is_none());
    }
}
