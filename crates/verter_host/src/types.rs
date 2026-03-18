#[cfg(feature = "host_metrics")]
use std::collections::BTreeMap;
#[cfg(any(not(feature = "scheduler"), test))]
use std::collections::BTreeSet;
#[cfg(feature = "host_metrics")]
use std::collections::HashMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;

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
///
/// **Deprecated**: Prefer [`AnalysisScope`](verter_analysis::AnalysisScope) bitflags
/// for fine-grained control. This enum is retained for FFI backwards compatibility.
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

impl AnalysisLevel {
    /// Convert this legacy level to the equivalent [`AnalysisScope`] flags.
    pub fn to_scope(self) -> verter_analysis::AnalysisScope {
        match self {
            Self::Full => verter_analysis::AnalysisScope::LSP,
            Self::Essential => verter_analysis::AnalysisScope::ESSENTIAL,
            Self::None => verter_analysis::AnalysisScope::NONE,
        }
    }
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
    ///
    /// **Deprecated**: Prefer `analysis_scope` for fine-grained control.
    /// If `analysis_scope` is `Some`, it takes precedence over this field.
    pub analysis_level: AnalysisLevel,

    /// Bitwise flags controlling which analysis passes run during upsert.
    ///
    /// When `Some`, takes precedence over `analysis_level`. When `None`,
    /// falls back to `analysis_level.to_scope()`.
    ///
    /// Use preset constants for common configurations:
    /// - [`AnalysisScope::BUILD`](verter_analysis::AnalysisScope::BUILD) — minimal for compilation
    /// - [`AnalysisScope::LSP`](verter_analysis::AnalysisScope::LSP) — full for IDE features
    /// - [`AnalysisScope::LINTER`](verter_analysis::AnalysisScope::LINTER) — for lint rules
    pub analysis_scope: Option<verter_analysis::AnalysisScope>,

    /// When `true`, `get_analysis()` enriches the snapshot with imported type
    /// information by resolving `macro_type_deps` through the workspace (VFS
    /// aliases, re-exports). Populates `prop_fields`/`emit_fields`/`slot_fields`
    /// on target macros and adds resolved types to `resolved_local_types`.
    ///
    /// Designed for component-meta consumers that need full cross-file type
    /// resolution without a TypeScript Program. Defaults to `false`.
    pub deep_macro_resolution_type: bool,
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
                ".d.ts".to_string(),
                ".d.mts".to_string(),
                ".d.cts".to_string(),
            ],
            analysis_level: AnalysisLevel::Full,
            analysis_scope: None,
            deep_macro_resolution_type: false,
        }
    }
}

