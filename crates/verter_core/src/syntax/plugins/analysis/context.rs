//! Context for analysis plugins.

/// Options for analysis plugins
#[derive(Clone, Debug, Default)]
pub struct AnalysisPluginOptions {
    /// Server-side rendering mode
    pub ssr: bool,
}

/// Attributes detected on a script tag
#[derive(Clone, Debug, Default)]
pub struct ScriptAttributes<'a> {
    /// Whether this is a <script setup> block
    pub setup: bool,
    /// The lang attribute value (e.g., "ts", "tsx")
    pub lang: Option<&'a str>,
}

/// Context for analysis plugins, holding the source input and options.
pub struct AnalysisPluginContext<'a> {
    /// The input source as a string
    pub input: &'a str,
    /// The input source as bytes
    pub bytes: &'a [u8],
    /// Analysis options
    pub options: &'a AnalysisPluginOptions,
}

impl<'a> AnalysisPluginContext<'a> {
    pub fn new(input: &'a str, bytes: &'a [u8], options: &'a AnalysisPluginOptions) -> Self {
        Self {
            input,
            bytes,
            options,
        }
    }

    /// Get a slice of the input source from start to end positions
    pub fn slice(&self, start: u32, end: u32) -> &'a str {
        &self.input[start as usize..end as usize]
    }
}
