#[cfg(feature = "host_metrics")]
use std::collections::BTreeMap;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use thiserror::Error;

/// 128-bit hash (xxh3) stored as a byte array, used for content and semantic hashing.
pub type Hash16 = [u8; 16];

/// Discriminates between Vue Single File Components and other file types
/// (e.g. `.ts` files tracked for cross-file type resolution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// A `.vue` Single File Component — parsed into script/template/style blocks.
    VueSfc,
    /// A non-SFC file (`.ts`, `.js`, etc.) — tracked for dependency and export signatures.
    NonSfc,
}

/// Hot Module Replacement strategy injected into the assembled main module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HmrStrategy {
    /// No HMR code is emitted.
    None,
    /// Vite-style HMR (`import.meta.hot`).
    Vite,
    /// Webpack-style HMR (`module.hot`).
    Webpack,
}

/// Controls behavior when compilation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileErrorPolicy {
    /// Always return an error on compile failure.
    StrictError,
    /// In dev mode, serve the last known good output (with `stale: true`)
    /// instead of returning an error. Falls back to error if no good output exists.
    DevServeLastKnownGood,
}

/// Controls how much static analysis is performed during upsert().
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisLevel {
    /// Full analysis: OXC script parsing + lightningcss style analysis.
    /// Use for LSP (auto-complete, diagnostics, hover, etc.).
    Full,
    /// Essential analysis: OXC script parsing only (imports, macros, bindings).
    /// Skips style analysis. Use for bundler mode where smart invalidation
    /// needs import/macro data but style analysis is wasted work.
    Essential,
    /// No extra analysis. Only the SFC tokenization and hashing needed for
    /// compilation are performed. Smart invalidation falls back to Tier 1
    /// (full invalidation on any dependency change).
    None,
}

/// Configuration for a [`VerterHost`](crate::VerterHost) instance.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Whether the host is running in development mode.
    /// Enables features like `DevServeLastKnownGood` and `__file` injection.
    pub dev_mode: bool,
    /// How to handle compile errors (strict vs. last-known-good fallback).
    pub compile_error_policy: CompileErrorPolicy,
    /// URI scheme prefix for LSP virtual file IDs (e.g. `"verter-virtual"`).
    pub lsp_scheme: String,
    /// Maximum number of compile profiles cached per file. Excess profiles
    /// are evicted LRU by `last_access_tick`.
    pub max_profiles_per_file: usize,
    /// Extensions to try when matching extensionless import specifiers
    /// (e.g. `import './types'`) against canonical IDs with extensions
    /// (e.g. `/src/types.ts`). Order determines priority.
    pub resolve_extensions: Vec<String>,
    /// Controls how much static analysis runs during upsert().
    ///
    /// - `Full`: script + style analysis (for LSP)
    /// - `Essential`: script analysis only (for bundler — smart invalidation)
    /// - `None`: no extra analysis (only SFC parse for code_gen)
    ///
    /// When not `Full`, `get_analysis()` computes missing data on demand.
    pub analysis_level: AnalysisLevel,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            dev_mode: true,
            compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
            lsp_scheme: "verter-virtual".to_string(),
            max_profiles_per_file: 8,
            resolve_extensions: vec![
                ".ts".to_string(),
                ".tsx".to_string(),
                ".js".to_string(),
                ".jsx".to_string(),
                ".mts".to_string(),
                ".mjs".to_string(),
                ".cts".to_string(),
                ".cjs".to_string(),
            ],
            analysis_level: AnalysisLevel::Full,
        }
    }
}

