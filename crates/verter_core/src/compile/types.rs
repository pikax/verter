//! Type definitions for the compilation pipeline.
//!
//! Contains all option structs, result types, and enums used by [`super::compile()`].

/// Whitespace handling strategy for template compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitespaceStrategy {
    /// Condense whitespace (Vue default): collapse consecutive whitespace to a single space,
    /// remove whitespace-only text nodes between elements.
    Condense,
    /// Preserve all whitespace as-is.
    Preserve,
}

/// Shared codegen options that control template compilation behaviour.
///
/// These mirror the Vue compiler's public options (comments, whitespace, hoisting,
/// custom elements, etc.) and are passed through to both script and template codegen.
/// Use the `resolve_*` methods to apply defaults.
#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    /// The filename for source map generation and component name extraction.
    pub filename: Option<String>,
    /// Production mode — affects component ID generation and optimizations.
    pub is_production: bool,
    /// Custom component ID (overrides auto-generation from filename).
    pub component_id: Option<String>,
    /// When true, include the TSX codegen plugin in the pipeline.
    /// When false, the pipeline still runs (producing compiled CSS) but skips TSX generation.
    pub include_tsx: bool,
    /// When true, skip source map generation and base64 encoding.
    /// Returns empty strings for `source_map` and `code_with_source_map`.
    pub skip_source_map: bool,

    // -- Vue compiler parity options --
    /// Custom interpolation delimiters. Default: `("{{", "}}")`.
    pub delimiters: Option<(String, String)>,
    /// Tag name prefixes treated as custom elements (skip component resolution).
    /// E.g. `["ion-", "my-"]` matches `<ion-button>`, `<my-card>`.
    pub custom_elements: Option<Vec<String>>,
    /// Whether to preserve HTML comments in output.
    /// `None` = `!is_production` (comments in dev, stripped in prod).
    pub comments: Option<bool>,
    /// Runtime module name to import helpers from. Default: `"vue"`.
    pub runtime_module_name: Option<String>,
    /// Hoist static VNodes/props to constants outside the render function.
    /// `None` = `true`.
    pub hoist_static: Option<bool>,
    /// Whitespace handling strategy. `None` = `Condense`.
    pub whitespace: Option<WhitespaceStrategy>,
    /// Cache event handler expressions. `None` = `false`.
    pub cache_handlers: Option<bool>,
    /// Inline the render function inside `setup()`.
    /// `None` = `is_production`.
    pub inline: Option<bool>,
    /// Indicates the SFC uses `:slotted()` in styles.
    /// `None` = `true`.
    pub slotted: Option<bool>,
    /// Add `_ctx.`/`$setup.` prefix to template identifiers.
    /// `None` = `true`.
    pub prefix_identifiers: Option<bool>,
}

impl CodegenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    pub fn with_production(mut self, is_production: bool) -> Self {
        self.is_production = is_production;
        self
    }

    // -- Resolved accessors (apply defaults) --

    /// Whether to preserve HTML comments in output.
    pub fn resolve_comments(&self) -> bool {
        self.comments.unwrap_or(!self.is_production)
    }

    /// Whether to hoist static VNodes/props.
    pub fn resolve_hoist_static(&self) -> bool {
        self.hoist_static.unwrap_or(true)
    }

    /// Whitespace handling strategy.
    pub fn resolve_whitespace(&self) -> WhitespaceStrategy {
        self.whitespace.unwrap_or(WhitespaceStrategy::Condense)
    }

    /// Whether to cache event handlers.
    pub fn resolve_cache_handlers(&self) -> bool {
        self.cache_handlers.unwrap_or(false)
    }

    /// Whether to inline the render function.
    pub fn resolve_inline(&self) -> bool {
        self.inline.unwrap_or(self.is_production)
    }

    /// Whether the SFC uses `:slotted()`.
    pub fn resolve_slotted(&self) -> bool {
        self.slotted.unwrap_or(true)
    }

    /// Whether to prefix template identifiers.
    pub fn resolve_prefix_identifiers(&self) -> bool {
        self.prefix_identifiers.unwrap_or(true)
    }

    /// Runtime module name for helper imports.
    pub fn resolve_runtime_module_name(&self) -> &str {
        self.runtime_module_name.as_deref().unwrap_or("vue")
    }
}

