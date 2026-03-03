// NAPI-RS generates variables from camelCase struct fields — suppress warnings.
#![allow(non_snake_case)]

//! # verter_napi — Node.js bindings for Verter
//!
//! NAPI-RS binding layer that exposes [`verter_host::VerterHost`] and
//! `processStyle` to Node.js.
//!
//! ## API parity
//!
//! This crate exposes the same `VerterHost` API as [`verter_wasm`] with
//! one addition that requires a Node.js runtime:
//!
//! - **`processStyle`** — applies scoped CSS, CSS Modules, and `v-bind()`
//!   replacement to preprocessed CSS (SCSS/Less/Stylus output).
//!
//! ## Performance
//!
//! Uses `#[napi(object)]` structs for zero-copy V8 ↔ Rust transfer.
//! All panics are caught via [`catch_panic`] to prevent Node.js crashes.
//!
//! ## FFI architecture
//!
//! NAPI structs use camelCase field names matching the JS API convention.
//! They map to/from `verter_ffi` types via zero-copy `From` impls
//! (field-by-field moves, no serialization). The shared conversion logic
//! in `verter_ffi::convert` handles the FFI ↔ host type mapping.

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use verter_ffi::convert::*;
use verter_ffi::types::*;
use verter_host as host;

// Re-imports for code actions and diagnostics (parity with verter_wasm)
use verter_actions::{ActionContext, ActionEngine};
use verter_diagnostics::rules::RuleRegistry;
use verter_diagnostics::Linter;

/// Run a closure, converting any panic into a napi::Error.
/// Prevents Rust panics from crashing the Node.js process.
fn catch_panic<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T> {
    std::panic::catch_unwind(f).map_err(|panic_info| {
        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown internal error".to_string()
        };
        Error::new(
            Status::GenericFailure,
            format!("internal compiler error: {msg}"),
        )
    })
}

fn ffi_err(msg: impl std::fmt::Display) -> Error {
    Error::new(Status::InvalidArg, msg.to_string())
}

/// Convert a `Buffer` (raw bytes) to a `String`, validating UTF-8.
fn buffer_to_string(buf: Buffer) -> Result<String> {
    String::from_utf8(buf.into()).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("Buffer is not valid UTF-8: {e}"),
        )
    })
}

fn host_error(err: host::HostError) -> Error {
    let status = match &err {
        host::HostError::InvalidQuery
        | host::HostError::MissingSource { .. }
        | host::HostError::MissingVirtualNode { .. } => Status::InvalidArg,
        host::HostError::CompileError { .. } => Status::GenericFailure,
    };
    Error::new(status, host_error_to_string(&err))
}

// =============================================================================
// Standalone CSS Style Processing (NAPI-only)
//
// Available in NAPI but not WASM because CSS preprocessing (LESS/SCSS/Stylus)
// requires Node.js. The WASM host processes styles inline during compilation.
// =============================================================================

#[napi(object)]
#[derive(Default)]
pub struct ProcessStyleOptions {
    /// Scope ID string (e.g., "a4f2eed6")
    pub scopeId: String,
    /// Whether this style block is scoped
    pub scoped: Option<bool>,
    /// Whether this is a CSS module block
    pub isModule: Option<bool>,
    /// Custom module name (None = "$style")
    pub moduleName: Option<String>,
    /// Source filename for source map generation
    pub filename: Option<String>,
    /// Whether to generate source maps
    pub sourcemap: Option<bool>,
}

#[napi(object)]
pub struct ProcessStyleVBind {
    /// The original expression text (e.g., "color" or "theme.color")
    pub expression: String,
    /// The generated CSS variable name (e.g., "--a4f2eed6-color")
    pub varName: String,
}

#[napi(object)]
pub struct ProcessStyleResult {
    /// Transformed CSS code
    pub code: String,
    /// Source map as JSON string (if sourcemap was requested)
    pub sourceMap: Option<String>,
    /// CSS module class mappings (original → hashed), each entry is [original, hashed]
    pub moduleClasses: Vec<Vec<String>>,
    /// CSS module variable name (e.g. "$style" or custom name from `<style module="...">`)
    pub moduleName: Option<String>,
    /// v-bind() expressions found and replaced
    pub vBindVars: Vec<ProcessStyleVBind>,
}

/// Process a CSS style block: apply scoping, CSS modules, and v-bind replacement.
///
/// Called by the Vite plugin after preprocessing SCSS/Less/Stylus to valid CSS.
/// For plain CSS blocks, the Rust compiler handles this inline during compileForVite().
///
/// @param css - Valid CSS string (already preprocessed if originally SCSS/Less/etc.)
/// @param options - Processing options (scope ID, scoped, modules, etc.)
/// @returns Processed CSS with scoping/modules applied, plus v-bind metadata
#[napi]
pub fn process_style(css: Buffer, options: ProcessStyleOptions) -> Result<ProcessStyleResult> {
    let css = buffer_to_string(css)?;
    catch_panic(std::panic::AssertUnwindSafe(|| {
        let core_options = verter_core::css::ProcessStyleOptions {
            scope_id: &options.scopeId,
            scoped: options.scoped.unwrap_or(false),
            is_module: options.isModule.unwrap_or(false),
            module_name: options.moduleName.as_deref(),
            filename: options.filename.as_deref(),
            sourcemap: options.sourcemap.unwrap_or(false),
        };

        verter_core::css::process_style(&css, &core_options)
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))
    }))?
    .map(|result| ProcessStyleResult {
        code: result.code,
        sourceMap: result.source_map,
        moduleClasses: result
            .module_classes
            .into_iter()
            .map(|(k, v)| vec![k, v])
            .collect(),
        moduleName: result.module_name,
        vBindVars: result
            .v_bind_vars
            .into_iter()
            .map(|v| ProcessStyleVBind {
                expression: v.expression,
                varName: v.var_name,
            })
            .collect(),
    })
}

// =============================================================================
// NAPI ↔ FFI zero-copy boundary structs
//
// These use camelCase field names for JS convention. They map to/from
// verter_ffi types via From impls (field-by-field moves, zero allocation).
// =============================================================================

#[napi(object)]
#[derive(Default)]
pub struct NapiHostConfig {
    pub devMode: Option<bool>,
    pub compileErrorPolicy: Option<String>,
    pub lspScheme: Option<String>,
    pub maxProfilesPerFile: Option<u32>,
    pub resolveExtensions: Option<Vec<String>>,
    pub analysisLevel: Option<String>,
}