/// Per-compilation variant options.
///
/// A single `.vue` file can be compiled multiple times with different profiles
/// (e.g. client vs. SSR, dev vs. production). Each profile produces a separate
/// compile slot in the cache, keyed by the hash of this struct.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompileProfile {
    /// Override filename passed to `verter_core` codegen (defaults to canonical ID).
    pub filename: Option<String>,
    /// Production mode: strips dev-only code (`__file`, HMR).
    pub is_production: bool,
    /// Server-side rendering mode.
    pub ssr: bool,
    /// HMR code injection strategy.
    pub hmr_strategy: HmrStrategy,
    /// Explicit component ID for scoped style hashing (auto-generated if `None`).
    pub component_id: Option<String>,
    /// Custom template expression delimiters (default `{{ }}`).
    pub delimiters: Option<(String, String)>,
    /// Tag names treated as custom elements (not resolved as Vue components).
    pub custom_elements: Option<Vec<String>>,
    /// Whether to preserve HTML comments in template output.
    pub comments: Option<bool>,
    /// Runtime module name for template imports (default `"vue"`).
    pub runtime_module_name: Option<String>,
    /// Force Vapor mode codegen regardless of `<template vapor>` attribute.
    pub force_vapor: bool,
    /// Strip TypeScript type annotations from compiled output.
    pub force_js: bool,
    /// Generate source maps for compiled output.
    pub source_map: bool,
    /// Generate TSX output for IDE type checking (script + template JSX).
    pub enable_types: bool,
}

impl Default for CompileProfile {
    fn default() -> Self {
        Self {
            filename: None,
            is_production: false,
            ssr: false,
            hmr_strategy: HmrStrategy::None,
            component_id: None,
            delimiters: None,
            custom_elements: None,
            comments: None,
            runtime_module_name: Some("vue".to_string()),
            force_vapor: false,
            force_js: false,
            source_map: false,
            enable_types: false,
        }
    }
}

/// Discriminator for the virtual files produced from a single `.vue` SFC.
///
/// Each SFC is split into multiple virtual nodes that the bundler loads
/// independently (script, template, styles, custom blocks), plus a `Main`
/// node that assembles them into a single ES module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VirtualNodeKind {
    /// The assembled main module (imports styles, binds render function, exports component).
    Main,
    /// The `<script>` / `<script setup>` options object.
    Script,
    /// The compiled `<template>` render function.
    Template,
    /// A `<style>` block at the given index.
    Style { index: usize },
    /// A custom block (e.g. `<i18n>`) at the given index.
    Custom { index: usize },
    /// Combined TSX output for LSP type checking (script types + template JSX).
    Tsx,
}

/// A `src="..."` attribute on an SFC block that references an external file.
///
/// Produced during parsing when `<script src="...">`, `<template src="...">`,
/// or `<style src="...">` is encountered. The host uses these to fetch and
/// merge external content before compilation.
#[derive(Debug, Clone)]
pub struct ExternalSourceRequest {
    /// Canonical ID of the SFC that contains the `src` attribute.
    pub owner_canonical_id: String,
    /// Which block kind the `src` belongs to.
    pub block_kind: ExternalBlockKind,
    /// Block index (relevant for styles and custom blocks).
    pub index: usize,
    /// Raw specifier from the `src` attribute value.
    pub specifier: String,
    /// Resolved canonical path of the external file.
    pub resolved_canonical_id: String,
}

/// Which SFC block kind an external `src` reference belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalBlockKind {
    Script,
    Template,
    Style,
    Custom,
}

/// Granular description of which SFC slices changed between two file versions.
///
/// Used by the host to selectively invalidate compile slots and by the
/// caller (bundler/LSP) to trigger targeted HMR updates.
#[derive(Debug, Clone, Default)]
pub struct SliceChanges {
    /// The combined script block content hash changed.
    pub script_changed: bool,
    /// The template content hash changed.
    pub template_changed: bool,
    /// Indices of style blocks whose content hash changed.
    pub style_indices_changed: Vec<usize>,
    /// Indices of custom blocks whose content hash changed.
    pub custom_indices_changed: Vec<usize>,
    /// The number of blocks changed (script/template/style/custom count mismatch).
    pub structure_changed: bool,
    /// Block attributes changed (e.g. `lang`, `scoped`, `setup` added/removed).
    pub descriptor_changed: bool,
}

