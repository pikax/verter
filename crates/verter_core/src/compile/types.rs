//! Type definitions for the compilation pipeline.
//!
//! Contains all option structs, result types, and enums used by [`super::compile()`].

bitflags::bitflags! {
    /// Controls which compilation steps run in the pipeline.
    ///
    /// Each consumer (bundler, LSP, MCP, TSC) needs a different subset of
    /// compilation outputs. Using `CompileTarget` lets the pipeline skip
    /// expensive steps whose output would be discarded.
    ///
    /// Use the preset constants for common configurations:
    /// - [`BUNDLER`](Self::BUNDLER) — style + script + template codegen (runtime output)
    /// - [`IDE`](Self::IDE) — TSX only (LSP type checking)
    /// - [`ANALYSIS`](Self::ANALYSIS) — script + template data (MCP static analysis)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CompileTarget: u8 {
        /// Run style codegen (v-bind scan, scoped CSS, CSS modules).
        const STYLE         = 0b0000_0001;
        /// Run script codegen (macros, bindings, imports, CodeTransform).
        const SCRIPT        = 0b0000_0010;
        /// Run template VDOM/Vapor/SSR codegen (render function output).
        const TEMPLATE      = 0b0000_0100;
        /// Run TSX codegen (valid JSX for LSP/TSGO type checking).
        const TSX           = 0b0000_1000;
        /// Run TSC codegen (minimal TypeScript declarations).
        const TSC           = 0b0001_0000;
        /// Extract raw template data for cross-file analysis.
        const TEMPLATE_DATA = 0b0010_0000;

        /// Bundler preset: style + script + template VDOM codegen.
        const BUNDLER  = Self::STYLE.bits() | Self::SCRIPT.bits() | Self::TEMPLATE.bits();
        /// IDE/LSP preset: TSX only (independent of style/script/template).
        const IDE      = Self::TSX.bits();
        /// MCP analysis preset: script (for bindings) + template data extraction.
        const ANALYSIS = Self::SCRIPT.bits() | Self::TEMPLATE_DATA.bits();
    }
}

impl Default for CompileTarget {
    fn default() -> Self {
        Self::BUNDLER
    }
}

impl CompileTarget {
    /// Whether style codegen should run.
    pub fn needs_style(self) -> bool {
        self.intersects(Self::STYLE)
    }

    /// Whether script codegen should run.
    ///
    /// True when SCRIPT, TEMPLATE, or TEMPLATE_DATA is set, since template
    /// codegen and template data extraction both consume script bindings.
    pub fn needs_script(self) -> bool {
        self.intersects(Self::SCRIPT | Self::TEMPLATE | Self::TEMPLATE_DATA)
    }

    /// Whether VDOM/Vapor/SSR template codegen should run.
    pub fn needs_template_codegen(self) -> bool {
        self.intersects(Self::TEMPLATE)
    }

    /// Whether TSX codegen should run.
    pub fn needs_tsx(self) -> bool {
        self.intersects(Self::TSX)
    }

    /// Whether TSC codegen should run.
    pub fn needs_tsc(self) -> bool {
        self.intersects(Self::TSC)
    }

    /// Whether raw template data extraction should run.
    pub fn needs_template_data(self) -> bool {
        self.intersects(Self::TEMPLATE_DATA)
    }
}

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
    /// Controls which compilation steps run.
    /// See [`CompileTarget`] for available flags and presets.
    pub target: CompileTarget,
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
    /// Types module name for TSX helper imports. Default: `"$verter/types"`.
    pub types_module_name: Option<String>,
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
    /// Embed `declare module "@verter/types"` in TSX output so that
    /// `import ... from "@verter/types"` resolves without the real package.
    /// When `false` (default), the ambient module block is omitted, relying
    /// on `@verter/types` being installed in `node_modules`.
    pub embed_ambient_types: bool,
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
    /// When true, compile for server-side rendering.
    /// Emits string-concatenation code (`_push()`, `_ssrRenderAttrs()`, etc.)
    /// instead of VDOM render functions. Also sets `__ssrInlineRender: true`
    /// on the component and attaches the render function as `ssrRender`.
    pub ssr: bool,
    /// Pre-resolved external types for cross-file type resolution.
    ///
    /// Keyed by type name (e.g., `"BadgeProps"`), value is the resolved type elements.
    /// These are merged into the type resolution context alongside companion `<script>`
    /// types, enabling `defineProps<ExternalType>()` to resolve types from other files.
    ///
    /// The host is responsible for resolving these from its file store before compilation.
    pub external_types:
        Option<rustc_hash::FxHashMap<String, crate::utils::oxc::vue::ResolvedElements>>,
    /// Deprecated: use `CodegenOptions::target` with `CompileTarget::TEMPLATE_DATA`.
    /// Kept for backward-compatibility with direct `compile()` callers.
    /// When true, ORs `CompileTarget::TEMPLATE_DATA` into the active target.
    pub extract_template_data: bool,
    /// Props known to be const across all call sites (from cross-file analysis).
    /// These are treated as `Static` for reactivity purposes while keeping
    /// `$props.`/`__props.` prefix for correct runtime access.
    /// The codegen skips dynamic tracking (patch flags / renderEffect) for these props.
    pub prop_constness_overrides: Option<rustc_hash::FxHashSet<String>>,
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
    /// Combined IDE output for type checking. Present when `CompileTarget::TSX` is set.
    /// Contains both script types and template JSX in a single `.tsx` (TS) or `.jsx` (JS) file.
    pub tsx: Option<VerterTsxBlock>,
    /// TSC declaration output for type checking (vue-tsc replacement).
    /// Present when `CompileTarget::TSC` is set.
    pub tsc: Option<VerterTsxBlock>,
    /// Raw template data for cross-file analysis. Present when `extract_template_data` is true.
    pub template_data: Option<super::template_data::RawTemplateData>,
}

/// Generated output for the `<script>` or `<script setup>` block.
pub struct VerterScriptBlock {
    pub code: String,
    pub duration_ms: f64,
    pub source_map: String,
    pub setup: bool,
    pub attrs: Vec<(String, String)>,
}

/// Generated output for the `<template>` block (VDOM, Vapor, or SSR render function).
pub struct VerterTemplateBlock {
    pub code: String,
    pub source_map: String,
    /// Runtime helper imports from `"vue"`.
    pub imports: Vec<&'static str>,
    /// SSR runtime helper imports from `"vue/server-renderer"`.
    /// Empty for non-SSR builds.
    pub ssr_imports: Vec<&'static str>,
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

/// Generated IDE block for type checking (TSX or JSX).
pub struct VerterTsxBlock {
    /// The generated TSX/JSX code.
    pub code: String,
    /// JSON source map string (empty if source maps disabled).
    pub source_map: String,
    /// Duration of generation in milliseconds.
    pub duration_ms: f64,
    /// `true` for JavaScript SFCs (.jsx output), `false` for TypeScript (.tsx output).
    pub is_jsx: bool,
}