impl From<NapiHostConfig> for FfiHostConfig {
    fn from(n: NapiHostConfig) -> Self {
        Self {
            dev_mode: n.devMode,
            compile_error_policy: n.compileErrorPolicy,
            lsp_scheme: n.lspScheme,
            max_profiles_per_file: n.maxProfilesPerFile,
            resolve_extensions: n.resolveExtensions,
            analysis_level: n.analysisLevel,
        }
    }
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiCompileProfile {
    pub filename: Option<String>,
    pub isProduction: Option<bool>,
    pub ssr: Option<bool>,
    pub hmrStrategy: Option<String>,
    pub componentId: Option<String>,
    pub delimiters: Option<Vec<String>>,
    pub customElements: Option<Vec<String>>,
    pub comments: Option<bool>,
    pub runtimeModuleName: Option<String>,
    pub typesModuleName: Option<String>,
    pub forceVapor: Option<bool>,
    pub forceJs: Option<bool>,
    pub sourceMap: Option<bool>,
    /// Compilation target preset: "bundler" (default), "ide", or "analysis".
    pub target: Option<String>,
}

impl From<NapiCompileProfile> for FfiCompileProfile {
    fn from(n: NapiCompileProfile) -> Self {
        Self {
            filename: n.filename,
            is_production: n.isProduction,
            ssr: n.ssr,
            hmr_strategy: n.hmrStrategy,
            component_id: n.componentId,
            delimiters: n.delimiters,
            custom_elements: n.customElements,
            comments: n.comments,
            runtime_module_name: n.runtimeModuleName,
            types_module_name: n.typesModuleName,
            force_vapor: n.forceVapor,
            force_js: n.forceJs,
            source_map: n.sourceMap,
            target: n.target,
        }
    }
}

#[napi(object)]
#[derive(Clone)]
pub struct NapiVirtualNodeKind {
    pub kind: String,
    pub index: Option<u32>,
}

impl From<NapiVirtualNodeKind> for FfiVirtualNodeKind {
    fn from(n: NapiVirtualNodeKind) -> Self {
        Self {
            kind: n.kind,
            index: n.index,
        }
    }
}

impl From<FfiVirtualNodeKind> for NapiVirtualNodeKind {
    fn from(f: FfiVirtualNodeKind) -> Self {
        Self {
            kind: f.kind,
            index: f.index,
        }
    }
}

#[napi(object)]
pub struct NapiUpsertRequest {
    pub canonicalId: Option<String>,
    pub inputId: String,
    /// SFC source code as UTF-8 bytes (e.g., `fs.readFileSync(path)`).
    pub source: Buffer,
    pub fileKind: Option<String>,
    pub aliases: Option<Vec<String>>,
}

#[napi(object)]
pub struct NapiStyleOverrideEntry {
    pub index: u32,
    /// Preprocessed CSS as UTF-8 bytes.
    pub code: Buffer,
    pub sourceMap: Option<String>,
}

#[napi(object)]
pub struct NapiStyleOverrideRequest {
    pub canonicalId: String,
    pub compileProfile: Option<NapiCompileProfile>,
    pub overrides: Vec<NapiStyleOverrideEntry>,
}

#[napi(object)]
pub struct NapiVirtualQuery {
    pub rawId: Option<String>,
    pub canonicalId: Option<String>,
    pub nodeKind: Option<NapiVirtualNodeKind>,
    pub compileProfile: Option<NapiCompileProfile>,
}

impl From<NapiVirtualQuery> for FfiVirtualQuery {
    fn from(n: NapiVirtualQuery) -> Self {
        Self {
            raw_id: n.rawId,
            canonical_id: n.canonicalId,
            node_kind: n.nodeKind.map(Into::into),
            compile_profile: n.compileProfile.map(Into::into),
        }
    }
}

// --- Output structs (Rust → V8) ---

#[napi(object)]
pub struct NapiSliceChanges {
    pub scriptChanged: bool,
    pub templateChanged: bool,
    pub styleIndicesChanged: Vec<u32>,
    pub customIndicesChanged: Vec<u32>,
    pub structureChanged: bool,
    pub descriptorChanged: bool,
}

#[napi(object)]
pub struct NapiDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub spanStart: Option<u32>,
    pub spanEnd: Option<u32>,
}

#[napi(object)]
pub struct NapiDiagnosticsSnapshot {
    pub diagnostics: Vec<NapiDiagnostic>,
    pub hasErrors: bool,
}

#[napi(object)]
pub struct NapiExternalSourceRequest {
    pub ownerCanonicalId: String,
    pub blockKind: String,
    pub index: u32,
    pub specifier: String,
    pub resolvedCanonicalId: String,
}

#[napi(object)]
pub struct NapiScriptImportInfo {
    pub source: String,
    pub isTypeOnly: bool,
    pub bindings: Vec<String>,
}

#[napi(object)]
pub struct NapiPreprocessorRequest {
    /// Block type: "template", "script", "style", or "custom".
    pub blockType: String,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: u32,
    /// The `lang` attribute value (e.g., "pug", "coffee", "scss").
    pub lang: String,
    /// Raw content of the block that needs preprocessing.
    pub content: String,
}

#[napi(object)]
pub struct NapiBlockOverrideEntry {
    /// Block type: "template", "script", "style", or "custom".
    pub blockType: String,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: u32,
    /// Preprocessed code as UTF-8 bytes.
    pub code: Buffer,
    /// Source map from the preprocessor, if available.
    pub sourceMap: Option<String>,
}

#[napi(object)]
pub struct NapiBlockOverrideRequest {
    pub canonicalId: String,
    pub compileProfile: Option<NapiCompileProfile>,
    pub overrides: Vec<NapiBlockOverrideEntry>,
}

#[napi(object)]
pub struct NapiUpdateResult {
    pub canonicalId: String,
    pub changed: bool,
    pub sliceChanges: NapiSliceChanges,
    pub changedVirtualNodes: Vec<NapiVirtualNodeKind>,
    pub removedVirtualNodes: Vec<NapiVirtualNodeKind>,
    pub changedVirtualIds: Vec<String>,
    pub removedVirtualIds: Vec<String>,
    pub changedLspIds: Vec<String>,
    pub removedLspIds: Vec<String>,
    pub diagnostics: NapiDiagnosticsSnapshot,
    pub externalSourceRequests: Vec<NapiExternalSourceRequest>,
    pub importSpecifiers: Vec<NapiScriptImportInfo>,
    pub preprocessorRequests: Vec<NapiPreprocessorRequest>,
    pub parseDurationMs: f64,
}

#[napi(object)]
pub struct NapiResolvedId {
    pub canonicalId: String,
    pub nodeKind: NapiVirtualNodeKind,
    pub existsInHost: bool,
    pub bundlerId: String,
    pub lspId: String,
}

#[napi(object)]
pub struct NapiVirtualMeta {
    pub scopeId: Option<String>,
    pub blockType: Option<String>,
    pub styleIndex: Option<u32>,
    pub customIndex: Option<u32>,
}

#[napi(object)]
pub struct NapiVirtualFileResponse {
    pub id: String,
    pub code: String,
    pub sourceMap: Option<String>,
    pub lang: Option<String>,
    pub stale: bool,
    pub diagnostics: NapiDiagnosticsSnapshot,
    pub meta: NapiVirtualMeta,
}

/// IDE output for type checking (dedicated API, not a virtual file).
#[napi(object)]
pub struct NapiIdeResponse {
    pub code: String,
    pub sourceMap: Option<String>,
    pub isJsx: bool,
}

/// TSC output for TypeScript declaration generation (macro-extraction only).
#[napi(object)]
pub struct NapiTscResponse {
    pub code: String,
    pub sourceMap: Option<String>,
}

#[napi(object)]
pub struct NapiRemoveResult {
    pub canonicalId: String,
}

// --- Code action structs ---