/// Summary of a single import statement found in a script block.
///
/// Returned in [`HostUpdateResult::import_specifiers`] so the caller can
/// resolve non-relative import paths via bundler/LSP resolution.
#[derive(Debug, Clone)]
pub struct ScriptImportInfo {
    /// The import source specifier (e.g. `"vue"`, `"./types"`).
    pub source: String,
    /// Whether this is a `import type` (type-only import).
    pub is_type_only: bool,
    /// Imported binding names.
    pub bindings: Vec<String>,
}

/// Result of [`VerterHost::upsert`](crate::VerterHost::upsert) or
/// [`VerterHost::apply_style_overrides`](crate::VerterHost::apply_style_overrides).
///
/// Describes what changed so the caller can trigger targeted HMR updates
/// and resolve any external dependencies.
#[derive(Debug, Clone)]
#[must_use]
pub struct HostUpdateResult {
    /// Canonical file ID that was upserted.
    pub canonical_id: String,
    /// Whether any virtual nodes changed or were removed.
    pub changed: bool,
    /// Granular slice-level change breakdown.
    pub slice_changes: SliceChanges,
    /// Virtual node kinds that need recompilation.
    pub changed_virtual_nodes: Vec<VirtualNodeKind>,
    /// Virtual node kinds that were removed (e.g. a style block was deleted).
    pub removed_virtual_nodes: Vec<VirtualNodeKind>,
    /// Bundler-format virtual IDs for changed nodes.
    pub changed_virtual_ids: Vec<String>,
    /// Bundler-format virtual IDs for removed nodes.
    pub removed_virtual_ids: Vec<String>,
    /// LSP-format virtual IDs for changed nodes.
    pub changed_lsp_ids: Vec<String>,
    /// LSP-format virtual IDs for removed nodes.
    pub removed_lsp_ids: Vec<String>,
    /// Parse-phase diagnostics (syntax errors, warnings).
    pub diagnostics: DiagnosticsSnapshot,
    /// External `src="..."` requests that need caller-side file resolution.
    pub external_source_requests: Vec<ExternalSourceRequest>,
    /// Import specifiers found in script blocks, for caller-side resolution.
    pub import_specifiers: Vec<ScriptImportInfo>,
    /// Time spent in the parse phase (ms).
    pub parse_duration_ms: f64,
}

impl HostUpdateResult {
    /// Construct a "nothing changed" result with all-empty change lists.
    pub fn no_change(canonical_id: String) -> Self {
        Self {
            canonical_id,
            changed: false,
            slice_changes: SliceChanges::default(),
            changed_virtual_nodes: Vec::new(),
            removed_virtual_nodes: Vec::new(),
            changed_virtual_ids: Vec::new(),
            removed_virtual_ids: Vec::new(),
            changed_lsp_ids: Vec::new(),
            removed_lsp_ids: Vec::new(),
            diagnostics: DiagnosticsSnapshot::default(),
            external_source_requests: Vec::new(),
            import_specifiers: Vec::new(),
            parse_duration_ms: 0.0,
        }
    }
}

/// Serializable snapshot of file analysis data, suitable for WASM export.
///
/// Returned by [`VerterHost::get_analysis`](crate::VerterHost::get_analysis).
/// Contains the combined script and style analysis for an SFC.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisSnapshot {
    /// Import statements found in script blocks.
    pub imports: Vec<verter_analysis::AnalyzedImport>,
    /// Variable/function bindings declared in script blocks.
    pub bindings: Vec<verter_analysis::AnalyzedBinding>,
    /// Vue compiler macros used (defineProps, defineEmits, etc.).
    pub macros: Vec<verter_analysis::AnalyzedMacro>,
    /// Type dependencies from macros that reference external files.
    pub macro_type_deps: Vec<verter_analysis::MacroTypeDep>,
    /// Bitflags representing script characteristics (see `verter_analysis::ScriptFlags`).
    pub script_flags: u32,
    /// Per-style-block analysis (scoped, modules, v-bind usage).
    pub styles: Vec<verter_analysis::StyleBlockAnalysis>,
}

