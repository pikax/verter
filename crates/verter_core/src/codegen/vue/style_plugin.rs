//! Style codegen plugin - processes `<style>` blocks independently.
//!
//! Each style block gets its own `CodeTransform` wrapping the style content region,
//! so CSS transformations (scoped selectors, CSS modules) produce proper source maps.

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{CssStyleContent, CssVBindExpression, SyntaxEvent},
    },
};

use super::template::types::CssModuleEntry;

/// A processed style block with its own CodeTransform for source maps.
pub struct ProcessedStyle<'a> {
    /// CodeTransform wrapping the style content for source-mapped CSS output.
    pub code_transform: CodeTransform<'a>,
    /// Whether this is a scoped style.
    pub scoped: bool,
    /// Whether this is a CSS module.
    pub is_module: bool,
    /// Style language (css, scss, less, etc.).
    pub lang: Option<String>,
    /// Module name (e.g., "$style" or custom name).
    pub module_name: Option<String>,
    /// CSS module class mappings (original → hashed).
    pub module_classes: Vec<(String, String)>,
    /// v-bind() expressions found in this style block.
    pub v_bind_expressions: Vec<CssVBindExpression>,
    /// UTF-8 byte offset of content start in original SFC.
    pub content_start: u32,
    /// UTF-8 byte offset of content end in original SFC.
    pub content_end: u32,
    /// Tag open start in original SFC (for region tracking).
    pub tag_open_start: u32,
    /// Tag close end in original SFC (for region tracking).
    pub tag_close_end: u32,
}

impl<'a> ProcessedStyle<'a> {
    /// Get the transformed CSS code.
    pub fn get_code(&self) -> String {
        self.code_transform.to_string()
    }

    /// Generate source map JSON string for this style block.
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.generate_map_json(options)
    }
}

/// Style codegen plugin that processes each `<style>` block independently.
pub struct StyleCodegenPlugin<'a> {
    /// Original SFC source (for creating CodeTransforms).
    source: &'a str,
    /// Allocator for CodeTransform.
    alloc: &'a oxc_allocator::Allocator,
    /// Component name (for hash generation).
    component_name: String,
    /// Scope ID for scoped styles.
    scope_id: Option<[u8; 8]>,
    /// Processed style blocks.
    pub styles: Vec<ProcessedStyle<'a>>,
    /// Style region positions (tag_open_start, tag_close_end) for other plugins to remove.
    style_regions: Vec<(u32, u32)>,
}

impl<'a> StyleCodegenPlugin<'a> {
    pub fn new(source: &'a str, alloc: &'a oxc_allocator::Allocator, component_name: &str) -> Self {
        Self {
            source,
            alloc,
            component_name: component_name.to_string(),
            scope_id: None,
            styles: Vec::new(),
            style_regions: Vec::new(),
        }
    }

    /// Set scope ID for scoped styles.
    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    /// Get scope ID (computed from first scoped style, or pre-set).
    pub fn get_scope_id(&self) -> Option<[u8; 8]> {
        self.scope_id
    }

    /// Get style regions for other plugins to know what to remove.
    pub fn get_style_regions(&self) -> &[(u32, u32)] {
        &self.style_regions
    }

    /// Get all v-bind expressions across all style blocks.
    pub fn get_v_bind_expressions(&self) -> Vec<CssVBindExpression> {
        self.styles
            .iter()
            .flat_map(|s| s.v_bind_expressions.clone())
            .collect()
    }

    /// Get all CSS module entries.
    pub fn get_css_modules(&self) -> Vec<CssModuleEntry> {
        self.styles
            .iter()
            .filter(|s| s.is_module)
            .map(|s| CssModuleEntry {
                name: s
                    .module_name
                    .clone()
                    .unwrap_or_else(|| "$style".to_string()),
                classes: s.module_classes.clone(),
                css: s.get_code().into_bytes(),
            })
            .collect()
    }

    /// Process a CSS style content event.
    fn process_css_style_content(&mut self, css: &CssStyleContent, ctx: &SyntaxPluginContext<'a>) {
        use crate::builder::codegen::get_hash;

        // Record the style region
        self.style_regions
            .push((css.tag_open_start, css.tag_close_end));

        let css_content = &ctx.bytes[css.content_start as usize..css.content_end as usize];

        // Create a CodeTransform for this style block's content region.
        // We use the full SFC source so source map positions are correct relative to it.
        let mut code_transform = CodeTransform::new(self.source, self.alloc);

        // Remove everything before and after the style content
        if css.content_start > 0 {
            code_transform.remove(0, css.content_start);
        }
        if (css.content_end as usize) < self.source.len() {
            code_transform.remove(css.content_end, self.source.len() as u32);
        }

        let v_bind_expressions = Vec::new();
        let mut module_name = None;
        let mut module_classes = Vec::new();
        let mut is_module = false;

        // Determine style language
        let lang = css.lang.as_ref().map(|l| match l {
            crate::syntax::types::StyleLang::Css => "css".to_string(),
            crate::syntax::types::StyleLang::Scss => "scss".to_string(),
            crate::syntax::types::StyleLang::Sass => "sass".to_string(),
            crate::syntax::types::StyleLang::Less => "less".to_string(),
            crate::syntax::types::StyleLang::Stylus => "stylus".to_string(),
        });

        let is_plain_css = lang.is_none() || lang.as_deref() == Some("css");

        // Extract module info
        if let Some(ref module_span) = css.module {
            is_module = true;
            module_name = Some(if module_span.start == 0 && module_span.end == 0 {
                "$style".to_string()
            } else {
                ctx.input[module_span.start as usize..module_span.end as usize].to_string()
            });
        }

        if is_plain_css && (css.scoped || is_module) {
            // Plain CSS: use lightningcss for scoping and/or modules
            let scope_id_str = if let Some(id) = self.scope_id {
                std::str::from_utf8(&id).unwrap_or("00000000").to_string()
            } else {
                let hash = get_hash(&self.component_name);
                let hash_str = hash[..8.min(hash.len())].to_string();
                let hash_bytes = hash_str.as_bytes();
                let mut id = [0u8; 8];
                id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
                self.scope_id = Some(id);
                hash_str
            };

            let css_str = std::str::from_utf8(css_content).unwrap_or("");
            let options = crate::css::ProcessStyleOptions {
                scope_id: scope_id_str,
                scoped: css.scoped,
                is_module,
                module_name: module_name.clone(),
                filename: None,
                sourcemap: false,
            };

            match crate::css::process_style(css_str, &options) {
                Ok(result) => {
                    module_classes = result.module_classes;
                    code_transform.overwrite(css.content_start, css.content_end, &result.code);
                }
                Err(_e) => {
                    // If lightningcss processing fails, keep original
                }
            }
        }
        // Preprocessor langs: skip transforms — Vite plugin will handle
        // via preprocessCSS + NAPI processStyle. Raw content passes through.

        self.styles.push(ProcessedStyle {
            code_transform,
            scoped: css.scoped,
            is_module,
            lang,
            module_name,
            module_classes,
            v_bind_expressions,
            content_start: css.content_start,
            content_end: css.content_end,
            tag_open_start: css.tag_open_start,
            tag_close_end: css.tag_close_end,
        });
    }
}

impl<'a> SyntaxPlugin<'a> for StyleCodegenPlugin<'a> {
    fn name(&self) -> &str {
        "StyleCodegen"
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        if let SyntaxEvent::CssStyleContent(ref css) = event {
            self.process_css_style_content(css, ctx);
        }
        SyntaxResult::Keep(event)
    }
}
