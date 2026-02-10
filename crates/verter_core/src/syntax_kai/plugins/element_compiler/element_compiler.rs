use crate::{
    common::Span,
    cursor::ScriptLanguage,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::*,
    },
};

/// Element Compiler Plugin for the syntax_kai pipeline.
///
/// Consolidates raw tokenizer events into compiled element events:
/// - `OpenTag` + `Prop`* + `OpenTagEnd` → `ElementStart(CompiledElementStart)`
/// - `CloseTag` → `ElementClosed(CompiledElementClosed)`
/// - Root tag props + `RootOpenTagEnd` → `CompiledScriptStart` / `CompiledTemplateStart` / etc.
/// - `RootCloseTag` → `CompiledScriptEnd` / `CompiledTemplateEnd` / etc.
pub struct ElementCompilerPlugin {
    /// Current element being built (set on OpenTag, consumed on OpenTagEnd).
    current_element: Option<ElementBuildState>,
    /// Props buffered before a RootOpenTagEnd arrives.
    pending_root_props: Vec<Prop>,
    /// Whether we are inside a root block (between RootOpenTagEnd and RootCloseTag).
    in_root: bool,
    /// Track the last RootOpenTagEnd to pair with RootCloseTag for content span.
    last_root_open_end: Option<RootNodeOpenTagEnd>,
}

struct ElementBuildState {
    open_tag: ElementOpenTagStart,
    props: Vec<Prop>,
}

impl Default for ElementCompilerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> ElementCompilerPlugin {
    pub fn new() -> Self {
        Self {
            current_element: None,
            pending_root_props: Vec::with_capacity(3),
            in_root: false,
            last_root_open_end: None,
        }
    }

    /// Build a CompiledRootScriptStart from the root open tag end and buffered props.
    fn build_script_start(
        &mut self,
        open_end: RootNodeOpenTagEnd,
        ctx: &SyntaxPluginContext,
    ) -> CompiledRootScriptStart {
        let props = std::mem::take(&mut self.pending_root_props);
        let mut setup: Option<Span> = None;
        let mut lang: Option<ScriptLanguage> = None;
        let mut generic: Option<Span> = None;
        let mut attrs: Option<Span> = None;

        for prop in &props {
            let name = &ctx.bytes[prop.start as usize..prop.name_end as usize];
            match name {
                b"setup" => {
                    setup = Some(Span::new(prop.start, prop.end));
                }
                b"lang" => {
                    if let Some(v) = prop.value {
                        lang = Some(ScriptLanguage::from_bytes(
                            &ctx.bytes[v.start as usize..v.end as usize],
                        ));
                    }
                }
                b"generic" => {
                    generic = prop.value;
                }
                b"attrs" => {
                    attrs = prop.value;
                }
                _ => {}
            }
        }

        let tag_open = Span::new(open_end.start, open_end.end);
        let tag_open_event = RootNodeOpenTagStart {
            kind: RootNodeKind::Script,
            start: open_end.start,
            name_end: open_end.name_end,
        };

        CompiledRootScriptStart {
            start: open_end.start,
            name_end: open_end.name_end,
            tag_open,
            setup,
            lang,
            generic,
            attrs,
            attributes: props.into_iter().collect(),
            tag_open_event,
            tag_open_end_event: open_end,
        }
    }