/// Severity level for a compilation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// A structured diagnostic emitted during compilation.
#[derive(Debug, Clone)]
pub struct CompileDiagnostic {
    /// Severity level.
    pub severity: CompileDiagnosticSeverity,
    /// Vue-compatible error code string (e.g., "X_MISSING_END_TAG").
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Optional source span (byte offsets into original input).
    pub span: Option<crate::common::Span>,
}

/// A custom block extracted from the SFC (e.g., `<i18n>`, `<docs>`).
pub struct CustomBlock {
    /// The tag name (e.g., "i18n", "docs").
    pub block_type: String,
    /// Raw content between open and close tags (empty string for self-closing).
    pub content: String,
    /// Attributes as key-value pairs (e.g., `[("lang", "json")]`).
    /// Boolean attributes (no value) have an empty string value.
    pub attrs: Vec<(String, String)>,
}

/// Verter-specific compilation options, layered on top of [`CodegenOptions`].
///
/// Controls Vapor-mode output, TypeScript stripping, source map generation,
/// and cross-file type resolution for macros like `defineProps<ExternalType>()`.
#[derive(Default)]
pub struct VerterCompileOptions {
    /// When true, force Vapor mode output regardless of template attributes,
    /// and implicitly treat script as `<script setup>`.
    pub force_vapor: bool,
    /// When true, strip remaining TypeScript syntax (type annotations, generics)
    /// from script output to produce valid JavaScript.
    pub force_js: bool,
    /// When true, generate a source map for the template output.
    pub source_map: bool,
    /// Pre-resolved external types for cross-file type resolution.
    ///
    /// Keyed by type name (e.g., `"BadgeProps"`), value is the resolved type elements.
    /// These are merged into the type resolution context alongside companion `<script>`
    /// types, enabling `defineProps<ExternalType>()` to resolve types from other files.
    ///
    /// The host is responsible for resolving these from its file store before compilation.
    pub external_types:
        Option<rustc_hash::FxHashMap<String, crate::utils::oxc::vue::ResolvedElements>>,
}

// ── Result types ───────────────────────────────────────────────────

/// The complete output of [`super::compile()`], containing generated code for each SFC block.
pub struct VerterCompileResult {
    pub script: Option<VerterScriptBlock>,
    pub template: Option<VerterTemplateBlock>,
    pub styles: Vec<VerterStyleBlock>,
    pub custom_blocks: Vec<VerterCustomBlock>,
    pub scope_id: String,
    pub errors: Vec<CompileDiagnostic>,
    pub parse_duration_ms: f64,
    pub total_duration_ms: f64,
    /// Combined TSX output for IDE type checking. Present when `include_tsx` is true.
    /// Contains both script types and template JSX in a single `.tsx` file.
    pub tsx: Option<VerterTsxBlock>,
}

/// Generated output for the `<script>` or `<script setup>` block.
pub struct VerterScriptBlock {
    pub code: String,
    pub duration_ms: f64,
    pub source_map: String,
    pub setup: bool,
    pub attrs: Vec<(String, String)>,
}

/// Generated output for the `<template>` block (VDOM or Vapor render function).
pub struct VerterTemplateBlock {
    pub code: String,
    pub source_map: String,
    pub imports: Vec<&'static str>,
    pub duration_ms: f64,
    pub attrs: Vec<(String, String)>,
}

/// Generated output for a single `<style>` block (CSS with scoping/modules applied).
pub struct VerterStyleBlock {
    pub code: String,
    pub scoped: bool,
    pub lang: Option<String>,
    pub duration_ms: f64,
    pub attrs: Vec<(String, String)>,
}

/// A custom block extracted from the SFC (e.g., `<i18n>`, `<docs>`).
pub struct VerterCustomBlock {
    pub block_type: String,
    pub content: String,
    pub attrs: Vec<(String, String)>,
}

/// Generated TSX block for IDE type checking (script or template).
pub struct VerterTsxBlock {
    /// The generated TSX code.
    pub code: String,
    /// JSON source map string (empty if source maps disabled).
    pub source_map: String,
    /// Duration of generation in milliseconds.
    pub duration_ms: f64,
}
