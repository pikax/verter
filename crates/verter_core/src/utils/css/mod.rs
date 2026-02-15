//! CSS scanning utilities for byte-level parsing of CSS and preprocessor languages.
//!
//! - `common` — character classification, skip helpers, trim utilities
//! - `shared` — language-agnostic scanning (v-bind, selectors, classes, special pseudos)
//! - `scanners` — per-language scanners (CSS, SCSS, Less, Stylus)

pub mod common;
pub mod scanners;
pub mod shared;

use crate::syntax::types::{CssParsedClass, CssParsedRule, CssParsedVBind, StyleLang};

/// Dispatch to the correct language scanner based on `StyleLang`.
///
/// Falls back to the CSS scanner for `None`, `Css`, `Sass`, and `Unknown`.
/// (Indented Sass is not supported; falls back to CSS scanner.)
pub fn scan_style(
    lang: Option<StyleLang>,
    content: &[u8],
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    v_binds: &mut Vec<CssParsedVBind>,
    classes: &mut Vec<CssParsedClass>,
) {
    match lang {
        Some(StyleLang::Scss) => scanners::scss::scan(content, offset, rules, v_binds, classes),
        Some(StyleLang::Less) => scanners::less::scan(content, offset, rules, v_binds, classes),
        Some(StyleLang::Stylus) => scanners::stylus::scan(content, offset, rules, v_binds, classes),
        _ => scanners::css::scan(content, offset, rules, v_binds, classes),
    }
}