/// Result of [`VerterHost::resolve`](crate::VerterHost::resolve).
///
/// Maps a raw import identifier to its canonical ID and rendered virtual IDs
/// in both bundler query-string format and LSP `._VERTER_.` format.
#[derive(Debug, Clone)]
#[must_use]
pub struct ResolvedId {
    /// Normalized canonical file path (forward slashes, no query string).
    pub canonical_id: String,
    /// Which virtual node the raw ID points to.
    pub node_kind: VirtualNodeKind,
    /// Whether the file has been upserted into the host.
    pub exists_in_host: bool,
    /// Bundler-format virtual ID (e.g. `Comp.vue?vue&type=style&index=0&lang.css`).
    pub bundler_id: String,
    /// LSP-format virtual ID (e.g. `Comp.vue._VERTER_.style.0.css`).
    pub lsp_id: String,
}

/// Query parameters for [`VerterHost::get_virtual_file`](crate::VerterHost::get_virtual_file).
///
/// Provide either `raw_id` (parsed into canonical + node kind) or
/// `canonical_id` + `node_kind` explicitly. The `compile_profile` selects
/// which cached compile slot to use or compile with.
#[derive(Debug, Clone)]
pub struct VirtualQuery {
    /// Raw import ID (bundler or LSP format). Parsed to extract canonical ID and node kind.
    pub raw_id: Option<String>,
    /// Explicit canonical ID (used when `raw_id` is `None`).
    pub canonical_id: Option<String>,
    /// Explicit node kind (used when `raw_id` is `None`).
    pub node_kind: Option<VirtualNodeKind>,
    /// Compilation options for this request.
    pub compile_profile: CompileProfile,
}

/// Block-specific metadata attached to a [`VirtualFileResponse`].
#[derive(Debug, Clone, Default)]
pub struct VirtualMeta {
    /// Scoped style ID (e.g. `"data-v-abc123"`), present on Main nodes when styles are scoped.
    pub scope_id: Option<String>,
    /// Custom block type name (e.g. `"i18n"`).
    pub block_type: Option<String>,
    /// Style block index (for Style nodes).
    pub style_index: Option<usize>,
    /// Custom block index (for Custom nodes).
    pub custom_index: Option<usize>,
}

/// Result of [`VerterHost::get_virtual_file`](crate::VerterHost::get_virtual_file).
///
/// Contains the compiled code for a single virtual node, along with
/// diagnostics and metadata. The `stale` flag indicates fallback to
/// last-known-good output when the current source has compile errors.
#[derive(Debug, Clone)]
#[must_use]
pub struct VirtualFileResponse {
    /// Rendered virtual ID (bundler or LSP format, matching the query).
    pub id: String,
    /// Compiled code for this virtual node.
    pub code: Arc<str>,
    /// Source map (JSON string), if `source_map` was enabled in the profile.
    pub source_map: Option<Arc<str>>,
    /// Output language (e.g. `"js"`, `"ts"`, `"css"`, `"scss"`).
    pub lang: Option<String>,
    /// `true` if this is last-known-good fallback output (current source has errors).
    pub stale: bool,
    /// Compilation diagnostics (errors, warnings) from this compile slot.
    pub diagnostics: DiagnosticsSnapshot,
    /// Block-specific metadata (scope ID, block type, index).
    pub meta: VirtualMeta,
}

/// Input to [`VerterHost::upsert`](crate::VerterHost::upsert).
#[derive(Debug, Clone)]
pub struct UpsertRequest {
    /// Pre-resolved canonical ID. If `None`, derived from `input_id`.
    pub canonical_id: Option<String>,
    /// Raw file path or identifier as provided by the caller.
    pub input_id: String,
    /// Full source text of the file.
    pub source: Arc<str>,
    /// Whether this is a Vue SFC or a non-SFC dependency.
    pub file_kind: FileKind,
    /// Additional path aliases that should resolve to this file.
    pub aliases: Vec<String>,
}