#[napi(object)]
pub struct NapiTextEdit {
    pub spanStart: u32,
    pub spanEnd: u32,
    pub newText: String,
}

#[napi(object)]
pub struct NapiCodeAction {
    pub title: String,
    pub kind: String,
    pub edits: Vec<NapiTextEdit>,
    pub isPreferred: bool,
    pub diagnosticRule: Option<String>,
}

impl From<FfiCodeAction> for NapiCodeAction {
    fn from(f: FfiCodeAction) -> Self {
        Self {
            title: f.title,
            kind: f.kind,
            edits: f
                .edits
                .into_iter()
                .map(|e| NapiTextEdit {
                    spanStart: e.span_start,
                    spanEnd: e.span_end,
                    newText: e.new_text,
                })
                .collect(),
            isPreferred: f.is_preferred,
            diagnosticRule: f.diagnostic_rule,
        }
    }
}

// --- Lint rule metadata structs ---

#[napi(object)]
pub struct NapiLintRuleMetadata {
    pub name: String,
    pub category: String,
    pub defaultSeverity: String,
}

impl From<FfiLintRuleMetadata> for NapiLintRuleMetadata {
    fn from(f: FfiLintRuleMetadata) -> Self {
        Self {
            name: f.name,
            category: f.category,
            defaultSeverity: f.default_severity,
        }
    }
}

// --- Document symbol structs ---

#[napi(object)]
pub struct NapiDocumentSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: u32,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub selectionStart: u32,
    pub selectionEnd: u32,
    pub children: Vec<NapiDocumentSymbol>,
}

impl From<FfiDocumentSymbol> for NapiDocumentSymbol {
    fn from(f: FfiDocumentSymbol) -> Self {
        Self {
            name: f.name,
            detail: f.detail,
            kind: f.kind,
            spanStart: f.span_start,
            spanEnd: f.span_end,
            selectionStart: f.selection_start,
            selectionEnd: f.selection_end,
            children: f.children.into_iter().map(Into::into).collect(),
        }
    }
}

// --- CSS selector matching structs ---

#[napi(object)]
pub struct NapiElementMatch {
    pub tag: String,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub result: String,
}

impl From<FfiElementMatch> for NapiElementMatch {
    fn from(f: FfiElementMatch) -> Self {
        Self {
            tag: f.tag,
            spanStart: f.span_start,
            spanEnd: f.span_end,
            result: f.result,
        }
    }
}

#[napi(object)]
pub struct NapiSelectorMatchResult {
    pub selectorText: String,
    pub selectorStart: u32,
    pub selectorEnd: u32,
    pub matches: Vec<NapiElementMatch>,
}

impl From<FfiSelectorMatchResult> for NapiSelectorMatchResult {
    fn from(f: FfiSelectorMatchResult) -> Self {
        Self {
            selectorText: f.selector_text,
            selectorStart: f.selector_start,
            selectorEnd: f.selector_end,
            matches: f.matches.into_iter().map(Into::into).collect(),
        }
    }
}

// --- Lint diagnostic struct ---

#[napi(object)]
pub struct NapiLintDiagnostic {
    pub rule: String,
    pub category: String,
    pub severity: String,
    pub message: String,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub tags: Vec<String>,
    pub spanKind: String,
}

/// Point-in-time snapshot of host performance metrics.
///
/// Only populated when built with the `host_metrics` feature.
/// Obtain via [`NapiVerterHost::getMetrics`].
#[napi(object)]
pub struct NapiHostMetrics {
    /// Total number of `upsert()` calls.
    pub upserts: f64,
    /// Total compile requests (cache misses that triggered Rust compilation).
    pub compileRequests: f64,
    /// Compile requests served from cache.
    pub compileCacheHits: f64,
    /// Cache hit rate (0.0 – 1.0).
    pub compileCacheHitRate: f64,
    /// Total `getVirtualFile()` calls.
    pub virtualLoads: f64,
    /// Total `resolve()` calls.
    pub resolves: f64,
    /// Total `applyStyleOverrides()` calls.
    pub styleOverrideCalls: f64,
    /// Cumulative parse/hash time across all upserts (microseconds).
    pub sliceHashTimeUsTotal: f64,
    /// Average parse/hash time per upsert (microseconds).
    pub avgSliceHashTimeUs: f64,
    /// Cumulative Rust compilation time (microseconds).
    pub compileTimeUsTotal: f64,
}

// =============================================================================
// Direct Host → NAPI conversion (bypasses FFI intermediate types)
// =============================================================================