impl HostConfig {
    /// Returns the effective analysis scope, preferring `analysis_scope`
    /// over the legacy `analysis_level` field.
    pub fn effective_scope(&self) -> verter_analysis::AnalysisScope {
        self.analysis_scope
            .unwrap_or_else(|| self.analysis_level.to_scope())
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
    /// Types module name for TSX helper imports (default `"$verter/types"`).
    pub types_module_name: Option<String>,
    /// Force Vapor mode codegen regardless of `<template vapor>` attribute.
    pub force_vapor: bool,
    /// Strip TypeScript type annotations from compiled output.
    pub force_js: bool,
    /// Generate source maps for compiled output.
    pub source_map: bool,
    /// Controls which compilation steps run.
    /// See [`verter_core::compile::CompileTarget`] for available flags and presets.
    pub target: verter_core::compile::CompileTarget,
    /// Embed `declare module "@verter/types"` in generated TSX.
    /// When `false` (default), relies on the real `@verter/types` package.
    pub embed_ambient_types: bool,
    /// Experimental: Enable conditional root generic narrowing.
    pub conditional_root_narrowing: bool,
    /// Experimental: strict slot children type checking.
    pub strict_slots: bool,
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
            types_module_name: None,
            force_vapor: false,
            force_js: false,
            source_map: false,
            target: verter_core::compile::CompileTarget::BUNDLER,
            embed_ambient_types: false,
            conditional_root_narrowing: false,
            strict_slots: false,
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

impl SliceChanges {
    /// Returns `true` when the only change is in style blocks — no script, template,
    /// structure, or descriptor changes. Useful for skipping TSGO sync since the
    /// generated TSX is unaffected by style-only edits.
    pub fn is_style_only(&self) -> bool {
        !self.script_changed
            && !self.template_changed
            && !self.structure_changed
            && !self.descriptor_changed
            && !self.style_indices_changed.is_empty()
    }
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

/// Summary of a single module reference found in a script block.
#[derive(Debug, Clone)]
pub struct ScriptModuleReference {
    /// Syntax form that introduced the reference.
    pub syntax: verter_analysis::ModuleReferenceSyntax,
    /// Import vs require semantics.
    pub semantics: verter_analysis::ModuleReferenceSemantics,
    /// Whether the site is declaration-level type-only.
    pub is_type_only: bool,
    /// Raw source text for the specifier expression.
    pub raw_text: String,
    /// Exact literal when statically known.
    pub literal_specifier: Option<String>,
    /// Finite set of literals when narrowed to a union.
    pub finite_specifiers: Vec<String>,
    /// Static prefix for dynamic expressions, if any.
    pub static_prefix: Option<String>,
    /// Static analyzability classification.
    pub analyzability: verter_analysis::ModuleReferenceAnalyzability,
    /// Span of the containing statement or call expression.
    pub span: verter_span::Span,
    /// Span of the specifier expression.
    pub expr_span: verter_span::Span,
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
    /// Module reference sites found in script blocks.
    pub module_references: Vec<ScriptModuleReference>,
    /// Blocks that need external preprocessing before compilation.
    ///
    /// Non-empty when the SFC uses non-native languages (e.g., `<template lang="pug">`).
    /// The caller should invoke the appropriate preprocessor and feed results back
    /// via [`VerterHost::apply_block_overrides`](crate::VerterHost::apply_block_overrides).
    pub preprocessor_requests: Vec<PreprocessorRequest>,
    /// Export signatures extracted from the file's script block.
    /// For `.ts`/`.js` files these include re-export metadata for barrel file resolution.
    pub export_signatures: Vec<verter_analysis::ExportSignature>,
    /// Time spent in the parse phase (ms).
    pub parse_duration_ms: f64,
}

impl HostUpdateResult {
    /// Construct a no-op result for superseded upserts (scheduler mode).
    #[cfg(feature = "scheduler")]
    pub fn noop() -> Self {
        Self::no_change(String::new())
    }

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
            module_references: Vec::new(),
            preprocessor_requests: Vec::new(),
            export_signatures: Vec::new(),
            parse_duration_ms: 0.0,
        }
    }
}

/// Serializable snapshot of file analysis data, suitable for WASM export.
///
/// Returned by [`VerterHost::get_analysis`](crate::VerterHost::get_analysis).
/// Contains the combined script, style, and template analysis for an SFC.
///
/// Most fields are `Arc`-wrapped for cheap cloning — the underlying data is
/// shared between all snapshots of the same file version. Only `imports` and
/// `bindings` are owned `Vec`s because [`VerterHost::get_analysis`] mutates
/// them (import resolution and destructured binding enrichment).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisSnapshot {
    /// Import statements found in script blocks.
    /// Owned because `resolve_snapshot_imports` mutates `resolved_canonical_id`.
    pub imports: Vec<verter_analysis::AnalyzedImport>,
    /// Module reference sites found in script blocks.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub module_references: Arc<Vec<verter_analysis::AnalyzedModuleReference>>,
    /// Variable/function bindings declared in script blocks.
    /// Owned because `enrich_destructured_bindings` mutates `reactivity_kind`.
    pub bindings: Vec<verter_analysis::AnalyzedBinding>,
    /// Vue compiler macros used (defineProps, defineEmits, etc.).
    pub macros: Arc<Vec<verter_analysis::AnalyzedMacro>>,
    /// Type dependencies from macros that reference external files.
    pub macro_type_deps: Arc<Vec<verter_analysis::MacroTypeDep>>,
    /// Bitflags representing script characteristics (see `verter_analysis::ScriptFlags`).
    pub script_flags: u32,
    /// Per-style-block analysis (scoped, modules, v-bind usage).
    pub styles: Arc<Vec<verter_analysis::StyleBlockAnalysis>>,
    /// Template analysis (components, bindings, slots, refs, events).
    /// Present after compilation when template analysis scope flags are active.
    pub template: Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>>,
    /// Vue API call sites (lifecycle hooks, watchers, provide/inject, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub vue_api_calls: Arc<Vec<verter_analysis::types::VueApiCallSite>>,
    /// DOM query call sites (querySelector, getElementById, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub dom_query_calls: Arc<Vec<verter_analysis::types::DomQueryCallSite>>,

