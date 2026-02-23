//! Style codegen for the AST-based pipeline.
//!
//! Scans `<style>` blocks for Vue-specific syntax (`v-bind()`) and produces
//! a [`CodeGenOutput`] with overwrite operations. The caller applies these
//! to extract modified CSS, then optionally passes through
//! [`crate::css::process_style()`] for scoped/module processing.
//!
//! For non-CSS languages (SCSS, Less, Stylus), only `v-bind()` replacements
//! are applied — the content is returned for external preprocessor handling.

pub mod v_bind;

use oxc_allocator::Allocator;

use crate::css::types::VBindVar;
use crate::parser::types::{RootNodeStyle, StyleLang};
use crate::template::code_gen::types::CodeGenOutput;

/// Result of style codegen for a single `<style>` block.
///
/// Contains a [`CodeGenOutput`] with `v-bind()` overwrites (at absolute SFC
/// positions) and the extracted variable metadata for script-side `_useCssVars`
/// injection.
///
/// The caller is responsible for:
/// 1. Applying the overwrites to a CodeTransform of the style content
/// 2. Extracting the CSS string
/// 3. For CSS lang: calling `process_style()` for scoped/module finalization
/// 4. For non-CSS lang: returning as-is for the external preprocessor
/// 5. Removing the `<style>` block from the main JS CodeTransform
pub struct StyleCodeGenResult<'alloc> {
    /// CodeGenOutput with v-bind() overwrites (absolute SFC positions).
    pub out: CodeGenOutput<'alloc>,
    /// Extracted v-bind() variables for `_useCssVars` injection.
    pub v_bind_vars: Vec<VBindVar>,
}

/// Processed CSS output for a single `<style>` block.
///
/// Produced by the orchestrator after applying [`StyleCodeGenResult`]'s
/// CodeGenOutput and (for CSS lang) running `process_style()`.
#[derive(Debug, Clone)]
pub struct StyleOutput {
    /// Processed CSS code (with v-bind replaced, and for CSS lang: scoped/modules applied).
    pub code: String,
    /// Whether this block has the `scoped` attribute.
    pub scoped: bool,
    /// The style preprocessor language.
    pub lang: Option<StyleLang>,
    /// CSS Modules class mappings (original → hashed). `None` if not a module block.
    pub module: Option<Vec<(String, String)>>,
    /// Errors encountered during CSS processing.
    pub errors: Vec<String>,
    /// Extracted v-bind() variables for `_useCssVars` injection.
    pub v_bind_vars: Vec<VBindVar>,
}

/// Scan a single `<style>` block for Vue-specific syntax.
///
/// Produces a [`CodeGenOutput`] with `v-bind()` overwrite operations.
/// Does NOT call `process_style()` — the caller handles that.
///
/// If the style block has no content, returns an empty result.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn generate_style<'alloc>(
    style: &RootNodeStyle,
    source: &'alloc str,
    alloc: &'alloc Allocator,
    scope_id: &str,
) -> StyleCodeGenResult<'alloc> {
    let mut out = CodeGenOutput::new(alloc);
    let mut v_bind_vars = Vec::new();

    if let Some(content) = &style.content {
        let css = &source[content.start as usize..content.end as usize];
        v_bind::scan_v_bind(css, content.start, scope_id, &mut out, &mut v_bind_vars);
    }

    StyleCodeGenResult { out, v_bind_vars }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;
    use crate::types::NodeTag;

    fn make_style(content_start: u32, content_end: u32, scoped: bool) -> RootNodeStyle {
        RootNodeStyle {
            tag_open: NodeTag {
                start: 0,
                end: content_start,
                name_end: 6,
            },
            tag_close: Some(NodeTag {
                start: content_end,
                end: content_end + 8,
                name_end: content_end + 7,
            }),
            lang: None,
            scoped,
            module: false,
            attributes: Vec::new(),
            content: Some(Span {
                start: content_start,
                end: content_end,
            }),
        }
    }

    #[test]
    fn empty_style_no_content() {
        let alloc = Allocator::default();
        let style = RootNodeStyle {
            tag_open: NodeTag {
                start: 0,
                end: 7,
                name_end: 6,
            },
            tag_close: Some(NodeTag {
                start: 7,
                end: 15,
                name_end: 14,
            }),
            lang: None,
            scoped: false,
            module: false,
            attributes: Vec::new(),
            content: None,
        };
        let result = generate_style(&style, "<style></style>", &alloc, "abc");
        assert!(result.out.overwrites.is_empty());
        assert!(result.v_bind_vars.is_empty());
    }

    #[test]
    fn style_with_v_bind() {
        let source = "<style>.box { color: v-bind(color); }</style>";
        // content starts at 7 (after <style>), ends at 37 (before </style>)
        let content = ".box { color: v-bind(color); }";
        let content_start = 7u32;
        let content_end = content_start + content.len() as u32;

        let alloc = Allocator::default();
        let style = make_style(content_start, content_end, false);
        let result = generate_style(&style, source, &alloc, "a4f2eed6");

        assert_eq!(result.out.overwrites.len(), 1);
        // v-bind(color) starts at offset 14 within content + 7 offset = 21
        assert_eq!(result.out.overwrites[0].0, 21); // 7 + 14
        assert_eq!(result.out.overwrites[0].1, 34); // 7 + 27
        assert_eq!(result.out.overwrites[0].2, "var(--a4f2eed6-color)");
        assert_eq!(result.v_bind_vars.len(), 1);
        assert_eq!(result.v_bind_vars[0].expression, "color");
    }

    #[test]
    fn style_no_v_bind() {
        let source = "<style>.box { color: red; }</style>";
        let content_start = 7u32;
        let content_end = 26u32;

        let alloc = Allocator::default();
        let style = make_style(content_start, content_end, false);
        let result = generate_style(&style, source, &alloc, "abc");

        assert!(result.out.overwrites.is_empty());
        assert!(result.v_bind_vars.is_empty());
    }

    #[test]
    fn style_multiple_v_binds() {
        let source = "<style>.box { color: v-bind(fg); bg: v-bind(bg); }</style>";
        let content_start = 7u32;
        let content = &source[content_start as usize..source.len() - 8]; // before </style>
        let content_end = content_start + content.len() as u32;

        let alloc = Allocator::default();
        let style = make_style(content_start, content_end, false);
        let result = generate_style(&style, source, &alloc, "a4f2eed6");

        assert_eq!(result.out.overwrites.len(), 2);
        assert_eq!(result.v_bind_vars.len(), 2);
        assert_eq!(result.v_bind_vars[0].expression, "fg");
        assert_eq!(result.v_bind_vars[1].expression, "bg");
    }
}