fn host_node_kind_to_napi(input: &host::VirtualNodeKind) -> NapiVirtualNodeKind {
    match input {
        host::VirtualNodeKind::Main => NapiVirtualNodeKind {
            kind: "main".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Script => NapiVirtualNodeKind {
            kind: "script".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Template => NapiVirtualNodeKind {
            kind: "template".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Style { index } => NapiVirtualNodeKind {
            kind: "style".to_string(),
            index: Some(*index as u32),
        },
        host::VirtualNodeKind::Custom { index } => NapiVirtualNodeKind {
            kind: "custom".to_string(),
            index: Some(*index as u32),
        },
    }
}

fn host_diagnostics_to_napi(
    input: &host::DiagnosticsSnapshot,
    source: Option<&str>,
) -> NapiDiagnosticsSnapshot {
    let ffi = host_diagnostics_to_ffi(input, source);
    NapiDiagnosticsSnapshot {
        diagnostics: ffi
            .diagnostics
            .into_iter()
            .map(|d| NapiDiagnostic {
                severity: d.severity,
                code: d.code,
                message: d.message,
                spanStart: d.span_start,
                spanEnd: d.span_end,
            })
            .collect(),
        hasErrors: ffi.has_errors,
    }
}

fn host_block_kind_to_str(kind: &host::ExternalBlockKind) -> &'static str {
    match kind {
        host::ExternalBlockKind::Script => "script",
        host::ExternalBlockKind::Template => "template",
        host::ExternalBlockKind::Style => "style",
        host::ExternalBlockKind::Custom => "custom",
    }
}

fn host_update_to_napi(input: host::HostUpdateResult, source: Option<&str>) -> NapiUpdateResult {
    NapiUpdateResult {
        canonicalId: input.canonical_id,
        changed: input.changed,
        sliceChanges: NapiSliceChanges {
            scriptChanged: input.slice_changes.script_changed,
            templateChanged: input.slice_changes.template_changed,
            styleIndicesChanged: input
                .slice_changes
                .style_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            customIndicesChanged: input
                .slice_changes
                .custom_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            structureChanged: input.slice_changes.structure_changed,
            descriptorChanged: input.slice_changes.descriptor_changed,
        },
        changedVirtualNodes: input
            .changed_virtual_nodes
            .iter()
            .map(host_node_kind_to_napi)
            .collect(),
        removedVirtualNodes: input
            .removed_virtual_nodes
            .iter()
            .map(host_node_kind_to_napi)
            .collect(),
        changedVirtualIds: input.changed_virtual_ids,
        removedVirtualIds: input.removed_virtual_ids,
        changedLspIds: input.changed_lsp_ids,
        removedLspIds: input.removed_lsp_ids,
        diagnostics: host_diagnostics_to_napi(&input.diagnostics, source),
        externalSourceRequests: input
            .external_source_requests
            .into_iter()
            .map(|req| NapiExternalSourceRequest {
                ownerCanonicalId: req.owner_canonical_id,
                blockKind: host_block_kind_to_str(&req.block_kind).to_string(),
                index: req.index as u32,
                specifier: req.specifier,
                resolvedCanonicalId: req.resolved_canonical_id,
            })
            .collect(),
        importSpecifiers: input
            .import_specifiers
            .into_iter()
            .map(|imp| NapiScriptImportInfo {
                source: imp.source,
                isTypeOnly: imp.is_type_only,
                bindings: imp.bindings,
            })
            .collect(),
        preprocessorRequests: input
            .preprocessor_requests
            .iter()
            .map(|req| NapiPreprocessorRequest {
                blockType: match req.block_type {
                    host::PreprocessorBlockType::Template => "template".to_string(),
                    host::PreprocessorBlockType::Script => "script".to_string(),
                    host::PreprocessorBlockType::Style => "style".to_string(),
                    host::PreprocessorBlockType::Custom => "custom".to_string(),
                },
                index: req.index as u32,
                lang: req.lang.clone(),
                content: req.content.clone(),
            })
            .collect(),
        parseDurationMs: input.parse_duration_ms,
    }
}

fn host_virtual_file_to_napi(
    input: host::VirtualFileResponse,
    source: Option<&str>,
) -> NapiVirtualFileResponse {
    NapiVirtualFileResponse {
        id: input.id,
        code: input.code.to_string(),
        sourceMap: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: host_diagnostics_to_napi(&input.diagnostics, source),
        meta: NapiVirtualMeta {
            scopeId: input.meta.scope_id,
            blockType: input.meta.block_type,
            styleIndex: input.meta.style_index.map(|i| i as u32),
            customIndex: input.meta.custom_index.map(|i| i as u32),
        },
    }
}

fn host_resolved_id_to_napi(input: host::ResolvedId) -> NapiResolvedId {
    NapiResolvedId {
        canonicalId: input.canonical_id,
        nodeKind: host_node_kind_to_napi(&input.node_kind),
        existsInHost: input.exists_in_host,
        bundlerId: input.bundler_id,
        lspId: input.lsp_id,
    }
}

// =============================================================================
// VerterHost (in-memory virtual file host)
//
// API parity with WASM (crates/verter_wasm):
// - Both: new, resolve, upsert, applyStyleOverrides, applyBlockOverrides,
//         getVirtualFile, listVirtualFiles, remove, setImportDependencies,
//         getAnalysis, getTsx, lint, getCodeActions, getLintRuleMetadata,
//         getDocumentSymbols, matchCssSelectors, computeCrossFileOptimizations
// - NAPI-only: processStyle (requires Node.js), getTsc, compileBatch, getMetrics
// =============================================================================

/// In-memory virtual file host for Vue SFC compilation.
///
/// Manages a collection of Vue SFCs and their compiled virtual files (script,
/// template, styles). Files are upserted as source, then lazily compiled into
/// virtual outputs that a bundler or LSP can request individually.
#[napi(js_name = "VerterHost")]
pub struct NapiVerterHost {
    inner: host::VerterHost,
}

#[napi]
impl NapiVerterHost {
    /// Creates a new `VerterHost` with the given configuration.
    ///
    /// - `config` — optional host settings (dev mode, compile error policy,
    ///   LSP scheme, analysis level, etc.). Defaults are used when `None`.
    ///
    /// Returns an error if the configuration contains invalid values (e.g. an
    /// unrecognised `compileErrorPolicy` string).
    #[napi(constructor)]
    pub fn new(config: Option<NapiHostConfig>) -> Result<Self> {
        let ffi_config: FfiHostConfig = config.unwrap_or_default().into();
        Ok(Self {
            inner: host::VerterHost::new(ffi_config_to_host(ffi_config).map_err(ffi_err)?),
        })
    }

    /// Resolves a raw import ID (e.g. `./Foo.vue?type=style&index=0`) into its
    /// canonical ID, virtual node kind, and bundler/LSP identifiers.
    ///
    /// Returns `None` if the ID does not match any file tracked by this host.
    #[napi]
    pub fn resolve(&self, raw_id: String) -> Result<Option<NapiResolvedId>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.resolve(&raw_id).map(host_resolved_id_to_napi)
        }))
    }

    /// Inserts or updates a file in the host.
    ///
    /// Parses the SFC source, diffs it against the previously stored version
    /// (if any), and returns a detailed changeset describing which virtual
    /// nodes changed, any diagnostics, and external source requests that the
    /// caller must resolve (e.g. `<script src="...">` references).
    ///
    /// - `request.inputId` — the file path used for import resolution.
    /// - `request.source` — SFC source as a UTF-8 `Buffer`.
    /// - `request.fileKind` — optional override (`"vue"` or `"ts"`); inferred
    ///   from extension when `None`.
    ///
    /// Returns an error if the source is not valid UTF-8 or if the file kind
    /// is unrecognised.
    #[napi]
    pub fn upsert(&self, request: NapiUpsertRequest) -> Result<NapiUpdateResult> {
        let source = buffer_to_string(request.source)?;
        let source_for_spans = source.clone();
        let ffi_req = FfiUpsertRequest {
            canonical_id: request.canonicalId,
            input_id: request.inputId,
            source,
            file_kind: request.fileKind,
            aliases: request.aliases,
        };
        let host_req = ffi_upsert_to_host(ffi_req).map_err(ffi_err)?;
        catch_panic(std::panic::AssertUnwindSafe(|| self.inner.upsert(host_req)))?
            .map(|result| host_update_to_napi(result, Some(source_for_spans.as_str())))
            .map_err(host_error)
    }

    /// Replaces one or more style blocks with preprocessed CSS (e.g. the
    /// output of SCSS/Less/Stylus) and recompiles affected virtual nodes.
    ///
    /// This is used by the Vite plugin after running a CSS preprocessor on
    /// style blocks that have a `lang` attribute. The host then applies
    /// scoping, CSS Modules, and `v-bind()` replacement on the preprocessed
    /// CSS.
    ///
    /// Returns the same changeset structure as [`upsert`](Self::upsert).
    ///
    /// Returns an error if the canonical ID is unknown or the override code
    /// is not valid UTF-8.
    #[napi(js_name = "applyStyleOverrides")]
    pub fn apply_style_overrides(
        &self,
        request: NapiStyleOverrideRequest,
    ) -> Result<NapiUpdateResult> {
        let canonical_for_source = request.canonicalId.clone();
        let overrides = request
            .overrides
            .into_iter()
            .map(|e| {
                Ok(FfiStyleOverrideEntry {
                    index: e.index,
                    code: buffer_to_string(e.code)?,
                    source_map: e.sourceMap,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let ffi_req = FfiStyleOverrideRequest {
            canonical_id: request.canonicalId,
            compile_profile: request.compileProfile.map(Into::into),
            overrides,
        };
        let host_req = ffi_style_override_to_host(ffi_req).map_err(ffi_err)?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.apply_style_overrides(host_req)
        }))?
        .map_err(host_error)?;
        let source = self.inner.get_source(&canonical_for_source);
        Ok(host_update_to_napi(result, source.as_deref()))
    }

    /// Replaces one or more blocks with preprocessed content (e.g. the output
    /// of Pug, CoffeeScript, SCSS, or custom block preprocessors) and
    /// recompiles affected virtual nodes.
    ///
    /// This is the unified API that handles template, script, style, AND
    /// custom block preprocessing. Style overrides are delegated to the
    /// existing style pipeline; template/script overrides build a synthetic
    /// SFC with preprocessed content and stripped `lang` attributes.
    ///
    /// Returns the same changeset structure as [`upsert`](Self::upsert).
    #[napi(js_name = "applyBlockOverrides")]
    pub fn apply_block_overrides(
        &self,
        request: NapiBlockOverrideRequest,
    ) -> Result<NapiUpdateResult> {
        let canonical_for_source = request.canonicalId.clone();
        let overrides = request
            .overrides
            .into_iter()
            .map(|e| {
                Ok(FfiBlockOverrideEntry {
                    block_type: e.blockType,
                    index: e.index,
                    code: buffer_to_string(e.code)?,
                    source_map: e.sourceMap,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let ffi_req = FfiBlockOverrideRequest {
            canonical_id: request.canonicalId,
            compile_profile: request.compileProfile.map(Into::into),
            overrides,
        };
        let host_req = ffi_block_override_to_host(ffi_req).map_err(ffi_err)?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.apply_block_overrides(host_req)
        }))?
        .map_err(host_error)?;
        let source = self.inner.get_source(&canonical_for_source);
        Ok(host_update_to_napi(result, source.as_deref()))
    }

    /// Retrieves a single compiled virtual file (script, template, or style).
    ///
    /// The query can identify the file by raw import ID or by canonical ID +
    /// node kind. A compile profile may be provided to control production
    /// mode, SSR, source maps, etc.
    ///
    /// Returns the compiled code, optional source map, language hint, and
    /// any compilation diagnostics.
    ///
    /// Returns an error if the query is invalid or the file is not found.
    #[napi(js_name = "getVirtualFile")]
    pub fn get_virtual_file(&self, query: NapiVirtualQuery) -> Result<NapiVirtualFileResponse> {
        let canonical_for_source = if let Some(canonical) = query.canonicalId.as_ref() {
            Some(canonical.clone())
        } else if let Some(raw_id) = query.rawId.as_ref() {
            self.inner.resolve(raw_id).map(|r| r.canonical_id)
        } else {
            None
        };
        let ffi_query: FfiVirtualQuery = query.into();
        let host_query = ffi_virtual_query_to_host(ffi_query).map_err(ffi_err)?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_virtual_file(host_query)
        }))?
        .map_err(host_error)?;
        let source = canonical_for_source
            .as_deref()
            .and_then(|canonical| self.inner.get_source(canonical));
        Ok(host_virtual_file_to_napi(result, source.as_deref()))
    }

    /// Lists all virtual node kinds for a given canonical file ID.
    ///
    /// Returns an array of node kinds (e.g. `main`, `script`, `template`,
    /// `style[0]`, `style[1]`, ...) that can be passed to
    /// [`get_virtual_file`](Self::get_virtual_file). Returns an empty array
    /// if the canonical ID is not tracked by the host.
    #[napi(js_name = "listVirtualFiles")]
    pub fn list_virtual_files(&self, canonical_id: String) -> Result<Vec<NapiVirtualNodeKind>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .list_virtual_files(&canonical_id)
                .iter()
                .map(host_node_kind_to_napi)
                .collect()
        }))
    }

    /// Removes a file from the host by its canonical ID or any registered alias.
    ///
    /// All associated virtual nodes and cached compilations are discarded.
    /// Returns `None` if no file matched the given ID.
    #[napi]
    pub fn remove(&self, canonical_or_alias: String) -> Result<Option<NapiRemoveResult>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .remove(&canonical_or_alias)
                .map(|r| NapiRemoveResult {
                    canonicalId: r.canonical_id,
                })
        }))
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    ///
    /// Returns `null` if the file doesn't exist in the host.
    /// When `analysis_level` is not "full", computes analysis on demand from stored source.
    ///
    /// **Note:** Returns a JSON *string* — the caller must `JSON.parse()`.
    /// The WASM variant (`verter_wasm`) returns a native JS object instead
    /// (via `serde_wasm_bindgen`). This inconsistency is intentional:
    /// defining NAPI structs for all `verter_analysis` types is high effort
    /// for low value since `getAnalysis` is primarily used by the playground.
    #[napi(js_name = "getAnalysis")]
    pub fn get_analysis(&self, canonical_or_alias: String) -> Result<Option<String>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))
        .map(|opt| {
            opt.map(|snapshot| {
                serde_json::to_string(&snapshot).map_err(|e| {
                    Error::new(
                        Status::GenericFailure,
                        format!("analysis serialization error: {e}"),
                    )
                })
            })
            .transpose()
        })?
    }

    /// Retrieves the combined TSX output for LSP type checking.
    ///
    /// This is a dedicated API separate from virtual files. IDE output is
    /// only consumed by the LSP, never by bundlers.
    ///
    /// Returns `{ code, sourceMap?, isJsx }` or `null` if no IDE output is available.
    #[napi(js_name = "getIde")]
    pub fn get_ide(
        &self,
        canonical_id: String,
        profile: Option<NapiCompileProfile>,
    ) -> Result<Option<NapiIdeResponse>> {
        let ffi_profile: Option<FfiCompileProfile> = profile.map(Into::into);
        let host_profile = ffi_profile_to_host(ffi_profile)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid profile: {e}")))?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_ide(&canonical_id, &host_profile)
        }))?;
        Ok(result.map(|r| NapiIdeResponse {
            code: r.code.to_string(),
            sourceMap: r.source_map.map(|s| s.to_string()),
            isJsx: r.is_jsx,
        }))
    }

    /// Generates TSC output (minimal TypeScript declarations) for a Vue SFC.
    ///
    /// Unlike `getTsx`, this does NOT require a prior `getVirtualFile` call.
    /// It performs macro-only extraction (defineProps, defineEmits, defineModel,
    /// defineOptions) and generates a `ComponentPublicInstance`-based declaration
    /// with inline source map. This is the fast path for IDE type checking.
    ///
    /// Returns `{ code, sourceMap? }` or `null` if no TSC output is available.
    #[napi(js_name = "getTsc")]
    pub fn get_tsc(&self, canonical_id: String) -> Result<Option<NapiTscResponse>> {
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_tsc(&canonical_id)
        }))?;
        Ok(result.map(|r| NapiTscResponse {
            code: r.code.to_string(),
            sourceMap: r.source_map.map(|s| s.to_string()),
        }))
    }

    /// Records the resolved import dependencies for a file.
    ///
    /// Called by the bundler plugin after resolving the `importSpecifiers`
    /// returned by [`upsert`](Self::upsert). This enables cross-file type
    /// resolution (e.g. following `import type { Props } from './types'`
    /// chains) when recompiling dependent files.
    ///
    /// - `canonical_or_alias` — the file whose dependencies are being set.
    /// - `resolved_deps` — canonical IDs of the resolved dependency files.
    #[napi(js_name = "setImportDependencies")]
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: String,
        resolved_deps: Vec<String>,
    ) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .set_import_dependencies(&canonical_or_alias, resolved_deps);
        }))
    }

    /// Compute cross-file prop constness optimizations.
    ///
    /// Builds a render tree from all compiled files and determines which
    /// child component props are const across all call sites.
    /// Returns JSON with `constPropOverrides`, `changedFiles`, and `diagnostics`.
    ///
    /// Call after all files are compiled (e.g., after `preCompile` loop).
    /// On subsequent calls, `changedFiles` lists only files whose constness
    /// changed since the last computation.
    #[napi(js_name = "computeCrossFileOptimizations")]
    pub fn compute_cross_file_optimizations(&self) -> Result<String> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.compute_cross_file_optimizations()
        }))
        .and_then(|result| {
            let ffi = host_cross_file_result_to_ffi(result);
            serde_json::to_string(&ffi).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("cross-file result serialization error: {e}"),
                )
            })
        })
    }

    /// Returns a snapshot of host performance metrics.
    ///
    /// Only available when built with the `host_metrics` feature.
    /// Returns `null` when the feature is disabled.
    #[napi(js_name = "getMetrics")]
    pub fn get_metrics(&self) -> Option<NapiHostMetrics> {
        #[cfg(feature = "host_metrics")]
        {
            let m = self.inner.metrics_snapshot();
            Some(NapiHostMetrics {
                upserts: m.upserts as f64,
                compileRequests: m.compile_requests as f64,
                compileCacheHits: m.compile_cache_hits as f64,
                compileCacheHitRate: m.compile_cache_hit_rate,
                virtualLoads: m.virtual_loads as f64,
                resolves: m.resolves as f64,
                styleOverrideCalls: m.style_override_calls as f64,
                sliceHashTimeUsTotal: m.slice_hash_time_us_total as f64,
                avgSliceHashTimeUs: m.avg_slice_hash_time_us,
                compileTimeUsTotal: m.compile_time_us_total as f64,
            })
        }
        #[cfg(not(feature = "host_metrics"))]
        {
            None
        }
    }

    /// Runs lint rules against a file's analysis data and returns diagnostics.
    ///
    /// Takes a canonical ID (or alias), retrieves its analysis data from the
    /// host, and runs the linter with the given config. Returns an array of
    /// lint diagnostics with UTF-16 spans.
    ///
    /// - `canonical_or_alias` — the file to lint.
    /// - `config` — optional JSON string with lint config. Pass `None` for defaults.
    #[napi]
    pub fn lint(
        &self,
        canonical_or_alias: String,
        config: Option<String>,
    ) -> Result<Vec<NapiLintDiagnostic>> {
        let lint_config = match config {
            Some(json) => serde_json::from_str::<verter_diagnostics::LintConfig>(&json)
                .map_err(|e| Error::new(Status::InvalidArg, format!("invalid lint config: {e}")))?,
            None => verter_diagnostics::LintConfig::default(),
        };

        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;

        let diagnostics = match analysis {
            Some(snapshot) => {
                let linter = Linter::new(lint_config);
                let script = build_script_snapshot(&snapshot);
                linter
                    .lint(Some(&script), snapshot.template.as_ref(), &snapshot.styles)
                    .into_diagnostics()
            }
            None => Vec::new(),
        };

        let source = self.inner.get_source(&canonical_or_alias);
        let ffi_diagnostics = lint_diagnostics_to_utf16(diagnostics, source.as_deref());

        Ok(ffi_diagnostics
            .into_iter()
            .map(|d| NapiLintDiagnostic {
                rule: d.rule,
                category: d.category,
                severity: match d.severity {
                    verter_diagnostics::Severity::Error => "error".to_string(),
                    verter_diagnostics::Severity::Warning => "warning".to_string(),
                    verter_diagnostics::Severity::Info => "info".to_string(),
                    verter_diagnostics::Severity::Hint => "hint".to_string(),
                },
                message: d.message,
                spanStart: d.span.start,
                spanEnd: d.span.end,
                tags: d.tags.iter().map(|t| format!("{:?}", t)).collect(),
                spanKind: format!("{:?}", d.span_kind),
            })
            .collect())
    }

    /// Returns code actions (quick fixes) available for a file at a given
    /// UTF-16 offset.
    ///
    /// Runs lint rules, then queries the action engine for fixes matching
    /// diagnostics at the given position. Returns an array of code actions
    /// with UTF-16 spans.
    ///
    /// - `canonical_or_alias` — the file to get actions for.
    /// - `offset` — UTF-16 cursor offset in the SFC source.
    #[napi(js_name = "getCodeActions")]
    pub fn get_code_actions(
        &self,
        canonical_or_alias: String,
        offset: u32,
    ) -> Result<Vec<NapiCodeAction>> {
        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;
        let source = self.inner.get_source(&canonical_or_alias);

        let actions = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => {
                let byte_offset = utf16_to_byte_offset(source, offset);
                let script = build_script_snapshot(&snapshot);
                let linter = Linter::default();
                let diag_set = linter.lint_with_source(
                    Some(&script),
                    snapshot.template.as_ref(),
                    &snapshot.styles,
                    Some(source),
                );

                let engine = ActionEngine::default();
                let ctx = ActionContext {
                    source,
                    file_id: &canonical_or_alias,
                    diagnostics: &diag_set,
                    template: snapshot.template.as_ref(),
                    script: Some(&script),
                    styles: &snapshot.styles,
                };

                let mut actions = Vec::new();
                for diag in diag_set.iter() {
                    if diag.span.start <= byte_offset && byte_offset <= diag.span.end {
                        actions.extend(engine.fixes_for(diag, &ctx));
                    }
                }
                actions.extend(engine.actions_at(byte_offset, &ctx));

                let mut seen = std::collections::HashSet::new();
                actions.retain(|a| seen.insert(a.title.clone()));

                actions
                    .iter()
                    .map(|a| code_action_to_ffi(a, source).into())
                    .collect::<Vec<NapiCodeAction>>()
            }
            _ => Vec::new(),
        };

        Ok(actions)
    }

    /// Returns metadata for all registered lint rules.
    ///
    /// Used by the lint rule browser UI to display available rules,
    /// their categories, and default severities.
    #[napi(js_name = "getLintRuleMetadata")]
    pub fn get_lint_rule_metadata(&self) -> Vec<NapiLintRuleMetadata> {
        let registry = RuleRegistry::default();
        registry
            .rules()
            .iter()
            .map(|rule| lint_rule_to_ffi_metadata(rule.as_ref()).into())
            .collect()
    }

    /// Returns document symbols for a file (outline / Ctrl+Shift+O).
    ///
    /// Generates a hierarchical tree of symbols: SFC blocks at the top,
    /// with script bindings, template components, and style classes as
    /// children. Returns an array of document symbols with UTF-16 spans.
    #[napi(js_name = "getDocumentSymbols")]
    pub fn get_document_symbols(
        &self,
        canonical_or_alias: String,
    ) -> Result<Vec<NapiDocumentSymbol>> {
        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;
        let source = self.inner.get_source(&canonical_or_alias);

        let symbols = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => {
                build_document_symbols_from_analysis(&snapshot, source)
                    .into_iter()
                    .map(Into::into)
                    .collect()
            }
            _ => Vec::new(),
        };

        Ok(symbols)
    }

    /// Matches CSS selectors against template elements, returning a
    /// three-valued match matrix.
    ///
    /// Each selector is tested against each template element, producing
    /// "match", "maybe", or "no" results. Used by the CSS selector
    /// matching visualization panel.
    #[napi(js_name = "matchCssSelectors")]
    pub fn match_css_selectors(
        &self,
        canonical_or_alias: String,
    ) -> Result<Vec<NapiSelectorMatchResult>> {
        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;
        let source = self.inner.get_source(&canonical_or_alias);

        let results = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => build_selector_match_results(&snapshot, source)
                .into_iter()
                .map(Into::into)
                .collect(),
            _ => Vec::new(),
        };

        Ok(results)
    }
}

