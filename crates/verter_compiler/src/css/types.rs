//! Types for the lightningcss-based CSS style processor.

use std::borrow::Cow;

/// Errors that can occur during CSS processing.
#[derive(Debug, Clone)]
pub enum CssError {
    /// A CSS parsing error from lightningcss.
    Parse(String),
    /// A CSS serialization error from lightningcss.
    Serialize(String),
}

impl std::fmt::Display for CssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CssError::Parse(msg) => write!(f, "CSS parse error: {msg}"),
            CssError::Serialize(msg) => write!(f, "CSS serialization error: {msg}"),
        }
    }
}

impl std::error::Error for CssError {}

impl From<CssError> for String {
    fn from(e: CssError) -> String {
        e.to_string()
    }
}

/// Options for processing a style block with lightningcss.
#[derive(Debug, Clone)]
pub struct ProcessStyleOptions<'a> {
    /// Scope ID string (e.g., "a4f2eed6")
    pub scope_id: &'a str,
    /// Whether this style block is scoped
    pub scoped: bool,
    /// Whether this is a CSS module block
    pub is_module: bool,
    /// Custom CSS module variable name (e.g., `"classes"` for `<style module="classes">`).
    /// Defaults to `"$style"` when `None`. Passed through to the result — does not
    /// affect CSS processing itself, only the variable name the bundler injects.
    pub module_name: Option<&'a str>,
    /// Source filename for source map generation
    pub filename: Option<&'a str>,
    /// Whether to generate source maps
    pub sourcemap: bool,
}

/// Result of processing a style block.
///
/// `code` is a [`Cow`] over the input CSS: a zero-marker `<style>` (no scoped,
/// module, deep, slotted, or v-bind) is returned by borrowing the input
/// verbatim, so the passthrough costs no allocation. Any transformed surface
/// (scoping, module hashing, v-bind/deep/slotted rewrite) yields an owned buffer.
///
/// Alongside the code, the result carries the structural facts the owner path
/// discovered while processing — whether scoping was applied, whether a
/// `:deep()`/`:slotted()` selector was rewritten, and whether lightningcss
/// normalization ran. Consumers read these facts instead of re-deriving them by
/// re-scanning the CSS text.
#[derive(Debug, Clone)]
pub struct ProcessStyleResult<'a> {
    /// Transformed CSS code — borrowed from the input on a zero-marker passthrough.
    pub code: Cow<'a, str>,
    /// Source map as JSON string (if sourcemap was requested)
    pub source_map: Option<String>,
    /// CSS module class mappings (original → hashed)
    pub module_classes: Vec<(String, String)>,
    /// CSS module variable name (e.g., `"$style"` or custom name from `<style module="...">`).
    /// Only set when `is_module` was `true`.
    pub module_name: Option<String>,
    /// v-bind() expressions found and replaced
    pub v_bind_vars: Vec<VBindVar>,
    /// Whether scoped attribute selectors (`[data-v-…]`) were applied to the surface.
    pub scoped: bool,
    /// Whether a `:deep()` / `::v-deep()` selector was found and rewritten.
    pub has_deep: bool,
    /// Whether a `:slotted()` / `::v-slotted()` selector was found and rewritten.
    pub has_slotted: bool,
    /// Whether lightningcss normalization ran. True only when a CSS-modules or
    /// scoped transform required a flattened, well-formed AST; a marker-free or
    /// v-bind-only surface skips normalization and leaves this `false`.
    pub normalization_needed: bool,
}

/// A v-bind() expression that was replaced with a CSS variable.
#[derive(Debug, Clone)]
pub struct VBindVar {
    /// The original expression text (e.g., "color" or "theme.color")
    pub expression: String,
    /// The generated CSS variable name (e.g., "--a4f2eed6-color")
    pub var_name: String,
}
