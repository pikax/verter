//! Block extractor plugin for Vite integration.
//!
//! This plugin hooks into the syntax pipeline to extract script, template,
//! and style blocks for Vite's virtual module system.
//!
//! **IMPORTANT**: This plugin stores UTF-8 byte offsets internally.
//! Conversion to UTF-16 happens at return time in `generate_for_vite()`.

use crate::cursor::script_detector::ScriptLanguage;
use crate::syntax::{
    plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
    types::{CssStyleContent, OxcScriptContent, StyleLang, SyntaxEvent, SyntaxTagType},
};

/// Block type for extracted blocks
#[derive(Debug, Clone)]
pub enum BlockType {
    Script,
    ScriptSetup,
    Template,
    Style,
}

/// Extracted block with UTF-8 byte positions (internal representation)
#[derive(Debug, Clone)]
pub struct ExtractedBlockInternal {
    pub block_type: BlockType,
    /// Raw content of the block
    pub content: String,
    /// Language attribute (ts, tsx, scss, etc.)
    pub lang: Option<String>,
    /// Is scoped style
    pub scoped: bool,
    /// Is CSS module
    pub module: bool,
    /// Start position in UTF-8 bytes (content start)
    pub start: u32,
    /// End position in UTF-8 bytes (content end)
    pub end: u32,
}

/// Plugin that extracts raw blocks during tokenization.
///
/// Stores UTF-8 byte offsets - conversion to UTF-16 happens at return time.
pub struct BlockExtractorPlugin {
    /// Extracted script blocks
    pub scripts: Vec<ExtractedBlockInternal>,
    /// Extracted style blocks
    pub styles: Vec<ExtractedBlockInternal>,
    /// Template info (we track open/close but content is the render function)
    pub template_start: Option<u32>,
    pub template_end: Option<u32>,
}

impl BlockExtractorPlugin {
    pub fn new() -> Self {
        Self {
            scripts: Vec::new(),
            styles: Vec::new(),
            template_start: None,
            template_end: None,
        }
    }

    /// Extract script block information from OxcScriptContent event
    fn extract_script(&mut self, script: &OxcScriptContent, source: &str) {
        let content_start = script.content_start as usize;
        let content_end = script.content_end as usize;

        // Extract content safely
        let content = if content_start < content_end && content_end <= source.len() {
            source[content_start..content_end].to_string()
        } else {
            String::new()
        };

        // Determine block type
        let block_type = if script.setup.is_some() {
            BlockType::ScriptSetup
        } else {
            BlockType::Script
        };

        // Get language
        let lang = script.lang.and_then(|l| match l {
            ScriptLanguage::JavaScript => Some("js".to_string()),
            ScriptLanguage::TypeScript => Some("ts".to_string()),
            ScriptLanguage::JSX => Some("jsx".to_string()),
            ScriptLanguage::TSX => Some("tsx".to_string()),
            ScriptLanguage::Unknown => None,
        });

        self.scripts.push(ExtractedBlockInternal {
            block_type,
            content,
            lang,
            scoped: false,
            module: false,
            start: script.content_start,
            end: script.content_end,
        });
    }

    /// Extract style block information from CssStyleContent event
    fn extract_style(&mut self, style: &CssStyleContent, source: &str) {
        let content_start = style.content_start as usize;
        let content_end = style.content_end as usize;

        // Extract content safely
        let content = if content_start < content_end && content_end <= source.len() {
            source[content_start..content_end].to_string()
        } else {
            String::new()
        };

        // Get language
        let lang = style.lang.map(|l| match l {
            StyleLang::Css => "css".to_string(),
            StyleLang::Scss => "scss".to_string(),
            StyleLang::Sass => "sass".to_string(),
            StyleLang::Less => "less".to_string(),
            StyleLang::Stylus => "stylus".to_string(),
        });

        self.styles.push(ExtractedBlockInternal {
            block_type: BlockType::Style,
            content,
            lang,
            scoped: style.scoped,
            module: style.module.is_some(),
            start: style.content_start,
            end: style.content_end,
        });
    }
}

impl Default for BlockExtractorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SyntaxPlugin<'a> for BlockExtractorPlugin {
    fn name(&self) -> &str {
        "block_extractor"
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match &event {
            // Track template boundaries
            SyntaxEvent::OpenTagEnd(tag) if tag.tag_type == SyntaxTagType::Template => {
                self.template_start = Some(tag.end);
            }

            // Extract script content (already parsed by OxcParserPlugin)
            SyntaxEvent::OxcScriptContent(script) => {
                self.extract_script(script, ctx.input);
            }

            // Extract style content (already parsed by CssParserPlugin)
            SyntaxEvent::CssStyleContent(style) => {
                self.extract_style(style, ctx.input);
            }

            _ => {}
        }

        // Always forward the event - we're just observing
        SyntaxResult::Keep(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_extractor_plugin_new() {
        let plugin = BlockExtractorPlugin::new();
        assert!(plugin.scripts.is_empty());
        assert!(plugin.styles.is_empty());
        assert!(plugin.template_start.is_none());
    }
}