/// A single preprocessor-compiled style block to override in the compile cache.
#[derive(Debug, Clone)]
pub struct StyleOverrideEntry {
    /// Style block index this override applies to.
    pub index: usize,
    /// Preprocessed CSS code.
    pub code: Arc<str>,
    /// Source map from the preprocessor, if available.
    pub source_map: Option<Arc<str>>,
}

/// Input to [`VerterHost::apply_style_overrides`](crate::VerterHost::apply_style_overrides).
#[derive(Debug, Clone)]
pub struct StyleOverrideRequest {
    /// Canonical ID of the file whose styles are being overridden.
    pub canonical_id: String,
    /// Compile profile to apply the overrides under.
    pub compile_profile: CompileProfile,
    /// Preprocessed style blocks to inject.
    pub overrides: Vec<StyleOverrideEntry>,
}

/// Result of [`VerterHost::remove`](crate::VerterHost::remove).
#[derive(Debug, Clone)]
pub struct HostRemoveResult {
    /// Canonical ID of the removed file.
    pub canonical_id: String,
}

/// Severity level for a [`HostDiagnostic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSeverity {
    Error,
    Warning,
    Info,
}

/// A single diagnostic (error, warning, or info) produced during parsing or compilation.
#[derive(Debug, Clone)]
pub struct HostDiagnostic {
    pub severity: HostSeverity,
    /// Machine-readable error code (e.g. `"HOST_MISSING_EXTERNAL_SOURCE"`).
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Byte offset of the start of the problematic span, if available.
    pub span_start: Option<u32>,
    /// Byte offset of the end of the problematic span, if available.
    pub span_end: Option<u32>,
}

/// Collection of diagnostics with a precomputed `has_errors` flag.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsSnapshot {
    /// All diagnostics (errors + warnings + info).
    pub diagnostics: Vec<HostDiagnostic>,
    /// `true` if at least one diagnostic has [`HostSeverity::Error`].
    pub has_errors: bool,
}

impl DiagnosticsSnapshot {
    pub(crate) fn from_vec(diagnostics: Vec<HostDiagnostic>) -> Self {
        let has_errors = diagnostics
            .iter()
            .any(|d| d.severity == HostSeverity::Error);
        Self {
            diagnostics,
            has_errors,
        }
    }

    pub(crate) fn merge(mut self, mut other: DiagnosticsSnapshot) -> Self {
        self.diagnostics.append(&mut other.diagnostics);
        self.has_errors = self.has_errors || other.has_errors;
        self
    }
}