// =============================================================================
// Shared helpers (parity with verter_wasm)
// =============================================================================

/// Build a `ScriptAnalysisSnapshot` from a `FileAnalysisSnapshot`.
///
/// Extracts all script-related fields, preserving `vue_api_calls` and
/// `dom_query_calls` from the snapshot.
fn build_script_snapshot(
    snapshot: &host::FileAnalysisSnapshot,
) -> verter_analysis::types::ScriptAnalysisSnapshot {
    verter_analysis::types::ScriptAnalysisSnapshot {
        imports: snapshot.imports.clone(),
        bindings: snapshot.bindings.clone(),
        macros: snapshot.macros.clone(),
        macro_type_deps: snapshot.macro_type_deps.clone(),
        flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(snapshot.script_flags),
        exported_functions: Vec::new(),
        vue_api_calls: snapshot.vue_api_calls.clone(),
        dom_query_calls: snapshot.dom_query_calls.clone(),
        first_await_offset: None,
        type_enhancements: None,
    }
}

/// Convert a UTF-16 offset to a UTF-8 byte offset.
fn utf16_to_byte_offset(source: &str, utf16_offset: u32) -> u32 {
    let mut utf16_count = 0u32;
    for (byte_idx, ch) in source.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx as u32;
        }
        utf16_count += ch.len_utf16() as u32;
    }
    source.len() as u32
}