    /// CSS variable manipulations via DOM style APIs.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub css_var_manipulations: Arc<Vec<verter_analysis::types::CssVarManipulation>>,

    /// Script-side binding usage occurrences with exact spans.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub script_binding_occurrences: Arc<Vec<verter_analysis::types::ScriptBindingOccurrence>>,

    /// Export signatures extracted from the file's script block.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub export_signatures: Arc<Vec<verter_analysis::ExportSignature>>,

    /// Options API analysis (`export default { ... }` or `export default defineComponent({ ... })`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_api: Option<verter_analysis::AnalyzedOptionsApi>,

    /// Store usage sites (Pinia, Vuex, convention-based composables).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub store_usages: Arc<Vec<verter_analysis::types::StoreUsage>>,
    /// Store definitions (defineStore, createStore, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub store_definitions: Arc<Vec<verter_analysis::types::StoreDefinition>>,

    /// Whether the script block uses TypeScript (`lang="ts"`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_typescript: bool,
}

/// Compile-time dependencies that must be available before a Vue SFC can codegen.
#[derive(Debug, Clone, Default)]
pub struct CompileBlockersSnapshot {
    /// External `src="..."` blocks referenced by the SFC.
    pub external_source_requests: Vec<ExternalSourceRequest>,
    /// Macro type dependencies referenced from the SFC script.
    pub macro_type_deps: Arc<Vec<verter_analysis::MacroTypeDep>>,
}

/// A fully resolved export after following re-export chains.
///
/// Returned by [`VerterHost::resolve_exports`](crate::VerterHost::resolve_exports).
#[derive(Debug, Clone)]
pub struct ResolvedExport {
    /// Exported name as seen by importers.
    pub name: String,
    /// Whether this is a type-only export.
    pub is_type: bool,
    /// Ultimate source file canonical ID. `None` for local exports.
    pub source_canonical_id: Option<String>,
    /// Name in the ultimate source file (may differ, e.g. `"default"` → `"Button"`).
    pub source_name: String,
}

/// Helper for `skip_serializing_if` on `Arc<Vec<T>>`.
fn arc_vec_is_empty<T>(v: &Arc<Vec<T>>) -> bool {
    v.is_empty()
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

/// Discriminates the SFC block type that needs external preprocessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocessorBlockType {
    Template,
    Script,
    Style,
    Custom,
}

/// A block that needs external preprocessing before compilation.
///
/// Returned in [`HostUpdateResult::preprocessor_requests`] when a block uses
/// a non-native `lang` attribute (e.g., `<template lang="pug">` or
/// `<script lang="coffee">`). The caller invokes the appropriate external
/// compiler and feeds the result back via
/// [`VerterHost::apply_block_overrides`](crate::VerterHost::apply_block_overrides).
#[derive(Debug, Clone)]
pub struct PreprocessorRequest {
    /// Which block type needs preprocessing.
    pub block_type: PreprocessorBlockType,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: usize,
    /// The `lang` attribute value (e.g., `"pug"`, `"coffee"`, `"scss"`).
    pub lang: String,
    /// Raw content of the block that needs preprocessing.
    pub content: String,
}

