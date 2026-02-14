//! CSS code generation plugin.
//!
//! Processes `CssParsedStyle` events from the css_parser plugin:
//! - Applies scoped CSS, `:deep()`, `:slotted()`, `:global()` transformations
//! - Replaces `v-bind(expr)` with `var(--{scopeId}-{sanitized})`
//! - Applies CSS module class name hashing
//! - Removes `<style>...</style>` tags from JS output via `CodeTransform::remove()`
//! - Stores `CssStyleOutput` for `CodegenResult.styles`
//!
//! Returns `Keep(event)` so other code_gen plugins (e.g. script) can also
//! read `CssParsedStyle` events for `useCssVars` injection.

use std::{cell::RefCell, rc::Rc};

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{CssModuleClassMapping, CssModuleInfo, CssParsedStyleBlock, Event, StyleLang},
    },
};

/// Compiled CSS output for a single `<style>` block.
///
/// Produced by [`CssGeneratorPlugin`] after processing scoped selectors, v-bind
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

/// CSS code generation plugin for the syntax_kai pipeline.
///
/// Consumes `CssParsedStyle` events and:
/// 1. Transforms CSS via `crate::css::process_style()` (scoping, modules, v-bind)
/// 2. Removes `<style>` tags from JS output via `CodeTransform::remove()`
/// 3. Stores processed CSS in `CssStyleOutput` for the build result
pub struct CssGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    scope_id: [u8; 8],
    styles: Vec<CssStyleOutput>,
}

impl<'alloc> CssGeneratorPlugin<'alloc> {
    pub fn new(code_transform: Rc<RefCell<CodeTransform<'alloc>>>, scope_id: [u8; 8]) -> Self {
        Self {
            code_transform,
            scope_id,
            styles: Vec::new(),
        }
    }

    /// Take the collected CSS outputs, leaving the internal buffer empty.
    pub fn take_styles(&mut self) -> Vec<CssStyleOutput> {
        std::mem::take(&mut self.styles)
    }

    fn process_parsed_style(&mut self, parsed: &CssParsedStyleBlock, ctx: &SyntaxPluginContext) {
        let scope_id_str = std::str::from_utf8(&self.scope_id).unwrap_or("00000000");
        let is_module = parsed.module.is_some();

        let (code, module, errors) = if let Some(content_span) = parsed.content {
            let css_str = &ctx.input[content_span.start as usize..content_span.end as usize];
            let needs_transform = parsed.scoped || !parsed.v_binds.is_empty() || is_module;

            if needs_transform {
                let options = crate::css::ProcessStyleOptions {
                    scope_id: scope_id_str,
                    scoped: parsed.scoped,
                    is_module,
                    filename: None,
                    sourcemap: false,
                };
                match crate::css::process_style(css_str, &options) {
                    Ok(result) => {
                        let module_info = if is_module {
                            let custom_name = parsed.module.and_then(|span| {
                                if span.start == 0 && span.end == 0 {
                                    None
                                } else {
                                    Some(span)
                                }
                            });
                            Some(CssModuleInfo {
                                custom_name,
                                classes: result
                                    .module_classes
                                    .iter()
                                    .map(|(orig, hashed)| CssModuleClassMapping {
                                        original: orig.clone(),
                                        hashed: hashed.clone(),
                                    })
                                    .collect(),
                            })
                        } else {
                            None
                        };
                        (result.code, module_info, Vec::new())
                    }
                    Err(e) => (css_str.to_string(), None, vec![e]),
                }
            } else {
                (css_str.to_string(), None, Vec::new())
            }
        } else {
            (String::new(), None, Vec::new())
        };

        self.styles.push(CssStyleOutput {
            code,
            scoped: parsed.scoped,
            lang: parsed.lang,
            module,
            errors,
        });

        // Remove the full <style>...</style> tag from JS output
        let remove_start = parsed.compiled_start.tag_open_event.start;
        let remove_end = parsed.compiled_end.end;
        self.code_transform
            .borrow_mut()
            .remove(remove_start, remove_end);
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for CssGeneratorPlugin<'alloc> {
    fn name(&self) -> &str {
        "code_gen_css"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        if let Event::CssParsedStyle(ref parsed) = event {
            self.process_parsed_style(parsed, ctx);
        }
        SyntaxResult::Keep(event)
    }
}