/// Safe UTF-16 conversion that handles 0 as identity.
fn byte_offset_to_utf16_safe(source: &str, byte_offset: u32) -> u32 {
    if byte_offset == 0 || source.is_empty() {
        return 0;
    }
    let clamped = source.len().min(byte_offset as usize);
    source[..clamped].encode_utf16().count() as u32
}

/// Monaco SymbolKind constants (subset used for document symbols).
mod symbol_kind {
    pub const MODULE: u32 = 1;
    pub const VARIABLE: u32 = 12;
    pub const FUNCTION: u32 = 11;
    pub const CLASS: u32 = 4;
    pub const STRUCT: u32 = 22;
    pub const PROPERTY: u32 = 6;
    pub const KEY: u32 = 19;
}

/// Build document symbols from analysis data.
fn build_document_symbols_from_analysis(
    snapshot: &host::FileAnalysisSnapshot,
    source: &str,
) -> Vec<FfiDocumentSymbol> {
    let mut symbols = Vec::new();

    if !snapshot.bindings.is_empty() || !snapshot.imports.is_empty() || !snapshot.macros.is_empty()
    {
        let mut children = Vec::new();

        for imp in &snapshot.imports {
            children.push(FfiDocumentSymbol {
                name: imp.source.clone(),
                detail: if imp.is_type_only {
                    Some("type import".to_string())
                } else {
                    None
                },
                kind: symbol_kind::MODULE,
                span_start: 0,
                span_end: 0,
                selection_start: 0,
                selection_end: 0,
                children: Vec::new(),
            });
        }

        for binding in &snapshot.bindings {
            let kind = match binding.kind {
                verter_analysis::AnalyzedBindingKind::Function
                | verter_analysis::AnalyzedBindingKind::AsyncFunction => symbol_kind::FUNCTION,
                verter_analysis::AnalyzedBindingKind::Class => symbol_kind::CLASS,
                _ => symbol_kind::VARIABLE,
            };
            children.push(FfiDocumentSymbol {
                name: binding.name.clone(),
                detail: binding.type_annotation.clone(),
                kind,
                span_start: byte_offset_to_utf16_safe(source, binding.span.start),
                span_end: byte_offset_to_utf16_safe(source, binding.span.end),
                selection_start: byte_offset_to_utf16_safe(source, binding.span.start),
                selection_end: byte_offset_to_utf16_safe(source, binding.span.end),
                children: Vec::new(),
            });
        }

        for m in &snapshot.macros {
            children.push(FfiDocumentSymbol {
                name: format!("{:?}", m.kind),
                detail: if m.is_type_based {
                    Some("type-based".to_string())
                } else {
                    None
                },
                kind: symbol_kind::FUNCTION,
                span_start: 0,
                span_end: 0,
                selection_start: 0,
                selection_end: 0,
                children: Vec::new(),
            });
        }

        symbols.push(FfiDocumentSymbol {
            name: "script".to_string(),
            detail: Some(format!(
                "{} binding(s), {} import(s)",
                snapshot.bindings.len(),
                snapshot.imports.len()
            )),
            kind: symbol_kind::MODULE,
            span_start: 0,
            span_end: 0,
            selection_start: 0,
            selection_end: 0,
            children,
        });
    }

    if let Some(template) = &snapshot.template {
        let mut children = Vec::new();

        for comp in &template.components {
            children.push(FfiDocumentSymbol {
                name: comp.name.clone(),
                detail: Some(format!("{} prop(s)", comp.props.len())),
                kind: symbol_kind::CLASS,
                span_start: byte_offset_to_utf16_safe(source, comp.span.start),
                span_end: byte_offset_to_utf16_safe(source, comp.span.end),
                selection_start: byte_offset_to_utf16_safe(source, comp.span.start),
                selection_end: byte_offset_to_utf16_safe(source, comp.span.end),
                children: Vec::new(),
            });
        }

        symbols.push(FfiDocumentSymbol {
            name: "template".to_string(),
            detail: Some(format!("{} component(s)", template.components.len())),
            kind: symbol_kind::STRUCT,
            span_start: 0,
            span_end: source.encode_utf16().count() as u32,
            selection_start: 0,
            selection_end: 0,
            children,
        });
    }

    for (i, style) in snapshot.styles.iter().enumerate() {
        let mut children = Vec::new();

        if let Some(css) = &style.css {
            for class in &css.classes {
                children.push(FfiDocumentSymbol {
                    name: format!(".{}", class.name),
                    detail: None,
                    kind: symbol_kind::PROPERTY,
                    span_start: byte_offset_to_utf16_safe(source, class.span.start),
                    span_end: byte_offset_to_utf16_safe(source, class.span.end),
                    selection_start: byte_offset_to_utf16_safe(source, class.span.start),
                    selection_end: byte_offset_to_utf16_safe(source, class.span.end),
                    children: Vec::new(),
                });
            }
        }

        symbols.push(FfiDocumentSymbol {
            name: format!(
                "style{}{}",
                if i > 0 {
                    format!(" {}", i)
                } else {
                    String::new()
                },
                if style.scoped { " (scoped)" } else { "" }
            ),
            detail: None,
            kind: symbol_kind::KEY,
            span_start: 0,
            span_end: 0,
            selection_start: 0,
            selection_end: 0,
            children,
        });
    }

    symbols
}

