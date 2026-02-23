//! Types for the lightningcss-based CSS style processor.

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
#[derive(Debug, Clone)]
pub struct ProcessStyleResult {
    /// Transformed CSS code
    pub code: String,
    /// Source map as JSON string (if sourcemap was requested)
    pub source_map: Option<String>,
    /// CSS module class mappings (original → hashed)
    pub module_classes: Vec<(String, String)>,
    /// CSS module variable name (e.g., `"$style"` or custom name from `<style module="...">`).
    /// Only set when `is_module` was `true`.
    pub module_name: Option<String>,
    /// v-bind() expressions found and replaced
    pub v_bind_vars: Vec<VBindVar>,
}

/// A v-bind() expression that was replaced with a CSS variable.
#[derive(Debug, Clone)]
pub struct VBindVar {
    /// The original expression text (e.g., "color" or "theme.color")
    pub expression: String,
    /// The generated CSS variable name (e.g., "--a4f2eed6-color")
    pub var_name: String,
}
