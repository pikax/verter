//! Types for the lightningcss-based CSS style processor.

/// Options for processing a style block with lightningcss.
#[derive(Debug, Clone)]
pub struct ProcessStyleOptions {
    /// Scope ID string (e.g., "a4f2eed6")
    pub scope_id: String,
    /// Whether this style block is scoped
    pub scoped: bool,
    /// Whether this is a CSS module block
    pub is_module: bool,
    /// Custom module name (None = "$style")
    pub module_name: Option<String>,
    /// Source filename for source map generation
    pub filename: Option<String>,
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