/// Build CSS selector match results for visualization.
fn build_selector_match_results(
    snapshot: &host::FileAnalysisSnapshot,
    source: &str,
) -> Vec<FfiSelectorMatchResult> {
    let template = match &snapshot.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for style in &snapshot.styles {
        let css = match &style.css {
            Some(c) => c,
            None => continue,
        };

        for selector in &css.selectors {
            let parsed = match &selector.structure {
                Some(s) => s.clone(),
                None => match verter_analysis::style::parse_selector(&selector.text) {
                    Some(s) => s,
                    None => continue,
                },
            };

            let mut matches = Vec::new();
            for (idx, element) in template.elements.iter().enumerate() {
                let result = verter_analysis::selector_match::match_selector(
                    &parsed,
                    idx,
                    &template.elements,
                );
                matches.push(FfiElementMatch {
                    tag: element.tag.clone(),
                    span_start: byte_offset_to_utf16_safe(source, element.span.start),
                    span_end: byte_offset_to_utf16_safe(source, element.span.end),
                    result: match result {
                        verter_analysis::selector_match::MatchResult::Matches => {
                            "match".to_string()
                        }
                        verter_analysis::selector_match::MatchResult::MaybeMatches => {
                            "maybe".to_string()
                        }
                        verter_analysis::selector_match::MatchResult::NoMatch => "no".to_string(),
                    },
                });
            }

            results.push(FfiSelectorMatchResult {
                selector_text: selector.text.clone(),
                selector_start: byte_offset_to_utf16_safe(source, selector.span.start),
                selector_end: byte_offset_to_utf16_safe(source, selector.span.end),
                matches,
            });
        }
    }

    results
}