/// Errors returned by [`VerterHost`](crate::VerterHost) operations.
#[derive(Debug, Error)]
pub enum HostError {
    /// The requested file has not been upserted into the host.
    #[error("missing source for canonical id '{canonical_id}'")]
    MissingSource { canonical_id: String },
    /// The virtual query could not be parsed (neither `raw_id` nor `canonical_id` + `node_kind`).
    #[error("invalid virtual query")]
    InvalidQuery,
    /// The requested virtual node does not exist for this file.
    #[error("missing virtual node for id '{canonical_id}'")]
    MissingVirtualNode { canonical_id: String },
    /// Compilation failed. Contains the error diagnostics.
    #[error("compile error")]
    CompileError { diagnostics: DiagnosticsSnapshot },
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedRawId {
    pub(crate) canonical_id: String,
    pub(crate) node_kind: VirtualNodeKind,
    pub(crate) was_lsp_like: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DescriptorMin {
    pub(crate) script_count: usize,
    pub(crate) template_count: usize,
    pub(crate) style_count: usize,
    pub(crate) custom_count: usize,
    pub(crate) script_attr_fingerprints: Vec<String>,
    pub(crate) template_attr_fingerprints: Vec<String>,
    pub(crate) style_attr_fingerprints: Vec<String>,
    pub(crate) custom_attr_fingerprints: Vec<String>,
    pub(crate) vapor: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SliceHashes {
    pub(crate) script: Option<Hash16>,
    pub(crate) template: Option<Hash16>,
    pub(crate) styles: Vec<Hash16>,
    pub(crate) custom: Vec<Hash16>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileMeta {
    pub(crate) has_script: bool,
    pub(crate) has_template: bool,
    /// True when any `<style scoped>` block exists. Used to expose a
    /// synthetic Script virtual node for template-only components that
    /// need `__scopeId` on the component object.
    pub(crate) has_scoped_style: bool,
    pub(crate) script_lang: Option<String>,
    pub(crate) style_langs: Vec<Option<String>>,
    pub(crate) custom_types: Vec<String>,
    pub(crate) custom_langs: Vec<Option<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct SrcBlockInfo {
    pub(crate) tag_name: String,
    pub(crate) resolved_canonical_id: String,
    pub(crate) tag_open_start: u32,
    pub(crate) tag_open_end: u32,
    pub(crate) tag_close_start: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct ParseSnapshot {
    pub(crate) whole_hash: Hash16,
    pub(crate) semantic_hash: Hash16,
    pub(crate) slices: SliceHashes,
    pub(crate) descriptor: DescriptorMin,
    pub(crate) meta: FileMeta,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) src_blocks: Vec<SrcBlockInfo>,
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) script_analysis: verter_analysis::ScriptAnalysisSnapshot,
    pub(crate) export_signatures: Vec<verter_analysis::ExportSignature>,
    pub(crate) style_analyses: Vec<verter_analysis::StyleBlockAnalysis>,
}

#[derive(Debug, Clone)]
pub(crate) struct StyleOverrideLayer {
    pub(crate) hash: u64,
    pub(crate) by_index: HashMap<usize, StyleOverrideEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedVirtualFile {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    pub(crate) lang: Option<String>,
    pub(crate) meta: VirtualMeta,
}

#[derive(Debug, Clone)]
pub(crate) struct CompileSlot {
    pub(crate) semantic_hash: Hash16,
    pub(crate) style_override_hash: u64,
    pub(crate) outputs: HashMap<VirtualNodeKind, CachedVirtualFile>,
    pub(crate) diagnostics: DiagnosticsSnapshot,
    pub(crate) last_good_outputs: Option<HashMap<VirtualNodeKind, CachedVirtualFile>>,
    pub(crate) last_access_tick: u64,
}

/// Lightweight extract of FileEntry fields needed for compilation,
/// avoids cloning heavy compile_slots and style_overrides maps.
pub(crate) struct CompileInput {
    pub(crate) canonical_id: String,
    pub(crate) source: Arc<str>,
    pub(crate) meta: FileMeta,
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) src_blocks: Vec<SrcBlockInfo>,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) style_override_layer: Option<StyleOverrideLayer>,
    /// Macro type dependencies for cross-file type resolution.
    pub(crate) macro_type_deps: Vec<verter_analysis::MacroTypeDep>,
}

#[derive(Debug, Clone)]
pub(crate) struct FileEntry {
    pub(crate) canonical_id: String,
    pub(crate) file_kind: FileKind,
    pub(crate) source: Arc<str>,
    pub(crate) whole_hash: Hash16,
    pub(crate) semantic_hash: Hash16,
    pub(crate) slices: SliceHashes,
    pub(crate) descriptor: DescriptorMin,
    pub(crate) meta: FileMeta,
    pub(crate) aliases: BTreeSet<String>,
    pub(crate) dependencies: BTreeSet<String>,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) src_blocks: Vec<SrcBlockInfo>,
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) script_analysis: verter_analysis::ScriptAnalysisSnapshot,
    pub(crate) export_signatures: Vec<verter_analysis::ExportSignature>,
    pub(crate) style_analyses: Vec<verter_analysis::StyleBlockAnalysis>,
    /// Per-dep, per-type resolved type shape hash for Tier 3 precision.
    /// Key: (dep_canonical_id, type_name). Value: hash of resolved prop shape.
    pub(crate) resolved_type_hashes: HashMap<(String, String), Hash16>,
    pub(crate) style_overrides: HashMap<u64, StyleOverrideLayer>,
    pub(crate) compile_slots: HashMap<u64, CompileSlot>,
    pub(crate) latest_diagnostics: HashMap<u64, DiagnosticsSnapshot>,
    pub(crate) generation: u64,
}

impl FileMeta {
    pub(crate) fn virtual_nodes(&self) -> Vec<VirtualNodeKind> {
        let mut nodes = vec![VirtualNodeKind::Main];
        if self.has_script || self.has_scoped_style {
            // Include Script for template-only components with scoped styles:
            // the compiler emits a synthetic script block with __scopeId.
            nodes.push(VirtualNodeKind::Script);
        }
        if self.has_template {
            nodes.push(VirtualNodeKind::Template);
        }
        for i in 0..self.style_langs.len() {
            nodes.push(VirtualNodeKind::Style { index: i });
        }
        for i in 0..self.custom_types.len() {
            nodes.push(VirtualNodeKind::Custom { index: i });
        }
        nodes
    }
}

impl FileEntry {
    pub(crate) fn all_virtual_nodes(&self) -> Vec<VirtualNodeKind> {
        self.meta.virtual_nodes()
    }
}

/// Point-in-time snapshot of host performance metrics.
///
/// Only available when the `host_metrics` feature is enabled.
/// Obtained via [`VerterHost::metrics_snapshot`](crate::VerterHost::metrics_snapshot).
#[derive(Debug, Default)]
#[cfg(feature = "host_metrics")]
pub struct HostMetricsSnapshot {
    /// Total number of `upsert()` calls.
    pub upserts: u64,
    /// Total number of compile requests (cache misses that triggered compilation).
    pub compile_requests: u64,
    /// Number of compile requests served from cache.
    pub compile_cache_hits: u64,
    /// Cache hit rate (0.0 to 1.0).
    pub compile_cache_hit_rate: f64,
    /// Total number of `get_virtual_file()` calls.
    pub virtual_loads: u64,
    /// Total number of `resolve()` calls.
    pub resolves: u64,
    /// Total number of `apply_style_overrides()` calls.
    pub style_override_calls: u64,
    /// Cumulative time spent in parse/hash phase across all upserts (microseconds).
    pub slice_hash_time_us_total: u64,
    /// Average parse/hash time per upsert (microseconds).
    pub avg_slice_hash_time_us: f64,
    /// Cumulative compilation time across all compiles (microseconds).
    pub compile_time_us_total: u64,
    /// Cumulative compilation time broken down by profile hash (microseconds).
    pub compile_time_us_total_by_profile: BTreeMap<u64, u64>,
    /// Number of compilations broken down by profile hash.
    pub compile_count_by_profile: BTreeMap<u64, u64>,
}

#[derive(Debug, Default)]
#[cfg(feature = "host_metrics")]
pub(crate) struct HostMetrics {
    pub(crate) upserts: std::sync::atomic::AtomicU64,
    pub(crate) compile_requests: std::sync::atomic::AtomicU64,
    pub(crate) compile_cache_hits: std::sync::atomic::AtomicU64,
    pub(crate) virtual_loads: std::sync::atomic::AtomicU64,
    pub(crate) resolves: std::sync::atomic::AtomicU64,
    pub(crate) style_override_calls: std::sync::atomic::AtomicU64,
    pub(crate) slice_hash_time_us_total: std::sync::atomic::AtomicU64,
    pub(crate) compile_time_us_total: std::sync::atomic::AtomicU64,
    pub(crate) compile_time_us_total_by_profile: std::sync::Mutex<HashMap<u64, u64>>,
    pub(crate) compile_count_by_profile: std::sync::Mutex<HashMap<u64, u64>>,
}