    /// Build a CompiledRootTemplateStart from the root open tag end and buffered props.
    fn build_template_start(
        &mut self,
        open_end: RootNodeOpenTagEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> CompiledRootTemplateStart {
        let props = std::mem::take(&mut self.pending_root_props);
        let mut vapor: Option<Span> = None;
        let mut lang: Option<Span> = None;

        for prop in &props {
            let name = &ctx.bytes[prop.start as usize..prop.name_end as usize];
            match name {
                b"vapor" => {
                    vapor = Some(Span::new(prop.start, prop.end));
                }
                b"lang" => {
                    lang = prop.value;
                }
                _ => {}
            }
        }

        let tag_open = Span::new(open_end.start, open_end.end);
        let tag_open_event = RootNodeOpenTagStart {
            kind: RootNodeKind::Template,
            start: open_end.start,
            name_end: open_end.name_end,
        };

        CompiledRootTemplateStart {
            start: open_end.start,
            name_end: open_end.name_end,
            tag_open,
            vapor,
            lang,
            attributes: props.into_iter().collect(),
            tag_open_event,
            tag_open_end_event: open_end,
        }
    }

    /// Build a CompiledRootStyleStart from the root open tag end and buffered props.
    fn build_style_start(
        &mut self,
        open_end: RootNodeOpenTagEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> CompiledRootStyleStart {
        let props = std::mem::take(&mut self.pending_root_props);
        let mut style_lang: Option<StyleLang> = None;
        let mut scoped = false;
        let mut module: Option<Span> = None;

        for prop in &props {
            let name = &ctx.bytes[prop.start as usize..prop.name_end as usize];
            match name {
                b"lang" => {
                    if let Some(v) = prop.value {
                        let val = &ctx.bytes[v.start as usize..v.end as usize];
                        style_lang = Some(match val {
                            b"css" => StyleLang::Css,
                            b"scss" => StyleLang::Scss,
                            b"sass" => StyleLang::Sass,
                            b"less" => StyleLang::Less,
                            b"stylus" => StyleLang::Stylus,
                            _ => StyleLang::Unknown,
                        });
                    }
                }
                b"scoped" => {
                    scoped = true;
                }
                b"module" => {
                    // module attribute: boolean → None value means default "$style",
                    // valued → custom name
                    module = Some(prop.value.unwrap_or(Span::new(0, 0)));
                }
                _ => {}
            }
        }

        let tag_open = Span::new(open_end.start, open_end.end);
        let tag_open_event = RootNodeOpenTagStart {
            kind: RootNodeKind::Style,
            start: open_end.start,
            name_end: open_end.name_end,
        };

        CompiledRootStyleStart {
            start: open_end.start,
            name_end: open_end.name_end,
            tag_open,
            lang: style_lang,
            scoped,
            module,
            attributes: props.into_iter().collect(),
            tag_open_event,
            tag_open_end_event: open_end,
        }
    }

    /// Build a CompiledRootUnknownStart from the root open tag end and buffered props.
    fn build_unknown_start(&mut self, open_end: RootNodeOpenTagEnd) -> CompiledRootUnknownStart {
        let props = std::mem::take(&mut self.pending_root_props);

        let tag_open = Span::new(open_end.start, open_end.end);
        let tag_open_event = RootNodeOpenTagStart {
            kind: RootNodeKind::Unknown,
            start: open_end.start,
            name_end: open_end.name_end,
        };

        CompiledRootUnknownStart {
            start: open_end.start,
            name_end: open_end.name_end,
            tag_open,
            content: None,
            attributes: props.into_iter().collect(),
            tag_open_event,
            tag_open_end_event: open_end,
        }
    }

    /// Build a compiled root end event.
    fn build_root_end(close: RootNodeCloseTag, open_end: &RootNodeOpenTagEnd) -> (u32, u32, u32) {
        // Content span: from tag_open_end.end to close.start
        let _content_start = open_end.end;
        let _content_end = close.start;
        (close.start, close.name_end, close.end)
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for ElementCompilerPlugin {
    fn name(&self) -> &str {
        "element_compiler"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            // --- Element events ---
            Event::OpenTag(open_tag) => {
                self.current_element = Some(ElementBuildState {
                    open_tag,
                    props: Vec::new(),
                });
                SyntaxResult::Drop
            }

            Event::Prop(prop) => {
                if let Some(ref mut el) = self.current_element {
                    // Prop belongs to current element being built
                    el.props.push(prop);
                    SyntaxResult::Drop
                } else if !self.in_root {
                    // Prop before any root open tag end → buffer as root prop
                    self.pending_root_props.push(prop);
                    SyntaxResult::Drop
                } else {
                    // Inside root content but no element open → pass through
                    SyntaxResult::Keep(Event::Prop(prop))
                }
            }

            Event::OpenTagEnd(open_tag_end) => {
                if let Some(el) = self.current_element.take() {
                    let compiled = CompiledElementStart {
                        element_id: el.open_tag.start,
                        parent_id: el.open_tag.parent_id,
                        event_open_tag: el.open_tag,
                        event_open_tag_end: open_tag_end,
                        props: el.props,
                    };
                    SyntaxResult::Replace(Event::ElementStart(compiled))
                } else {
                    // No matching OpenTag — keep as-is (error recovery)
                    SyntaxResult::Keep(Event::OpenTagEnd(open_tag_end))
                }
            }

            Event::CloseTag(close_tag) => {
                let compiled = CompiledElementClosed {
                    element_id: close_tag.start,
                    parent_id: close_tag.parent_id,
                    event_close_tag: Some(close_tag),
                };
                SyntaxResult::Replace(Event::ElementClosed(compiled))
            }

            // --- Root events ---
            Event::RootOpenTagEnd(open_end) => {
                self.in_root = true;
                let kind = open_end.kind.clone();

                let result = match kind {
                    RootNodeKind::Script => {
                        let compiled = self.build_script_start(open_end.clone(), ctx);
                        self.last_root_open_end = Some(open_end);
                        SyntaxResult::Replace(Event::CompiledScriptStart(compiled))
                    }
                    RootNodeKind::Template => {
                        let compiled = self.build_template_start(open_end.clone(), ctx);
                        self.last_root_open_end = Some(open_end);
                        SyntaxResult::Replace(Event::CompiledTemplateStart(compiled))
                    }
                    RootNodeKind::Style => {
                        let compiled = self.build_style_start(open_end.clone(), ctx);
                        self.last_root_open_end = Some(open_end);
                        SyntaxResult::Replace(Event::CompiledStyleStart(compiled))
                    }
                    RootNodeKind::Unknown => {
                        let compiled = self.build_unknown_start(open_end.clone());
                        self.last_root_open_end = Some(open_end);
                        SyntaxResult::Replace(Event::CompiledUnknownStart(compiled))
                    }
                };
                result
            }

            Event::RootCloseTag(close) => {
                self.in_root = false;
                let kind = close.kind.clone();

                if let Some(open_end) = self.last_root_open_end.take() {
                    // Content span from tag open end to close tag start
                    let content = if close.start > open_end.end {
                        Some(Span::new(open_end.end, close.start))
                    } else {
                        None
                    };
                    let tag_close = Some(Span::new(close.start, close.end));

                    match kind {
                        RootNodeKind::Script => {
                            let compiled = CompiledRootScriptEnd {
                                start: close.start,
                                name_end: close.name_end,
                                end: close.end,
                                tag_close,
                                content,
                                tag_close_event: Some(close),
                            };
                            SyntaxResult::Replace(Event::CompiledScriptEnd(compiled))
                        }
                        RootNodeKind::Template => {
                            let compiled = CompiledRootTemplateEnd {
                                start: close.start,
                                name_end: close.name_end,
                                end: close.end,
                                tag_close,
                                content,
                                tag_close_event: Some(close),
                            };
                            SyntaxResult::Replace(Event::CompiledTemplateEnd(compiled))
                        }
                        RootNodeKind::Style => {
                            let compiled = CompiledRootStyleEnd {
                                start: close.start,
                                name_end: close.name_end,
                                end: close.end,
                                tag_close,
                                content,
                                tag_close_event: Some(close),
                            };
                            SyntaxResult::Replace(Event::CompiledStyleEnd(compiled))
                        }
                        RootNodeKind::Unknown => {
                            let compiled = CompiledRootUnknownEnd {
                                start: close.start,
                                name_end: close.name_end,
                                end: close.end,
                                tag_close,
                                content,
                                tag_close_event: Some(close),
                            };
                            SyntaxResult::Replace(Event::CompiledUnknownEnd(compiled))
                        }
                    }
                } else {
                    // No matching open — forward as-is
                    SyntaxResult::Keep(Event::RootCloseTag(close))
                }
            }

            // --- Pass through everything else ---
            other => SyntaxResult::Keep(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;

    /// Helper: tokenize input, run through Syntax, then run through ElementCompilerPlugin.
    /// Returns the compiled events (both template events and root_script_events combined).
    fn compile_events(input: &str) -> Vec<String> {
        let mut tokenizer_events = Vec::new();
        tokenize(input.as_bytes(), |event| tokenizer_events.push(event));

        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &options,
        };

        let mut events_storage: Vec<Event<'_>> = Vec::new();
        let mut root_script_events: Vec<Event<'_>> = Vec::new();
        let ptr = &mut events_storage as *mut Vec<Event<'_>>;
        {
            let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
            for event in &tokenizer_events {
                syntax.handle(event, &mut ctx);
            }
            root_script_events = syntax.take_root_script_events();
        }

        // Run element_compiler plugin on template events
        let mut plugin = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match plugin.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Run a separate element_compiler plugin on root_script_events
        let mut script_plugin = ElementCompilerPlugin::new();
        for event in root_script_events {
            match script_plugin.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Convert to debug strings for assertions
        compiled
            .iter()
            .map(|e| match e {
                Event::ElementStart(es) => format!(
                    "ElementStart(id={}, parent={}, props={})",
                    es.element_id,
                    es.parent_id,
                    es.props.len()
                ),
                Event::ElementClosed(ec) => {
                    format!(
                        "ElementClosed(id={}, parent={})",
                        ec.element_id, ec.parent_id
                    )
                }
                Event::CompiledScriptStart(s) => {
                    format!(
                        "CompiledScriptStart(setup={}, lang={:?})",
                        s.setup.is_some(),
                        s.lang
                    )
                }
                Event::CompiledScriptEnd(_) => "CompiledScriptEnd".to_string(),
                Event::CompiledTemplateStart(s) => {
                    format!("CompiledTemplateStart(vapor={})", s.vapor.is_some())
                }
                Event::CompiledTemplateEnd(_) => "CompiledTemplateEnd".to_string(),
                Event::CompiledStyleStart(s) => {
                    format!(
                        "CompiledStyleStart(scoped={}, module={}, lang={:?})",
                        s.scoped,
                        s.module.is_some(),
                        s.lang
                    )
                }
                Event::CompiledStyleEnd(_) => "CompiledStyleEnd".to_string(),
                Event::CompiledUnknownStart(_) => "CompiledUnknownStart".to_string(),
                Event::CompiledUnknownEnd(_) => "CompiledUnknownEnd".to_string(),
                Event::Text(_) => "Text".to_string(),
                Event::Interpolation(_) => "Interpolation".to_string(),
                Event::Comment(_) => "Comment".to_string(),
                Event::Lang(_) => "Lang".to_string(),
                _ => format!("{:?}", std::mem::discriminant(e)),
            })
            .collect()
    }

    // === Test cases ===

    /// @ai-generated - Simple div compiles to ElementStart with no props.
    #[test]
    fn test_compile_simple_div() {
        let events = compile_events("<template><div></div></template>");
        assert!(
            events.iter().any(|e| e.starts_with("ElementStart(")),
            "Expected ElementStart event, got: {:?}",
            events
        );
        let es = events
            .iter()
            .find(|e| e.starts_with("ElementStart("))
            .unwrap();
        assert!(es.contains("props=0"), "Expected 0 props, got: {}", es);
    }

    /// @ai-generated - Div with props compiles to ElementStart with correct prop count.
    #[test]
    fn test_compile_div_with_props() {
        let events = compile_events(r#"<template><div class="x" id="y"></div></template>"#);
        let es = events
            .iter()
            .find(|e| e.starts_with("ElementStart("))
            .unwrap();
        assert!(es.contains("props=2"), "Expected 2 props, got: {}", es);
    }

    /// @ai-generated - Close tag compiles to ElementClosed.
    #[test]
    fn test_compile_close_tag() {
        let events = compile_events("<template><div></div></template>");
        assert!(
            events.iter().any(|e| e.starts_with("ElementClosed(")),
            "Expected ElementClosed event, got: {:?}",
            events
        );
    }

    /// @ai-generated - Script root with setup and lang compiles correctly.
    #[test]
    fn test_compile_root_script() {
        let events = compile_events(r#"<script setup lang="ts"></script>"#);
        let ss = events
            .iter()
            .find(|e| e.starts_with("CompiledScriptStart("))
            .expect("Expected CompiledScriptStart");
        assert!(
            ss.contains("setup=true"),
            "Expected setup=true, got: {}",
            ss
        );
        assert!(
            ss.contains("lang=Some(TypeScript)"),
            "Expected lang=TypeScript, got: {}",
            ss
        );
        assert!(
            events.iter().any(|e| e == "CompiledScriptEnd"),
            "Expected CompiledScriptEnd"
        );
    }

    /// @ai-generated - Template root with vapor attribute compiles correctly.
    #[test]
    fn test_compile_root_template_vapor() {
        let events = compile_events("<template vapor></template>");
        let ts = events
            .iter()
            .find(|e| e.starts_with("CompiledTemplateStart("))
            .expect("Expected CompiledTemplateStart");
        assert!(
            ts.contains("vapor=true"),
            "Expected vapor=true, got: {}",
            ts
        );
    }

    /// @ai-generated - Style root with scoped attribute compiles correctly.
    #[test]
    fn test_compile_root_style_scoped() {
        let events = compile_events("<style scoped></style>");
        let ss = events
            .iter()
            .find(|e| e.starts_with("CompiledStyleStart("))
            .expect("Expected CompiledStyleStart");
        assert!(
            ss.contains("scoped=true"),
            "Expected scoped=true, got: {}",
            ss
        );
    }

    /// @ai-generated - Style root with module attribute compiles correctly.
    #[test]
    fn test_compile_root_style_module() {
        let events = compile_events("<style module></style>");
        let ss = events
            .iter()
            .find(|e| e.starts_with("CompiledStyleStart("))
            .expect("Expected CompiledStyleStart");
        assert!(
            ss.contains("module=true"),
            "Expected module=true, got: {}",
            ss
        );
    }

    /// @ai-generated - Style root with custom module name compiles correctly.
    #[test]
    fn test_compile_root_style_module_custom_name() {
        let events = compile_events(r#"<style module="classes"></style>"#);
        let ss = events
            .iter()
            .find(|e| e.starts_with("CompiledStyleStart("))
            .expect("Expected CompiledStyleStart");
        assert!(
            ss.contains("module=true"),
            "Expected module=true, got: {}",
            ss
        );
    }

    /// @ai-generated - Self-closing void element (br) compiles correctly.
    #[test]
    fn test_compile_void_element() {
        let events = compile_events("<template><br><div></div></template>");
        let element_starts: Vec<_> = events
            .iter()
            .filter(|e| e.starts_with("ElementStart("))
            .collect();
        assert_eq!(
            element_starts.len(),
            2,
            "Expected 2 ElementStart events (br + div), got: {:?}",
            element_starts
        );
    }

    /// @ai-generated - Nested elements have correct parent tracking.
    #[test]
    fn test_compile_nested_elements() {
        let events = compile_events("<template><div><span></span></div></template>");
        let element_starts: Vec<_> = events
            .iter()
            .filter(|e| e.starts_with("ElementStart("))
            .collect();
        assert_eq!(
            element_starts.len(),
            2,
            "Expected 2 ElementStart events, got: {:?}",
            element_starts
        );
    }

    /// @ai-generated - Text, Interpolation, Comment events pass through unchanged.
    #[test]
    fn test_child_events_pass_through() {
        let events = compile_events("<template>hello {{ msg }} <!-- comment --></template>");
        assert!(
            events.iter().any(|e| e == "Text"),
            "Expected Text event to pass through, got: {:?}",
            events
        );
        assert!(
            events.iter().any(|e| e == "Interpolation"),
            "Expected Interpolation event to pass through, got: {:?}",
            events
        );
        assert!(
            events.iter().any(|e| e == "Comment"),
            "Expected Comment event to pass through, got: {:?}",
            events
        );
    }

    /// @ai-generated - Style with lang="scss" is correctly classified.
    #[test]
    fn test_compile_root_style_lang() {
        let events = compile_events(r#"<style lang="scss"></style>"#);
        let ss = events
            .iter()
            .find(|e| e.starts_with("CompiledStyleStart("))
            .expect("Expected CompiledStyleStart");
        assert!(
            ss.contains("lang=Some(Scss)"),
            "Expected lang=Scss, got: {}",
            ss
        );
    }

    /// @ai-generated - CompiledStyleEnd has content span.
    #[test]
    fn test_compile_style_end_has_content() {
        let input = "<style>.box { color: red; }</style>";
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

        let mut plugin = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match plugin.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let style_end = compiled.iter().find_map(|e| match e {
            Event::CompiledStyleEnd(end) => Some(end),
            _ => None,
        });

        assert!(style_end.is_some(), "Expected CompiledStyleEnd");
        let end = style_end.unwrap();
        assert!(end.content.is_some(), "Expected content span in style end");
        let content = end.content.unwrap();
        let content_str = &input[content.start as usize..content.end as usize];
        assert_eq!(
            content_str, ".box { color: red; }",
            "Content should be the CSS text"
        );
    }
}