// =============================================================================
// Batch Compilation (Rayon parallel)
//
// compile_batch() is a pure stateless parallel compiler: no VerterHost, no
// caching. Each file gets its own bumpalo Allocator per Rayon thread.
// This matches Vize's compileSfcBatch() API for a fair benchmark comparison.
// =============================================================================

use oxc_allocator::Allocator;
use rayon::prelude::*;
use verter_core::compile::{compile as compile_sfc, CodegenOptions, VerterCompileOptions};

/// A single file to compile in a batch.
#[napi(object)]
#[derive(Default)]
pub struct BatchFile {
    pub filename: String,
    pub source: String,
}

/// Options for batch compilation.
#[napi(object)]
#[derive(Default)]
pub struct BatchOptions {
    /// Number of Rayon threads (0 or None = all logical CPUs).
    pub threads: Option<u32>,
}

/// Result for a single file in a batch compilation.
#[napi(object)]
pub struct BatchResult {
    pub filename: String,
    /// Combined script + template code.
    pub code: String,
    /// First error message if compilation failed, otherwise None.
    pub error: Option<String>,
    pub durationMs: f64,
}

/// Internal helper: compile a slice of (filename, source) pairs using Rayon.
/// Pure Rust types — no NAPI, testable with `cargo test`.
fn compile_batch_files(files: &[(String, String)], skip_source_map: bool) -> Vec<BatchResult> {
    files
        .par_iter()
        .map(|(filename, source)| {
            let start = std::time::Instant::now();
            let allocator = Allocator::default();
            let codegen_opts = CodegenOptions {
                filename: Some(filename.clone()),
                skip_source_map,
                ..CodegenOptions::default()
            };
            let verter_opts = VerterCompileOptions {
                source_map: false,
                ..Default::default()
            };
            let result = compile_sfc(source, &codegen_opts, &verter_opts, &allocator);
            let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
            let error = result.errors.first().map(|e| e.message.clone());
            let mut code = String::new();
            if let Some(script) = result.script {
                code.push_str(&script.code);
            }
            if let Some(tmpl) = result.template {
                code.push_str(&tmpl.code);
            }
            BatchResult {
                filename: filename.clone(),
                code,
                error,
                durationMs: duration_ms,
            }
        })
        .collect()
}

/// Compile a batch of Vue SFC files in parallel using Rayon.
///
/// Each file is compiled independently with its own allocator — no shared
/// mutable state. No caching, no analysis — compile-only for maximum throughput.
///
/// Equivalent to Vize's `compileSfcBatch` for fair benchmark comparison.
#[napi]
pub fn compile_batch(
    files: Vec<BatchFile>,
    options: Option<BatchOptions>,
) -> Result<Vec<BatchResult>> {
    let threads = options.and_then(|o| o.threads).unwrap_or(0) as usize;
    catch_panic(std::panic::AssertUnwindSafe(move || {
        let file_pairs: Vec<(String, String)> =
            files.into_iter().map(|f| (f.filename, f.source)).collect();
        if threads > 0 {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("failed to build Rayon thread pool");
            pool.install(|| compile_batch_files(&file_pairs, true))
        } else {
            compile_batch_files(&file_pairs, true)
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // @ai-generated — Tests compile_batch_files() helper: multi-file parallel compilation

    #[test]
    fn test_compile_batch_files_basic() {
        let files = vec![
            (
                "test1.vue".to_string(),
                "<template><div>hello</div></template>".to_string(),
            ),
            (
                "test2.vue".to_string(),
                "<template><span>world</span></template>".to_string(),
            ),
        ];
        let results = compile_batch_files(&files, true);
        assert_eq!(results.len(), 2);
        // Positive: output contains compiled code
        assert!(!results[0].code.is_empty(), "file 1 should produce code");
        assert!(!results[1].code.is_empty(), "file 2 should produce code");
        // Negative: raw Vue template syntax must not leak into output
        assert!(
            !results[0].code.contains("<template>"),
            "template tag must not appear in output"
        );
        assert!(
            !results[1].code.contains("<template>"),
            "template tag must not appear in output"
        );
    }

    #[test]
    fn test_compile_batch_files_empty_input() {
        let files: Vec<(String, String)> = vec![];
        let results = compile_batch_files(&files, true);
        assert_eq!(results.len(), 0, "empty input returns empty output");
    }

    #[test]
    fn test_compile_batch_files_parallel_independence() {
        // 50 files compiled in parallel must not share allocator state
        let files: Vec<(String, String)> = (0..50)
            .map(|i| {
                (
                    format!("comp{i}.vue"),
                    format!(
                        "<template><div>{{{{ msg }}}}</div></template>\
                         <script setup>const msg = 'hello{i}'</script>"
                    ),
                )
            })
            .collect();
        let results = compile_batch_files(&files, true);
        assert_eq!(results.len(), 50);
        for (i, r) in results.iter().enumerate() {
            assert!(!r.code.is_empty(), "file {i} must produce code");
            // Negative: no raw template tags should appear in output
            assert!(
                !r.code.contains("<template>"),
                "file {i} must not contain raw template tag"
            );
        }
    }
}