/// A single preprocessed block override to inject into the compile pipeline.
///
/// Used in [`BlockOverrideRequest`] to provide the preprocessed output for
/// a template, script, style, or custom block.
#[derive(Debug, Clone)]
pub struct BlockOverrideEntry {
    /// Which block type this override applies to.
    pub block_type: PreprocessorBlockType,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: usize,
    /// Preprocessed code (HTML for template, JS for script, CSS for style).
    pub code: Arc<str>,
    /// Source map from the preprocessor, if available.
    pub source_map: Option<Arc<str>>,
}

/// Input to [`VerterHost::apply_block_overrides`](crate::VerterHost::apply_block_overrides).
///
/// Unified API for applying preprocessed block overrides. Replaces the
/// deprecated [`StyleOverrideRequest`] for new code.
#[derive(Debug, Clone)]
pub struct BlockOverrideRequest {
    /// Canonical ID of the file whose blocks are being overridden.
    pub canonical_id: String,
    /// Compile profile to apply the overrides under.
    pub compile_profile: CompileProfile,
    /// Preprocessed block overrides to inject.
    pub overrides: Vec<BlockOverrideEntry>,
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
    /// SFC-absolute byte offset span, if available.
    pub span: Option<verter_span::Span>,
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
    /// A scheduler error occurred.
    #[cfg(feature = "scheduler")]
    #[error("scheduler error: {0}")]
    Scheduler(#[from] verter_scheduler::job::SchedulerError),
    /// The request was superseded by a newer version of the file.
    #[cfg(feature = "scheduler")]
    #[error("request superseded by newer generation")]
    Superseded,
    /// The scheduler was shut down.
    #[cfg(feature = "scheduler")]
    #[error("scheduler shut down")]
    Shutdown,
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
    /// Template lang attribute value (e.g., `"pug"`). `None` for native HTML.
    /// Stored for preprocessor request generation; read in tests.
    #[allow(dead_code)]
    pub(crate) template_lang: Option<String>,
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
    /// Blocks that need external preprocessing (non-native `lang` attributes).
    pub(crate) preprocessor_requests: Vec<PreprocessorRequest>,
}

#[derive(Debug, Clone)]
pub(crate) struct StyleOverrideLayer {
    pub(crate) hash: u64,
    pub(crate) by_index: FxHashMap<usize, StyleOverrideEntry>,
}

/// Preprocessed template/script content that replaces the original block
/// content before compilation. Stored per compile-profile.
#[derive(Debug, Clone)]
pub(crate) struct ContentOverride {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
}

/// Per-profile layer of content overrides for template and/or script blocks.
/// The `template` and `script` fields store the preprocessor output for future
/// source map chain support; currently only `hash` is read by the compile pipeline.
#[derive(Debug, Clone)]
pub(crate) struct ContentOverrideLayer {
    pub(crate) hash: u64,
    #[allow(dead_code)]
    pub(crate) template: Option<ContentOverride>,
    #[allow(dead_code)]
    pub(crate) script: Option<ContentOverride>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedVirtualFile {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    pub(crate) lang: Option<String>,
    pub(crate) meta: VirtualMeta,
}

/// Cached IDE output for LSP type checking, stored separately from virtual files.
#[derive(Debug, Clone)]
pub(crate) struct CachedTsx {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    pub(crate) is_jsx: bool,
    pub(crate) destructured_block: Option<verter_core::compile::types::DestructuredBlockMeta>,
}

/// Response from [`VerterHost::get_ide`].
#[derive(Debug, Clone)]
pub struct IdeResponse {
    /// The generated TSX/JSX code.
    pub code: Arc<str>,
    /// JSON source map (if available).
    pub source_map: Option<Arc<str>>,
    /// `true` for JavaScript SFCs (.jsx output), `false` for TypeScript (.tsx output).
    pub is_jsx: bool,
    /// Structured metadata for the destructured block region, if present.
    pub destructured_block: Option<verter_core::compile::types::DestructuredBlockMeta>,
}

/// Response from [`VerterHost::get_public_api`].
///
/// Contains minimal TypeScript declarations generated by macro-only extraction
/// (defineProps, defineEmits, defineModel, defineOptions). No template
/// compilation is performed.
#[derive(Debug, Clone)]
pub struct TscResponse {
    /// The generated TSC code (ComponentPublicInstance-based declaration).
    /// Includes an inline `//# sourceMappingURL=` at the end.
    pub code: Arc<str>,
    /// JSON source map (always present — embedded inline in `code`).
    pub source_map: Option<Arc<str>>,
}

/// Controls which public API surface the host generates for a Vue SFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicApiMode {
    /// The normal public instance surface used by application code.
    Public,
    /// A Vue Test Utils-like debug surface that exposes `<script setup>` bindings.
    Testing,
}

#[derive(Debug, Clone)]
pub(crate) struct CompileSlot {
    pub(crate) semantic_hash: Hash16,
    pub(crate) style_override_hash: u64,
    pub(crate) content_override_hash: u64,
    pub(crate) outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
    pub(crate) diagnostics: DiagnosticsSnapshot,
    pub(crate) last_good_outputs: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
    #[cfg_attr(feature = "scheduler", allow(dead_code))]
    pub(crate) last_access_tick: u64,
    /// Combined TSX output for LSP type checking. Not a virtual file.
    pub(crate) tsx: Option<CachedTsx>,
    /// Template analysis extracted during compilation. Populated when
    /// the analysis scope includes template flags (TPL_COMPONENTS, etc.).
    /// Stored per-slot for future per-profile access; the latest is also
    /// copied to `FileEntry::template_analysis` for the public API.
    #[allow(dead_code)]
    pub(crate) template_analysis: Option<verter_analysis::template::TemplateAnalysisSnapshot>,
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
    /// Content overrides for preprocessed template/script blocks.
    pub(crate) content_override_layer: Option<ContentOverrideLayer>,
    /// Macro type dependencies for cross-file type resolution.
    pub(crate) macro_type_deps: Vec<verter_analysis::MacroTypeDep>,
    /// Import declarations from the SFC script analysis.
    /// Used to attach precise spans to unresolved compile blockers.
    pub(crate) script_imports: Vec<verter_analysis::AnalyzedImport>,
    /// Macro calls from the effective script analysis.
    /// Used when converting template compiler metadata into host analysis.
    pub(crate) script_macros: Vec<verter_analysis::AnalyzedMacro>,
    /// Local/exported bindings from the effective script analysis.
    /// Used when converting template compiler metadata into host analysis.
    pub(crate) script_bindings: Vec<verter_analysis::AnalyzedBinding>,
    /// Cached parsed SFC from upsert, reused during compilation to avoid re-parsing.
    pub(crate) cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
    /// Binding names referenced in style `v-bind()` expressions.
    /// Extracted from `FileEntry.style_analyses` at cache-miss time.
    pub(crate) style_v_bind_vars: Vec<String>,
}

/// Cached Arc-wrapped views of immutable `ScriptAnalysisSnapshot` fields.
///
/// Built once during upsert and shared across all `get_analysis()` calls.
/// These fields are never mutated after construction, so Arc sharing is safe.
/// On the scheduler path, `AnalysisArcs` in `HostAnalysisData` serves this role.
#[cfg(any(not(feature = "scheduler"), test))]
#[derive(Debug, Clone, Default)]
pub(crate) struct ScriptAnalysisArcs {
    pub(crate) module_references: Arc<Vec<verter_analysis::AnalyzedModuleReference>>,
    pub(crate) macros: Arc<Vec<verter_analysis::AnalyzedMacro>>,
    pub(crate) macro_type_deps: Arc<Vec<verter_analysis::MacroTypeDep>>,
    pub(crate) vue_api_calls: Arc<Vec<verter_analysis::types::VueApiCallSite>>,
    pub(crate) dom_query_calls: Arc<Vec<verter_analysis::types::DomQueryCallSite>>,
    pub(crate) css_var_manipulations: Arc<Vec<verter_analysis::types::CssVarManipulation>>,
    pub(crate) script_binding_occurrences:
        Arc<Vec<verter_analysis::types::ScriptBindingOccurrence>>,
    pub(crate) store_usages: Arc<Vec<verter_analysis::types::StoreUsage>>,
    pub(crate) store_definitions: Arc<Vec<verter_analysis::types::StoreDefinition>>,
}

#[cfg(not(feature = "scheduler"))]
impl ScriptAnalysisArcs {
    /// Build Arc-wrapped caches from a script analysis snapshot.
    pub(crate) fn from_analysis(sa: &verter_analysis::ScriptAnalysisSnapshot) -> Self {
        Self {
            module_references: Arc::new(sa.module_references.clone()),
            macros: Arc::new(sa.macros.clone()),
            macro_type_deps: Arc::new(sa.macro_type_deps.clone()),
            vue_api_calls: Arc::new(sa.vue_api_calls.clone()),
            dom_query_calls: Arc::new(sa.dom_query_calls.clone()),
            css_var_manipulations: Arc::new(sa.css_var_manipulations.clone()),
            script_binding_occurrences: Arc::new(sa.script_binding_occurrences.clone()),
            store_usages: Arc::new(sa.store_usages.clone()),
            store_definitions: Arc::new(sa.store_definitions.clone()),
        }
    }
}

/// Per-specifier resolution record for an import dependency.
///
/// Callers (unplugin, LSP, TS plugin) resolve import specifiers to canonical IDs
/// and pass these records to [`VerterHost::set_import_dependencies`]. The host uses
/// them for exact resolution instead of lossy basename/suffix heuristics.
#[derive(Debug, Clone)]
pub struct DependencyResolution {
    /// The raw import specifier as written in source (e.g., `@/components/base`).
    pub specifier: String,
    /// Exact resolved canonical ID, if the caller resolved it (e.g., `/src/components/base/index.ts`).
    pub resolved_canonical_id: Option<String>,
    /// Ordered candidate canonical IDs when exact resolution isn't available.
    /// First loaded candidate wins.
    pub possible_canonical_ids: Vec<String>,
}

#[cfg(any(not(feature = "scheduler"), test))]
#[derive(Debug, Clone)]
pub(crate) struct FileEntry {
    // ── Identity ──────────────────────────────────────────────────────
    pub(crate) canonical_id: String,
    pub(crate) file_kind: FileKind,
    pub(crate) aliases: BTreeSet<String>,
    pub(crate) generation: u64,

