//! CSS diagnostic rules.
//!
//! These rules check CSS `<style>` blocks against template elements using
//! `check_file` (which provides the full `FileContext`).

mod scoped_css_cascade;
mod undefined_css_class;
mod unused_css_selector;

pub use scoped_css_cascade::ScopedCssCascade;
pub use undefined_css_class::UndefinedCssClass;
pub use unused_css_selector::UnusedCssSelector;