    // ── Content & hashes ──────────────────────────────────────────────
    pub(crate) source: Arc<str>,
    pub(crate) whole_hash: Hash16,
    pub(crate) semantic_hash: Hash16,
    pub(crate) slices: SliceHashes,
    pub(crate) descriptor: DescriptorMin,
    pub(crate) meta: FileMeta,

    // ── Dependencies ──────────────────────────────────────────────────
    pub(crate) dependencies: BTreeSet<String>,
    /// Per-specifier resolution records from the last `set_import_dependencies` call.
    /// Keyed by raw import specifier. Used by `resolve_loaded_dependency_canonical`
    /// for exact lookup before falling back to heuristics.
    pub(crate) dependency_resolutions: FxHashMap<String, DependencyResolution>,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) src_blocks: Vec<SrcBlockInfo>,
    /// Per-dep, per-type resolved type shape hash for Tier 3 precision.
    /// Key: (dep_canonical_id, type_name). Value: hash of resolved prop shape.
    pub(crate) resolved_type_hashes: FxHashMap<(String, String), Hash16>,

    // ── Analysis snapshots ────────────────────────────────────────────
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) script_analysis: verter_analysis::ScriptAnalysisSnapshot,
    pub(crate) export_signatures: Vec<verter_analysis::ExportSignature>,
    pub(crate) style_analyses: Arc<Vec<verter_analysis::StyleBlockAnalysis>>,
    /// Template analysis from the most recent compilation. Populated when
    /// the analysis scope includes template flags and the file has a template.
    pub(crate) template_analysis: Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>>,
    /// Cached Arc-wrapped script analysis fields for cheap snapshot cloning.
    /// Built once during upsert, shared across all `get_analysis()` calls.
    pub(crate) arc_script_cache: ScriptAnalysisArcs,

    // ── Compilation cache ─────────────────────────────────────────────
    pub(crate) style_overrides: FxHashMap<u64, StyleOverrideLayer>,
    /// Per-profile content overrides for preprocessed template/script blocks.
    pub(crate) content_overrides: FxHashMap<u64, ContentOverrideLayer>,
    pub(crate) compile_slots: FxHashMap<u64, CompileSlot>,
    pub(crate) latest_diagnostics: FxHashMap<u64, DiagnosticsSnapshot>,
    /// Monotonic counter incremented on every write to `latest_diagnostics`.
    /// Used by the LSP cache to detect host-driven recompiles that don't change
    /// the document version (e.g., dependency hydration clearing stale errors).
    pub(crate) diagnostics_generation: u64,
    /// Cached parsed SFC from upsert, reused during compilation to avoid re-parsing.
    pub(crate) cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
    /// Cached intermediate TSC extract state. Populated on first `get_public_api_with_mode`
    /// call and reused for subsequent calls with different external types or modes.
    /// Cleared on source change (semantic_hash mismatch during upsert).
    pub(crate) cached_tsc_extract: Option<Arc<verter_core::tsc::ExtractedTscState>>,
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

#[cfg(any(not(feature = "scheduler"), test))]
impl FileEntry {
    pub(crate) fn all_virtual_nodes(&self) -> Vec<VirtualNodeKind> {
        self.meta.virtual_nodes()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CompileCacheEntry — scheduler-backed compile state (native only)
// ═══════════════════════════════════════════════════════════════════════════

/// Per-file compile cache entry for the scheduler-backed host.
///
/// Three conceptual subdomains — kept in one struct for DashMap efficiency:
///
/// **ProfileState**: per-profile override + compile outputs
/// - `content_overrides`, `style_overrides`, `compile_slots`, `latest_diagnostics`
///
/// **DerivedRawState**: source-hash-keyed caches
/// - `cached_tsc_extract`, `raw_template_analysis`
///
/// **DependencyState**: resolution metadata + invalidation hashes
/// - `dependency_resolutions`, `dependencies`, `resolved_type_hashes`, `aliases`
#[cfg(feature = "scheduler")]
#[derive(Debug, Default)]
#[allow(dead_code)] // Fields used progressively during Phase 2 migration
pub(crate) struct CompileCacheEntry {
    // ── ProfileState: per-profile override + compile outputs ──
    pub(crate) content_overrides: FxHashMap<u64, ContentOverrideWithParse>,
    pub(crate) style_overrides: FxHashMap<u64, StyleOverrideWithAnalysis>,
    pub(crate) compile_slots: FxHashMap<u64, CompileSlot>,
    pub(crate) latest_diagnostics: FxHashMap<u64, DiagnosticsSnapshot>,
    pub(crate) diagnostics_generation: u64,

    // ── DerivedRawState: source-hash-keyed caches ──
    /// Cached TSC extract keyed by whole_hash. On read: stored hash must match
    /// effective_file_state().whole_hash. Cleared on upsert when whole_hash changes.
    pub(crate) cached_tsc_extract: Option<(Hash16, Arc<verter_core::tsc::ExtractedTscState>)>,

    /// Raw template analysis (source-derived, profileless).
    /// Computed by compute_template_analysis_if_missing() from raw scheduler data.
    /// Always raw — never from overrides.
    ///
    /// EXTERNAL SRC RULE: When src_blocks is non-empty, raw_template_analysis is NOT cached
    /// (set to None after read). This mirrors current FileEntry behavior because editing
    /// an external <template src>/<script src> dep only triggers smart_invalidate_dependents
    /// (which clears compile_slots), not raw_template_analysis.
    pub(crate) raw_template_analysis:
        Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>>,

    // ── DependencyState: resolution metadata + invalidation hashes ──
    pub(crate) dependency_resolutions: FxHashMap<String, DependencyResolution>,
    pub(crate) dependencies: std::collections::BTreeSet<String>,
    pub(crate) resolved_type_hashes: FxHashMap<(String, String), Hash16>,
    pub(crate) aliases: std::collections::BTreeSet<String>,
    pub(crate) generation: u64,

    /// Eviction flag — when true, the file is invisible to host accessors
    /// but deps/aliases are preserved for old-state diffing during reload.
    pub(crate) evicted: bool,
}

/// Override-aware file state returned by `effective_file_state()`.
///
/// Contains either the raw scheduler data or the content override's synthetic
/// data, depending on whether a block override exists for the requested profile.
#[cfg(feature = "scheduler")]
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields read progressively as accessors migrate
pub(crate) struct EffectiveFileState {
    pub(crate) source: std::sync::Arc<str>,
    pub(crate) meta: FileMeta,
    pub(crate) script_analysis: verter_analysis::ScriptAnalysisSnapshot,
    pub(crate) cached_parse: Option<std::sync::Arc<verter_core::parser::types::ParsedSfc>>,
    pub(crate) whole_hash: Hash16,
}

/// Block override + cached re-parse from synthetic source.
///
/// When a preprocessor (e.g. Pug → HTML) overrides a block, the host builds a
/// synthetic SFC source, re-parses it, and stores the result here. The scheduler's
/// raw source/analysis are never modified by overrides.
#[cfg(feature = "scheduler")]
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in Phase 2a: apply_block_overrides
pub(crate) struct ContentOverrideWithParse {
    pub(crate) layer: ContentOverrideLayer,
    pub(crate) parse: ParseSnapshot,
    pub(crate) cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
    pub(crate) source: Arc<str>,
}

/// Style override + remapped CSS analyses + lang overrides.
///
/// When a style preprocessor (e.g. SCSS → CSS) runs, the compiled CSS and its
/// remapped CSS analysis (with SFC-absolute spans) are stored here per-profile.
#[cfg(feature = "scheduler")]
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in Phase 2a: apply_style_overrides
pub(crate) struct StyleOverrideWithAnalysis {
    pub(crate) layer: StyleOverrideLayer,
    /// Per-index: Some(remapped CSS analysis) for overridden blocks, None for raw.
    pub(crate) analyses: Vec<Option<verter_analysis::StyleBlockAnalysis>>,
    /// Per-index: Some("css") for overridden blocks, None for raw.
    pub(crate) lang_overrides: Vec<Option<String>>,
    pub(crate) hash: u64,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// @ai-generated — SliceChanges::is_style_only unit tests
    #[test]
    fn is_style_only_true_when_only_styles_changed() {
        let sc = SliceChanges {
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(sc.is_style_only());
    }

    #[test]
    fn is_style_only_true_for_multiple_style_indices() {
        let sc = SliceChanges {
            style_indices_changed: vec![0, 2],
            ..SliceChanges::default()
        };
        assert!(sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_when_nothing_changed() {
        let sc = SliceChanges::default();
        assert!(!sc.is_style_only(), "no changes at all is not style-only");
    }

    #[test]
    fn is_style_only_false_when_script_also_changed() {
        let sc = SliceChanges {
            script_changed: true,
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_when_template_also_changed() {
        let sc = SliceChanges {
            template_changed: true,
            style_indices_changed: vec![1],
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_when_structure_changed() {
        let sc = SliceChanges {
            structure_changed: true,
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(
            !sc.is_style_only(),
            "structure change means blocks were added/removed"
        );
    }

    #[test]
    fn is_style_only_false_when_descriptor_changed() {
        let sc = SliceChanges {
            descriptor_changed: true,
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(
            !sc.is_style_only(),
            "descriptor change (e.g. scoped added) affects compilation"
        );
    }

    #[test]
    fn is_style_only_false_for_script_only_change() {
        let sc = SliceChanges {
            script_changed: true,
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_for_template_only_change() {
        let sc = SliceChanges {
            template_changed: true,
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }
}
