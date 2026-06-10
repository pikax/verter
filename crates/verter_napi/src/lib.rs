// NAPI-RS generates variables from camelCase struct fields — suppress warnings.
#![allow(non_snake_case)]

//! # verter_napi — Node.js bindings for Verter
//!
//! NAPI-RS binding layer that exposes [`verter_session::VerterHost`] and
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
use verter_session as host;
use verter_type_expr::TypeExpr;

mod audit;
mod meta;
mod typeinfo;

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
        // Typed unsupported-language failure: the request named a
        // language row with no registered implementation — same status
        // family as the classify errors (the caller's input names a
        // language the host cannot serve), distinguishable from a
        // generic internal failure.
        host::HostError::Scheduler(
            verter_scheduler::job::SchedulerError::UnsupportedLanguage { .. },
        ) => Status::InvalidArg,
        host::HostError::CompileError(_) => Status::GenericFailure,
        #[allow(unreachable_patterns)]
        _ => Status::GenericFailure,
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
        let core_options = verter_compiler::css::ProcessStyleOptions {
            scope_id: &options.scopeId,
            scoped: options.scoped.unwrap_or(false),
            is_module: options.isModule.unwrap_or(false),
            module_name: options.moduleName.as_deref(),
            filename: options.filename.as_deref(),
            sourcemap: options.sourcemap.unwrap_or(false),
        };

        verter_compiler::css::process_style(&css, &core_options)
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
    /// Enable Rust-first native audit for component-meta requests.
    /// When true, timing/memory/store data is captured per request.
    pub auditEnabled: Option<bool>,
    /// Enable per-request semantic footprint capture. Requires
    /// `auditEnabled = true` — necessary for
    /// `getComponentMetaWithAudit` to return a populated bundle.
    pub footprintCapture: Option<bool>,
    /// Capacity of the host-owned typeinfo scratch cache used by
    /// `evaluateTypeExpressionWithAudit`. `None` (default) selects
    /// 64 entries; `Some(0)` disables the cache; other values cap
    /// the LRU at the chosen size.
    pub typeinfoScratchCacheCapacity: Option<u32>,
    /// Worker count for the host-owned CPU pool used by every host batch
    /// API's outer coordinator — `compile_many` and the component-meta
    /// batch. `None` (default) resolves to
    /// `std::thread::available_parallelism` at host-construction time;
    /// `Some(0)` is treated as `None`; other positive values cap the
    /// pool's worker count. The host pool is built once at host
    /// construction and reused across every batch call — to
    /// change the pool size, construct a new host.
    pub hostCpuThreads: Option<u32>,
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
            audit_enabled: n.auditEnabled,
            footprint_capture: n.footprintCapture,
            typeinfo_scratch_cache_capacity: n.typeinfoScratchCacheCapacity,
            host_cpu_threads: n.hostCpuThreads,
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
    /// Experimental: strict slot children type checking.
    pub strictSlots: Option<bool>,
    /// Requested compile cache mode: "stateless", "content", or
    /// "session" (default).
    pub requestedMode: Option<String>,
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
            strict_slots: n.strictSlots,
            requested_mode: n.requestedMode,
        }
    }
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiIdeProjectConfig {
    pub root: String,
    pub workspaceRoot: String,
    pub tsconfigPath: Option<String>,
    pub providerRoot: Option<String>,
    pub workspaceAliases: Option<Vec<NapiWorkspaceAlias>>,
    pub compilerOptions: Option<NapiIdeProjectCompilerOptions>,
    pub references: Option<Vec<String>>,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiWorkspaceAlias {
    pub find: String,
    pub replacement: String,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiIdeProjectCompilerOptions {
    pub baseUrl: Option<String>,
    pub paths: Option<Vec<NapiTsConfigPath>>,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiTsConfigPath {
    pub pattern: String,
    pub targets: Vec<String>,
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
pub struct NapiDependencyResolution {
    pub specifier: String,
    #[napi(ts_type = "string | undefined")]
    pub resolved_canonical_id: Option<String>,
    #[napi(ts_type = "string[] | undefined")]
    pub possible_canonical_ids: Option<Vec<String>>,
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
pub struct NapiExportSignature {
    pub name: String,
    pub isType: bool,
    pub reexportSource: Option<String>,
    pub reexportLocal: Option<String>,
}

#[napi(object)]
pub struct NapiResolvedExport {
    pub name: String,
    pub isType: bool,
    pub sourceCanonicalId: Option<String>,
    pub sourceName: String,
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
    pub moduleReferences: Vec<NapiModuleReference>,
    pub preprocessorRequests: Vec<NapiPreprocessorRequest>,
    pub exportSignatures: Vec<NapiExportSignature>,
    pub parseDurationMs: f64,
}

#[napi(object)]
pub struct NapiModuleReference {
    pub syntax: String,
    pub semantics: String,
    pub isTypeOnly: bool,
    pub rawText: String,
    pub literalSpecifier: Option<String>,
    pub finiteSpecifiers: Vec<String>,
    pub staticPrefix: Option<String>,
    pub analyzability: String,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub exprSpanStart: u32,
    pub exprSpanEnd: u32,
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
    /// `true` iff this response was served from a warm cache slot (the
    /// fact-validated session slot OR the content-addressed store).
    pub cacheHit: bool,
    /// Requested compile cache mode ("stateless" / "content" / "session").
    pub requestedMode: String,
    /// Actual compile cache mode the runtime ran under.
    pub actualMode: String,
    /// Highest-priority downgrade reason, or `None` when none fired.
    pub downgradeReason: Option<String>,
}

/// A single destructured binding's source mapping (UTF-16 for JS).
#[napi(object)]
pub struct NapiDestructuredBinding {
    pub name: String,
    pub sourceStart: u32,
    pub sourceEnd: u32,
}

/// Metadata for the destructured block region in the generated TSX (UTF-16 for JS).
#[napi(object)]
pub struct NapiDestructuredBlockMeta {
    pub bindings: Vec<NapiDestructuredBinding>,
    pub blockStart: u32,
    pub blockEnd: u32,
}

/// IDE output for type checking (dedicated API, not a virtual file).
#[napi(object)]
pub struct NapiIdeResponse {
    pub code: String,
    pub sourceMap: Option<String>,
    pub isJsx: bool,
    pub destructuredBlock: Option<NapiDestructuredBlockMeta>,
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
/// Only populated when built with the `session_metrics` feature.
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
    /// Total style override calls (legacy, reserved for metrics compatibility).
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

fn host_module_reference_syntax_to_str(
    syntax: verter_semantic::analysis::ModuleReferenceSyntax,
) -> &'static str {
    match syntax {
        verter_semantic::analysis::ModuleReferenceSyntax::StaticImport => "staticImport",
        verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom => "exportFrom",
        verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport => "dynamicImport",
        verter_semantic::analysis::ModuleReferenceSyntax::RequireCall => "requireCall",
    }
}

fn host_module_reference_semantics_to_str(
    semantics: verter_semantic::analysis::ModuleReferenceSemantics,
) -> &'static str {
    match semantics {
        verter_semantic::analysis::ModuleReferenceSemantics::Import => "import",
        verter_semantic::analysis::ModuleReferenceSemantics::Require => "require",
    }
}

fn host_module_reference_analyzability_to_str(
    analyzability: verter_semantic::analysis::ModuleReferenceAnalyzability,
) -> &'static str {
    match analyzability {
        verter_semantic::analysis::ModuleReferenceAnalyzability::Exact => "exact",
        verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet => "finiteSet",
        verter_semantic::analysis::ModuleReferenceAnalyzability::UnknownDynamic => "unknownDynamic",
    }
}

fn napi_module_reference_syntax_from_str(
    syntax: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceSyntax> {
    match syntax {
        "staticImport" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::StaticImport),
        "exportFrom" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom),
        "dynamicImport" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport),
        "requireCall" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::RequireCall),
        other => Err(ffi_err(format!("unknown module reference syntax: {other}"))),
    }
}

fn napi_module_reference_semantics_from_str(
    semantics: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceSemantics> {
    match semantics {
        "import" => Ok(verter_semantic::analysis::ModuleReferenceSemantics::Import),
        "require" => Ok(verter_semantic::analysis::ModuleReferenceSemantics::Require),
        other => Err(ffi_err(format!(
            "unknown module reference semantics: {other}"
        ))),
    }
}

fn napi_module_reference_analyzability_from_str(
    analyzability: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceAnalyzability> {
    match analyzability {
        "exact" => Ok(verter_semantic::analysis::ModuleReferenceAnalyzability::Exact),
        "finiteSet" => Ok(verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet),
        "unknownDynamic" => {
            Ok(verter_semantic::analysis::ModuleReferenceAnalyzability::UnknownDynamic)
        }
        other => Err(ffi_err(format!(
            "unknown module reference analyzability: {other}"
        ))),
    }
}

fn napi_module_reference_to_analysis(
    input: NapiModuleReference,
) -> Result<verter_semantic::analysis::AnalyzedModuleReference> {
    Ok(verter_semantic::analysis::AnalyzedModuleReference {
        syntax: napi_module_reference_syntax_from_str(&input.syntax)?,
        semantics: napi_module_reference_semantics_from_str(&input.semantics)?,
        is_type_only: input.isTypeOnly,
        span: verter_span::Span::new(input.spanStart, input.spanEnd),
        expr_span: verter_span::Span::new(input.exprSpanStart, input.exprSpanEnd),
        raw_text: input.rawText,
        literal_specifier: input.literalSpecifier,
        finite_specifiers: input.finiteSpecifiers,
        static_prefix: input.staticPrefix,
        analyzability: napi_module_reference_analyzability_from_str(&input.analyzability)?,
    })
}

fn default_known_dependency_extensions() -> Vec<String> {
    vec![
        "".to_string(),
        ".ts".to_string(),
        ".tsx".to_string(),
        ".js".to_string(),
        ".jsx".to_string(),
        ".mts".to_string(),
        ".mjs".to_string(),
        ".cts".to_string(),
        ".cjs".to_string(),
        ".vue".to_string(),
    ]
}

fn host_module_reference_to_napi(input: host::ScriptModuleReference) -> NapiModuleReference {
    NapiModuleReference {
        syntax: host_module_reference_syntax_to_str(input.syntax).to_string(),
        semantics: host_module_reference_semantics_to_str(input.semantics).to_string(),
        isTypeOnly: input.is_type_only,
        rawText: input.raw_text,
        literalSpecifier: input.literal_specifier,
        finiteSpecifiers: input.finite_specifiers,
        staticPrefix: input.static_prefix,
        analyzability: host_module_reference_analyzability_to_str(input.analyzability).to_string(),
        spanStart: input.span.start,
        spanEnd: input.span.end,
        exprSpanStart: input.expr_span.start,
        exprSpanEnd: input.expr_span.end,
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
        moduleReferences: input
            .module_references
            .into_iter()
            .map(host_module_reference_to_napi)
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
        exportSignatures: input
            .export_signatures
            .into_iter()
            .map(|sig| NapiExportSignature {
                name: sig.name,
                isType: sig.is_type,
                reexportSource: sig.reexport_source,
                reexportLocal: sig.reexport_local,
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
        cacheHit: input.cache_hit,
        requestedMode: input.requested_mode.to_string(),
        actualMode: input.actual_mode.to_string(),
        downgradeReason: input.downgrade_reason.map(|r| r.to_string()),
    }
}

fn napi_project_config_to_ide(
    config: NapiIdeProjectConfig,
) -> verter_semantic::analysis::project_resolver::IdeProjectConfig {
    let mut ide = verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
        config.root.clone(),
        config.workspaceRoot,
        config.tsconfigPath,
    );
    if let Some(provider_root) = config.providerRoot {
        ide.provider_root = provider_root;
    }
    if let Some(aliases) = config.workspaceAliases {
        ide.workspace_aliases = aliases
            .into_iter()
            .map(
                |a| verter_semantic::analysis::project_resolver::WorkspaceAlias {
                    find: a.find,
                    replacement: a.replacement,
                },
            )
            .collect();
    }
    if let Some(opts) = config.compilerOptions {
        ide.compiler_options.base_url = opts.baseUrl;
        if let Some(paths) = opts.paths {
            ide.compiler_options.paths =
                paths.into_iter().map(|p| (p.pattern, p.targets)).collect();
        }
    }
    if let Some(refs) = config.references {
        ide.references = refs;
    }
    ide
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
// - Both: new, resolve, upsert, applyBlockOverrides,
//         getVirtualFile, listVirtualFiles, remove, setImportDependencies,
//         getAnalysis, getTsx, lint, getCodeActions, getLintRuleMetadata,
//         getDocumentSymbols, matchCssSelectors, computeCrossFileOptimizations
// - NAPI-only: processStyle (requires Node.js), getTsc, compileMany, getMetrics
// =============================================================================

// ═══════════════════════════════════════════════════════════════════════════
// Workspace
// ═══════════════════════════════════════════════════════════════════════════

/// Directory entry returned by `Workspace.readDir()`.
#[napi(object)]
pub struct NapiDirEntry {
    pub path: String,
    pub is_dir: bool,
}

/// Workspace object backed by `FilesystemWorkspace`.
///
/// Provides file access, import resolution, and project configuration.
/// Construct first, then pass to `VerterHost.withWorkspace()`.
#[napi(js_name = "Workspace")]
pub struct NapiWorkspace {
    inner: std::sync::Arc<verter_workspace::FilesystemWorkspace>,
}

impl NapiWorkspace {
    /// Get the underlying workspace as a trait object.
    pub(crate) fn workspace(&self) -> std::sync::Arc<dyn verter_workspace::WorkspaceAccess> {
        std::sync::Arc::clone(&self.inner) as std::sync::Arc<dyn verter_workspace::WorkspaceAccess>
    }
}

#[napi]
impl NapiWorkspace {
    /// Create a new workspace rooted at the given directories.
    ///
    /// **Lazy by design.** The constructor stores the roots and the
    /// backing `FilesystemWorkspace` only — it does NOT auto-discover
    /// tsconfigs or build a project graph. Until a caller invokes
    /// [`Self::configure_projects`] (`workspace.configureProjects(...)`
    /// in JS), `Engine::resolve_import` walks an empty `ProjectGraph`
    /// and falls through to the bare-VFS resolver.
    ///
    /// JS consumers that need a configured workspace MUST call
    /// `configureProjects` after construction, supplying the alias map
    /// derived from the project's tsconfig chain. The canonical pattern
    /// lives in `packages/component-meta/src/compat/checker.ts`:
    /// `extractPathAliases(parsedTsconfig, projectRoot)` produces the
    /// `NapiIdeProjectConfig` shape, which is passed to
    /// `workspace.configureProjects([aliases])`. Bench and audit
    /// harnesses mirror the same shape.
    #[napi(constructor)]
    pub fn new(roots: Vec<String>) -> Self {
        let ws = verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions {
            roots,
            eager_preload: false,
        });
        Self {
            inner: std::sync::Arc::new(ws),
        }
    }

    // ── Async filesystem operations ──
    //
    // All filesystem methods are async to avoid blocking the Node.js event loop.
    // The underlying VFS operations are synchronous but run on the libuv thread pool.

    /// Read a file from the workspace (overlay → snapshot → disk).
    #[napi(js_name = "readFile")]
    pub async fn read_file(&self, path: String) -> Result<Option<String>> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.read_file(&path).map(|s| s.to_string()))
    }

    /// Check if a file exists in the workspace.
    #[napi(js_name = "fileExists")]
    pub async fn file_exists(&self, path: String) -> Result<bool> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.file_exists(&path))
    }

    /// Check if a path is a directory.
    #[napi(js_name = "isDir")]
    pub async fn is_dir(&self, path: String) -> Result<bool> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.is_dir(&path))
    }

    /// Write file content. Creates parent directories as needed.
    #[napi(js_name = "writeFile")]
    pub async fn write_file(&self, path: String, content: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .write_file(&path, &content)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Read directory entries. Returns array of { path, isDir }.
    #[napi(js_name = "readDir")]
    pub async fn read_dir(&self, dir: String) -> Result<Vec<NapiDirEntry>> {
        use verter_workspace::WorkspaceRead;
        self.inner
            .read_dir(&dir)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| NapiDirEntry {
                        path: e.path,
                        is_dir: e.is_dir,
                    })
                    .collect()
            })
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Recursively walk a directory. Returns matching file paths.
    #[napi(js_name = "walk")]
    pub async fn walk(
        &self,
        root: String,
        exclude_dirs: Vec<String>,
        extensions: Option<Vec<String>>,
    ) -> Result<Vec<String>> {
        use verter_workspace::WorkspaceRead;
        let exts = extensions;
        self.inner
            .walk(
                &root,
                &|dir_path| {
                    let name = dir_path.rsplit('/').next().unwrap_or(dir_path);
                    !exclude_dirs.iter().any(|ex| ex == name)
                },
                &|file_path| match &exts {
                    Some(ext_list) => ext_list.iter().any(|ext| file_path.ends_with(ext.as_str())),
                    None => true,
                },
            )
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Delete a file.
    #[napi(js_name = "deleteFile")]
    pub async fn delete_file(&self, path: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .delete_file(&path)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Create a directory and all parent directories.
    #[napi(js_name = "createDirAll")]
    pub async fn create_dir_all(&self, path: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .create_dir_all(&path)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Delete a directory and all its contents.
    #[napi(js_name = "deleteDirAll")]
    pub async fn delete_dir_all(&self, path: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .delete_dir_all(&path)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Copy a file from src to dst.
    #[napi(js_name = "copyFile")]
    pub async fn copy_file(&self, src: String, dst: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .copy_file(&src, &dst)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Resolve symlinks to real path. Returns null if not found.
    #[napi(js_name = "realpath")]
    pub async fn realpath(&self, path: String) -> Result<Option<String>> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.realpath(&path))
    }

    /// Resolve an import specifier with context.
    #[napi(js_name = "resolveImport")]
    pub async fn resolve_import(
        &self,
        importer: String,
        specifier: String,
        phase: Option<String>,
        kind: Option<String>,
    ) -> Result<Option<String>> {
        use verter_workspace::WorkspaceRead;
        let phase = match phase.as_deref() {
            Some("provider") => verter_workspace::ResolvePhase::ProviderGraph,
            _ => verter_workspace::ResolvePhase::CodegenBlocker,
        };
        let kind = match kind.as_deref() {
            Some("type") => verter_workspace::ResolveRequestKind::TypeImport,
            Some("require") => verter_workspace::ResolveRequestKind::RequireCall,
            Some("src") => verter_workspace::ResolveRequestKind::SfcSrcAttr,
            _ => verter_workspace::ResolveRequestKind::EsmImport,
        };
        let ctx = verter_workspace::ResolutionContext { phase, kind };
        Ok(self
            .inner
            .resolve_import(&importer, &specifier, ctx)
            .map(|r| r.source_id))
    }

    /// Configure project resolver from tsconfig/alias data.
    /// Replaces (not merges with) any auto-discovered graph.
    #[napi(js_name = "configureProjects")]
    pub fn configure_projects(&self, projects: Vec<NapiIdeProjectConfig>) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig> =
                projects
                    .into_iter()
                    .map(napi_project_config_to_ide)
                    .collect();
            use verter_workspace::WorkspaceAccess;
            self.inner.configure_resolver(configs);
        }))
    }

    /// Notify workspace that an editor buffer is open/changed.
    #[napi(js_name = "notifyUpsert")]
    pub fn notify_upsert(&self, canonical_id: String, source: Buffer) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        let source_str = std::str::from_utf8(&source)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid UTF-8: {e}")))?;
        self.inner
            .notify_upsert(&canonical_id, std::sync::Arc::from(source_str));
        Ok(())
    }

    /// Notify workspace that an editor buffer was closed.
    #[napi(js_name = "notifyClose")]
    pub fn notify_close(&self, canonical_id: String) {
        use verter_workspace::WorkspaceAccess;
        self.inner.notify_close(&canonical_id);
    }

    /// Notify workspace that a file was deleted.
    #[napi(js_name = "notifyDelete")]
    pub fn notify_delete(&self, canonical_id: String) {
        use verter_workspace::WorkspaceAccess;
        self.inner.notify_delete(&canonical_id);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Host
// ═══════════════════════════════════════════════════════════════════════════

/// Manages a collection of Vue SFCs and their compiled virtual files (script,
/// template, styles). Files are upserted as source, then lazily compiled into
/// virtual outputs that a bundler or LSP can request individually.
#[napi(js_name = "VerterHost")]
pub struct NapiVerterHost {
    inner: std::sync::Arc<host::VerterHost>,
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
            inner: std::sync::Arc::new(host::VerterHost::new_standalone(
                ffi_config_to_host(ffi_config).map_err(ffi_err)?,
            )),
        })
    }

    /// Creates a new `VerterHost` backed by the given workspace.
    ///
    /// The workspace handles all file access and import resolution.
    /// Use `workspace.configureProjects()` before calling this to set up
    /// the project resolver.
    #[napi(factory)]
    pub fn with_workspace(
        config: Option<NapiHostConfig>,
        workspace: &NapiWorkspace,
    ) -> Result<Self> {
        let ffi_config: FfiHostConfig = config.unwrap_or_default().into();
        let host_config = ffi_config_to_host(ffi_config).map_err(ffi_err)?;
        Ok(Self {
            inner: std::sync::Arc::new(host::VerterHost::new(host_config, workspace.inner.clone())),
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
    /// - `request.fileKind` — optional explicit kind (`"vue"`/`"sfc"`/
    ///   `"vue_sfc"`, `"svelte"`, or `"non_sfc"`/`"text"`/`"file"`);
    ///   classified from the canonical path when `None`.
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
    /// Returns `null` if the virtual node does not exist (e.g. no `<script>` block).
    /// Returns an error if the query is invalid or the source file is not found.
    #[napi(js_name = "getVirtualFile")]
    pub fn get_virtual_file(
        &self,
        query: NapiVirtualQuery,
    ) -> Result<Option<NapiVirtualFileResponse>> {
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
        }))?;
        match result {
            Ok(vf) => {
                let source = canonical_for_source
                    .as_deref()
                    .and_then(|canonical| self.inner.get_source(canonical));
                Ok(Some(host_virtual_file_to_napi(vf, source.as_deref())))
            }
            Err(host::HostError::MissingVirtualNode { .. }) => Ok(None),
            Err(e) => Err(host_error(e)),
        }
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
    /// defining NAPI structs for all `verter_semantic::analysis` types is high effort
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

    /// Evaluate type annotations for a file's component metadata using the
    /// lightweight native evaluator.
    ///
    /// Returns JSON `{ props, emits, slotBindings, bindings }` or `null`.
    #[napi(js_name = "evaluateTypes")]
    pub fn evaluate_types(&self, canonical_or_alias: String) -> Result<Option<String>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let result = self.inner.evaluate_types(&canonical_or_alias);
            let Some(result) = result else {
                return Ok(None);
            };
            let json = serde_json::to_string(&result).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("type evaluation serialization error: {e}"),
                )
            })?;
            Ok(Some(json))
        }))?
    }

    /// Returns all exports of a file, following re-export chains to their ultimate source.
    ///
    /// For barrel files like `export { default as Button } from './Button.vue'`, this
    /// resolves through the chain to return the ultimate source file and name.
    #[napi(js_name = "resolveExports")]
    pub fn resolve_exports(&self, canonical_or_alias: String) -> Result<Vec<NapiResolvedExport>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.resolve_exports(&canonical_or_alias)
        }))
        .map(|exports| {
            exports
                .into_iter()
                .map(|e| NapiResolvedExport {
                    name: e.name,
                    isType: e.is_type,
                    sourceCanonicalId: e.source_canonical_id,
                    sourceName: e.source_name,
                })
                .collect()
        })
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
        let sfc_source = self.inner.get_source(&canonical_id);
        Ok(result.map(|r| {
            let destructured_block = r.destructured_block.as_ref().map(|meta| {
                let sfc = sfc_source.as_deref().unwrap_or("");
                let bindings: Vec<verter_ffi::convert::DestructuredBindingInput<'_>> = meta
                    .bindings
                    .iter()
                    .map(|b| verter_ffi::convert::DestructuredBindingInput {
                        name: &b.name,
                        source_start: b.source_span.start,
                        source_end: b.source_span.end,
                    })
                    .collect();
                let ffi = verter_ffi::convert::convert_destructured_block_meta(
                    &bindings,
                    meta.block_start,
                    meta.block_end,
                    sfc,
                    &r.code,
                    verter_ffi::convert::OffsetEncoding::Utf16,
                );
                NapiDestructuredBlockMeta {
                    bindings: ffi
                        .bindings
                        .into_iter()
                        .map(|b| NapiDestructuredBinding {
                            name: b.name,
                            sourceStart: b.source_start,
                            sourceEnd: b.source_end,
                        })
                        .collect(),
                    blockStart: ffi.block_start,
                    blockEnd: ffi.block_end,
                }
            });
            NapiIdeResponse {
                code: r.code.to_string(),
                sourceMap: r.source_map.map(|s| s.to_string()),
                isJsx: r.is_jsx,
                destructuredBlock: destructured_block,
            }
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
    #[napi(js_name = "getPublicApi")]
    pub fn get_public_api(
        &self,
        canonical_id: String,
        mode: Option<String>,
    ) -> Result<Option<NapiTscResponse>> {
        let mode = match mode.as_deref() {
            None | Some("public") => host::PublicApiMode::Public,
            Some("testing") => host::PublicApiMode::Testing,
            Some(other) => {
                return Err(ffi_err(format!(
                    "invalid public api mode '{other}', expected 'public' or 'testing'"
                )));
            }
        };
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .get_public_api_with_mode(&canonical_id, mode, None)
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
    /// - `resolutions` — per-specifier resolution records with exact or candidate canonical IDs.
    #[napi(js_name = "setImportDependencies")]
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: String,
        resolutions: Vec<NapiDependencyResolution>,
    ) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.set_import_dependencies(
                &canonical_or_alias,
                resolutions
                    .into_iter()
                    .map(|r| host::DependencyResolution {
                        specifier: r.specifier,
                        resolved_canonical_id: r.resolved_canonical_id,
                        possible_canonical_ids: r.possible_canonical_ids.unwrap_or_default(),
                    })
                    .collect(),
            );
        }))
    }

    /// Returns the exact and finite-set module reference candidates in encounter order.
    ///
    /// Unknown-dynamic references are skipped entirely.
    #[napi(js_name = "collectResolvableModuleReferenceSpecifiers")]
    pub fn collect_resolvable_module_reference_specifiers(
        &self,
        module_references: Vec<NapiModuleReference>,
    ) -> Result<Vec<String>> {
        let module_references = module_references
            .into_iter()
            .map(napi_module_reference_to_analysis)
            .collect::<Result<Vec<_>>>()?;
        Ok(
            verter_semantic::analysis::project_resolver::collect_resolvable_module_reference_specifiers(
                &module_references,
            ),
        )
    }

    /// Resolves exact and finite module reference candidates against a caller-provided
    /// in-memory known-file set, without reading from disk.
    #[napi(js_name = "resolveKnownModuleReferenceDependencies")]
    pub fn resolve_known_module_reference_dependencies(
        &self,
        owner_id: String,
        module_references: Vec<NapiModuleReference>,
        known_ids: Vec<String>,
        extensions: Option<Vec<String>>,
    ) -> Result<Vec<String>> {
        let module_references = module_references
            .into_iter()
            .map(napi_module_reference_to_analysis)
            .collect::<Result<Vec<_>>>()?;
        let extensions = extensions.unwrap_or_else(default_known_dependency_extensions);
        Ok(
            verter_semantic::analysis::project_resolver::resolve_known_module_reference_dependencies(
                &owner_id,
                &module_references,
                &known_ids,
                &extensions,
            ),
        )
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

    /// Release all cached data (files, aliases, dependency graph).
    ///
    /// Configure project-scoped path alias resolution.
    ///
    /// Accepts a list of project configs describing tsconfig paths, workspace
    /// aliases, and project references. The host uses these to resolve aliased
    /// import specifiers (e.g. `@/components/Foo.vue`, `#imports`) without
    /// relying on external caller-provided resolutions.
    ///
    /// Pass an empty array to clear the resolver.
    #[napi(js_name = "configureProjects")]
    pub fn configure_projects(&self, projects: Vec<NapiIdeProjectConfig>) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig> =
                projects
                    .into_iter()
                    .map(napi_project_config_to_ide)
                    .collect();
            self.inner.configure_projects(configs);
        }))
    }

    /// Call this before dropping the host to allow the Rust allocator to free
    /// backing memory immediately, rather than waiting for GC finalisation.
    /// This prevents the Node.js process from hanging on exit.
    #[napi]
    pub fn close(&self) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.close();
        }))
    }

    /// Resolve an import specifier through the host's resolution chain.
    ///
    /// Uses VFS-first-then-fallback pattern (same as internal resolution).
    #[napi(js_name = "resolveImport")]
    pub fn resolve_import_napi(
        &self,
        importer: String,
        specifier: String,
        #[napi(ts_arg_type = "string | undefined")] _phase: Option<String>,
        #[napi(ts_arg_type = "string | undefined")] _kind: Option<String>,
    ) -> Option<String> {
        self.inner.resolve_import(&importer, &specifier)
    }

    /// Returns a snapshot of host performance metrics.
    ///
    /// Only available when built with the `session_metrics` feature.
    /// Returns `null` when the feature is disabled.
    #[napi(js_name = "getMetrics")]
    pub fn get_metrics(&self) -> Option<NapiHostMetrics> {
        #[cfg(feature = "session_metrics")]
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
        #[cfg(not(feature = "session_metrics"))]
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
                    .lint(
                        Some(&script),
                        snapshot.template.as_deref(),
                        &snapshot.styles,
                    )
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
                    snapshot.template.as_deref(),
                    &snapshot.styles,
                    Some(source),
                );

                let engine = ActionEngine::default();
                let ctx = ActionContext {
                    source,
                    file_id: &canonical_or_alias,
                    diagnostics: &diag_set,
                    template: snapshot.template.as_deref(),
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

    /// host-backed batch compile.
    ///
    /// Compiles a batch of Vue SFC inputs through the production host
    /// path (scheduler + dispatch + compile_cache). Returns one
    /// [`NapiCompileBatchEntry`] per input, in the original input
    /// order.
    ///
    /// Per-input panic isolation: if codegen panics for one input,
    /// only that input's entry receives a `compiler panic: ...`
    /// error message; the rest of the batch completes normally.
    ///
    /// `options.priority` is `"interactive"` or `"background"`;
    /// invalid strings return a NAPI error. Default is `"background"`.
    #[napi(js_name = "compileMany")]
    pub fn compile_many(
        &self,
        files: Vec<NapiCompileBatchInput>,
        options: Option<NapiCompileBatchOptions>,
    ) -> Result<Vec<NapiCompileBatchEntry>> {
        use verter_scheduler::stage::Priority;
        let opts = options.unwrap_or_default();
        let priority = match opts.priority.as_deref() {
            None | Some("background") => Some(Priority::Background),
            Some("interactive") => Some(Priority::Interactive),
            Some(other) => {
                return Err(ffi_err(format!(
                    "invalid priority '{other}', expected 'interactive' or 'background'"
                )));
            }
        };
        let inputs: Vec<host_compile::CompileBatchInput> = files
            .into_iter()
            .map(|f| {
                let requested_mode = f
                    .requestedMode
                    .map(|m| ffi_compile_cache_mode_to_host(&m))
                    .transpose()
                    .map_err(ffi_err)?;
                Ok(host_compile::CompileBatchInput {
                    canonical_id: f.canonicalId,
                    source: std::sync::Arc::from(buffer_to_string(f.source)?),
                    requested_mode,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let default_mode = opts
            .defaultMode
            .map(|m| ffi_compile_cache_mode_to_host(&m))
            .transpose()
            .map_err(ffi_err)?;
        let entries = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.compile_many(
                inputs,
                host_compile::CompileBatchOptions {
                    priority,
                    default_mode,
                },
            )
        }))?;
        Ok(entries
            .into_iter()
            .map(|e| NapiCompileBatchEntry {
                canonicalId: e.canonical_id,
                code: e.code.to_string(),
                sourceMap: e.source_map.map(|s| s.to_string()),
                errors: e.errors,
                durationMs: e.duration_ms,
                cacheHit: e.cache_hit,
                requestedMode: e.requested_mode.to_string(),
                actualMode: e.actual_mode.to_string(),
                downgradeReason: e.downgrade_reason.map(|r| r.to_string()),
            })
            .collect())
    }

    // =========================================================================
    // Typed audit entry-points
    //
    // Each entry-point wraps a `VerterHost::*_with_audit` Rust producer
    // and returns the produced `RequestAuditRecord` as a JSON Buffer.
    // Helper types and parsing free functions live in `crate::audit`.
    //
    // The methods MUST live in this `impl NapiVerterHost` block (not a
    // sibling module) so the napi-derive class registration picks up
    // the `js_name = "VerterHost"` rename declared on the struct in
    // this same file.
    // =========================================================================

    /// Run a single type-resolution query through the shared dispatch
    /// and return the produced `RequestAuditRecord` as a JSON
    /// `Buffer`. The query resolves `decl_name` in the top-level
    /// scope of `canonical_id`. Returns `null` when audit is
    /// disabled.
    #[napi(js_name = "resolveTypeWithAudit")]
    pub fn resolve_type_with_audit(
        &self,
        canonical_id: String,
        decl_name: String,
    ) -> Result<Option<Buffer>> {
        use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: ScopeId {
                    canonical_id: std::sync::Arc::<str>::from(canonical_id.as_str()),
                    local_scope: None,
                },
                name: std::sync::Arc::<str>::from(decl_name.as_str()),
            });
            let record = host
                .resolve_type_with_audit(key, &canonical_id)
                .audit()
                .clone();
            audit::encode_stored_record(&record)
        }))?
    }

    /// Compile `canonical_id` for the requested codegen target and
    /// return the produced `RequestAuditRecord` as a JSON `Buffer`.
    /// Accepted target names: `BUNDLER`, `IDE`, `ANALYSIS`, `META`,
    /// `TSX`, `TSC`. Returns `null` when audit is disabled.
    #[napi(js_name = "compileWithAudit")]
    pub fn compile_with_audit(
        &self,
        canonical_id: String,
        target: String,
    ) -> Result<Option<Buffer>> {
        let target = audit::parse_compile_target(&target)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let record = host
                .compile_with_audit(&canonical_id, target)
                .audit()
                .clone();
            audit::encode_stored_record(&record)
        }))?
    }

    /// Materialise the `AnalysisReady` artifact for `canonical_id`
    /// under audit and return the produced `RequestAuditRecord` as a
    /// JSON `Buffer`. Returns `null` when audit is disabled or the
    /// canonical does not exist.
    #[napi(js_name = "analyzeWithAudit")]
    pub fn analyze_with_audit(&self, canonical_id: String) -> Result<Option<Buffer>> {
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let record = host.analyze_with_audit(&canonical_id).audit().clone();
            audit::encode_stored_record(&record)
        }))?
    }

    /// Drive a workspace operation under audit and return the
    /// produced `RequestAuditRecord` as a JSON `Buffer`. The `op`
    /// argument is shaped as `{ type: "AuditResolve", specifier, from
    /// }` / `{ type: "DepGraphTraverse", root }` / `{ type:
    /// "ResolverWalk", specifier }`. Always returns a record.
    #[napi(js_name = "auditWorkspaceOp")]
    pub fn audit_workspace_op(&self, op: audit::NapiWorkspaceOp) -> Result<Buffer> {
        let arg = op.try_into_workspace_op()?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let record = host.audit_workspace_op(arg);
            audit::encode_record(&record)
        }))?
    }

    /// Drain the most-recent `RequestAuditRecord` from the host's
    /// audit store. Returns `null` when the store is empty. Drains
    /// the entry: a second call after a single insert returns null.
    #[napi(js_name = "getLastAuditRecord")]
    pub fn get_last_audit_record(&self) -> Result<Option<Buffer>> {
        use verter_audit::batch::AuditRecordSource;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let mut latest_id: Option<u64> = None;
            let mut latest_at: Option<std::time::Instant> = None;
            store.for_each_record(&mut |inserted_at, record| {
                let is_newer = match latest_at {
                    None => true,
                    Some(prev) => inserted_at > prev,
                };
                if is_newer {
                    latest_at = Some(inserted_at);
                    latest_id = Some(record.request_id);
                }
            });
            let Some(id) = latest_id else {
                return Ok(None);
            };
            match store.take(id) {
                Some(rec) => audit::encode_record(&rec).map(Some),
                None => Ok(None),
            }
        }))?
    }

    /// Non-destructive filtered query over the host's audit store.
    /// Returns a JSON-serialised array of records (`Buffer`).
    #[napi(js_name = "getAuditRecords")]
    pub fn get_audit_records(
        &self,
        filter: Option<audit::NapiAuditRecordFilter>,
    ) -> Result<Buffer> {
        use verter_audit::batch::AuditRecordSource;
        let filter = filter.unwrap_or_default();
        let kind_filter = filter.kind;
        let since = match filter.since_request_id.as_deref() {
            Some(s) => Some(audit::parse_request_id_str(s)?),
            None => None,
        };
        let limit = filter.limit.map(|n| n as usize);
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let mut collected: Vec<verter_audit::RequestAuditRecord> = Vec::new();
            store.for_each_record(&mut |_inserted_at, record| {
                if let Some(filter_kind) = kind_filter.as_deref() {
                    if !audit::kind_matches(filter_kind, &record.kind) {
                        return;
                    }
                }
                if let Some(since_id) = since {
                    if record.request_id <= since_id {
                        return;
                    }
                }
                collected.push(record.clone());
            });
            if let Some(n) = limit {
                collected.truncate(n);
            }
            audit::encode_record_list(&collected)
        }))?
    }

    /// Run the bundler-batch aggregator over the host's audit store
    /// and return the produced `BundlerBatchPayload` as a JSON
    /// `Buffer`. The summary tags the payload with the requested
    /// bundler kind (defaults to `Vite`).
    #[napi(js_name = "getBundlerBatchSummary")]
    pub fn get_bundler_batch_summary(
        &self,
        args: Option<audit::NapiBundlerBatchSummaryArgs>,
    ) -> Result<Buffer> {
        use verter_audit::batch::{AuditRecordSource, BatchAuditAggregator};
        let args = args.unwrap_or_default();
        let kind = audit::parse_bundler_kind(args.kind.as_deref());
        let since_id = match args.since_request_id.as_deref() {
            Some(s) => Some(audit::parse_request_id_str(s)?),
            None => None,
        };
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            // The aggregator keys its `since` filter by `Instant`,
            // but we accept a request-id watermark from JS callers
            // (instants do not survive a JSON round-trip). Walk the
            // store once to find the most-recent `inserted_at` whose
            // request_id is `<= since_id`; an unmatched watermark
            // (id newer than anything in the store) yields `None` —
            // equivalent to "no records pass the filter".
            let since_instant: Option<std::time::Instant> = match since_id {
                None => None,
                Some(target_id) => {
                    let mut best: Option<std::time::Instant> = None;
                    store.for_each_record(&mut |inserted_at, record| {
                        if record.request_id <= target_id {
                            best = match best {
                                None => Some(inserted_at),
                                Some(prev) if inserted_at > prev => Some(inserted_at),
                                Some(prev) => Some(prev),
                            };
                        }
                    });
                    best
                }
            };
            let aggregator = BatchAuditAggregator::new(store.as_ref(), kind);
            let payload = aggregator.summarize(since_instant);
            let bytes = serde_json::to_vec(&payload).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("bundler batch summary serialization error: {e}"),
                )
            })?;
            Ok(Buffer::from(bytes))
        }))?
    }

    // =========================================================================
    // Typeinfo entry-points (typeinfo public host substrate)
    //
    // Wrap the host substrate methods
    // (`list_file_symbols`, `resolve_named_symbol_with_audit`,
    // `evaluate_type_expression_with_audit`) and project the host
    // outputs back across the FFI boundary.
    //
    // - `listSymbols` returns a JSON Buffer carrying a `Vec<FfiSymbolEntry>`.
    // - `resolveSymbolWithAudit` and `evaluateTypeExpressionWithAudit`
    //   return a `NapiTypeInfoResolveResult { typeExpr, auditRecord }`
    //   — both are JSON Buffers; consumers decode whichever they need.
    //
    // Audit emission follows the typeinfo contract: when
    // `auditEnabled = true` the underlying host method publishes
    // exactly one `RequestAuditRecord` to the host's audit store and
    // also returns the cloned record on the call stack. The
    // `auditRecord` field on `NapiTypeInfoResolveResult` carries that
    // record without polling the audit store; the store-based
    // `getLastAuditRecord` continues to work too.
    // =========================================================================

    /// Return the top-level symbol inventory for `canonical_id`.
    ///
    /// JSON Buffer carrying a `Vec<FfiSymbolEntry>` per the FFI mirror
    /// in `verter_protocol::typeinfo`. The call is bounded by the
    /// shallow-state size and does not emit an audit record (per §17
    /// "no audit; pure shallow read").
    #[napi(js_name = "listSymbols")]
    pub fn list_symbols(&self, canonical_id: String) -> Result<Buffer> {
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let entries = host.list_file_symbols(&canonical_id);
            let ffi: Vec<verter_protocol::typeinfo::FfiSymbolEntry> = entries
                .into_iter()
                .map(verter_ffi::convert::host_to_ffi_symbol_entry)
                .collect();
            typeinfo::encode_symbol_list(&ffi)
        }))?
    }

    /// Resolve `name` in `canonical_id`'s top-level scope and return
    /// the raised `TypeExpr` plus the produced `RequestAuditRecord`.
    ///
    /// `type_args` is an optional JSON Buffer carrying an array of
    /// `TypeExpr` values (the wire form of `TypeExprList`). Empty /
    /// missing means "no generic instantiation".
    ///
    /// `mode` is one of the canonical projection-mode tags
    /// (`"identity" | "navigate" | "shallow" | "expanded" |
    /// "skeleton"`). Pass `null` to take the host's default per §5.2.
    ///
    /// `typeExpr` is `null` when the symbol could not be resolved
    /// (unknown decl, lowering miss, suppressed by host policy).
    /// `auditRecord` is `null` when `auditEnabled = false`.
    #[napi(js_name = "resolveSymbolWithAudit")]
    pub fn resolve_symbol_with_audit(
        &self,
        canonical_id: String,
        name: String,
        type_args: Option<Buffer>,
        mode: Option<String>,
    ) -> Result<typeinfo::NapiTypeInfoResolveResult> {
        let exprs = typeinfo::decode_type_expr_list(type_args)?;
        let resolve_mode = typeinfo::parse_resolve_mode(mode)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let arc_args: Vec<std::sync::Arc<TypeExpr>> =
                exprs.into_iter().map(std::sync::Arc::new).collect();
            let (outcome, record) = host
                .resolve_named_symbol_with_audit(&canonical_id, &name, &arc_args, resolve_mode)
                .into_parts();
            let (resolved, error) = typeinfo::split_resolve_outcome(outcome);
            let type_expr_buf = match resolved {
                Some(node_id) => host
                    .project_node_to_type_expr(node_id)
                    .map(|expr| typeinfo::encode_type_expr(&expr))
                    .transpose()?,
                None => None,
            };
            let audit_buf = typeinfo::encode_stored_audit_record(&record)?;
            Ok(typeinfo::NapiTypeInfoResolveResult {
                typeExpr: type_expr_buf,
                auditRecord: audit_buf,
                error,
            })
        }))?
    }

    /// Evaluate a synthetic type expression in a file scope and return
    /// the raised `TypeExpr` plus the produced `RequestAuditRecord`.
    ///
    /// `request` is a JSON Buffer carrying a
    /// `verter_protocol::typeinfo::FfiEvaluateTypeExpressionRequest`.
    /// See `EvaluateTypeExpressionRequest` for the host shape.
    ///
    /// `typeExpr` is `null` when the expression could not be resolved.
    /// `auditRecord` is `null` when audit is disabled.
    #[napi(js_name = "evaluateTypeExpressionWithAudit")]
    pub fn evaluate_type_expression_with_audit(
        &self,
        request: Buffer,
    ) -> Result<typeinfo::NapiTypeInfoResolveResult> {
        let req = typeinfo::decode_evaluate_request(request)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let (outcome, record) = host.evaluate_type_expression_with_audit(req).into_parts();
            let (resolved, error) = typeinfo::split_resolve_outcome(outcome);
            let type_expr_buf = match resolved {
                Some(node_id) => host
                    .project_node_to_type_expr(node_id)
                    .map(|expr| typeinfo::encode_type_expr(&expr))
                    .transpose()?,
                None => None,
            };
            let audit_buf = typeinfo::encode_stored_audit_record(&record)?;
            Ok(typeinfo::NapiTypeInfoResolveResult {
                typeExpr: type_expr_buf,
                auditRecord: audit_buf,
                error,
            })
        }))?
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
) -> verter_semantic::analysis::types::ScriptAnalysisSnapshot {
    verter_semantic::analysis::types::ScriptAnalysisSnapshot {
        imports: snapshot.imports.clone(),
        module_references: snapshot.module_references.to_vec(),
        bindings: snapshot.bindings.clone(),
        macros: snapshot.macros.to_vec(),
        macro_type_deps: snapshot.macro_type_deps.to_vec(),
        flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
            snapshot.script_flags,
        ),
        exported_functions: Vec::new(),
        vue_api_calls: snapshot.vue_api_calls.to_vec(),
        dom_query_calls: snapshot.dom_query_calls.to_vec(),
        css_var_manipulations: snapshot.css_var_manipulations.to_vec(),
        script_binding_occurrences: snapshot.script_binding_occurrences.to_vec(),
        store_usages: snapshot.store_usages.to_vec(),
        store_definitions: snapshot.store_definitions.to_vec(),
        first_await_offset: None,
        type_enhancements: None,
        options_api: snapshot.options_api.clone(),
        nested_macro_calls: Vec::new(),
        is_typescript: snapshot.is_typescript,
        declaration_entries: Vec::new(),
    }
}

/// Convert a UTF-16 offset to a UTF-8 byte offset.
fn utf16_to_byte_offset(source: &str, utf16_offset: u32) -> u32 {
    verter_ffi::convert::utf16_to_byte_offset(source, utf16_offset)
}

/// Safe UTF-16 conversion that handles 0 as identity.
fn byte_offset_to_utf16_safe(source: &str, byte_offset: u32) -> u32 {
    verter_ffi::convert::byte_offset_to_utf16(source, byte_offset)
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
                verter_semantic::analysis::AnalyzedBindingKind::Function
                | verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => {
                    symbol_kind::FUNCTION
                }
                verter_semantic::analysis::AnalyzedBindingKind::Class => symbol_kind::CLASS,
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

        for m in snapshot.macros.iter() {
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

    for style in snapshot.styles.iter() {
        let css = match &style.css {
            Some(c) => c,
            None => continue,
        };

        for selector in &css.selectors {
            let parsed = match &selector.structure {
                Some(s) => s.clone(),
                None => match verter_semantic::analysis::style::parse_selector(&selector.text) {
                    Some(s) => s,
                    None => continue,
                },
            };

            let mut matches = Vec::new();
            for (idx, element) in template.elements.iter().enumerate() {
                let result = verter_semantic::analysis::selector_match::match_selector(
                    &parsed,
                    idx,
                    &template.elements,
                );
                matches.push(FfiElementMatch {
                    tag: element.tag.clone(),
                    span_start: byte_offset_to_utf16_safe(source, element.span.start),
                    span_end: byte_offset_to_utf16_safe(source, element.span.end),
                    result: match result {
                        verter_semantic::analysis::selector_match::MatchResult::Matches => {
                            "match".to_string()
                        }
                        verter_semantic::analysis::selector_match::MatchResult::MaybeMatches => {
                            "maybe".to_string()
                        }
                        verter_semantic::analysis::selector_match::MatchResult::NoMatch => {
                            "no".to_string()
                        }
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
// host-backed batch compile (NAPI surface)
//
// `NapiVerterHost::compile_many` is the canonical batch-compile
// entry point. It routes through the host's scheduler + dispatch +
// compile_cache and preserves the read/parse/process-once
// invariant.
// =============================================================================

use verter_session::host_compile;

/// One file in a batch compile call.
#[napi(object)]
pub struct NapiCompileBatchInput {
    pub canonicalId: String,
    pub source: Buffer,
    /// Requested compile cache mode ("stateless" / "content" /
    /// "session"). `None` inherits the batch `defaultMode`.
    pub requestedMode: Option<String>,
}

/// Caller-configurable options for [`NapiVerterHost::compile_many`].
#[napi(object)]
#[derive(Default)]
pub struct NapiCompileBatchOptions {
    /// Scheduler priority for batch upserts. Default: `"background"`.
    /// Use `"interactive"` when there is no concurrent interactive
    /// work (benchmarks / CI cold-start measurement).
    pub priority: Option<String>,
    /// Default compile cache mode for inputs whose `requestedMode` is
    /// unset. `None` resolves to "session" (the host default).
    pub defaultMode: Option<String>,
}

/// Result for a single original input position.
#[napi(object)]
pub struct NapiCompileBatchEntry {
    pub canonicalId: String,
    pub code: String,
    pub sourceMap: Option<String>,
    /// All compilation errors for this file. Empty on success.
    pub errors: Vec<String>,
    pub durationMs: f64,
    /// `true` iff this input was served from a warm cache entry under its
    /// classified mode — the fact-validated session slot (`Session`) or
    /// the content-addressed store (`Content`) — as decided by the single
    /// mode classifier. A request that a reason downgraded to `Stateless`
    /// never warm-hits and reports `false`. Sourced directly from the
    /// Rust `CompileBatchEntry.cache_hit` on the compile response.
    pub cacheHit: bool,
    /// Requested compile cache mode ("stateless" / "content" / "session").
    pub requestedMode: String,
    /// Actual compile cache mode the runtime ran under.
    pub actualMode: String,
    /// Highest-priority downgrade reason, or `None` when none fired.
    pub downgradeReason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The typed unsupported-language failure surfaces at the NAPI
    /// boundary in the SAME status family as the classify errors
    /// (`InvalidArg` — the request named a language the host cannot
    /// serve), not as a generic failure. DISCRIMINATING: the catch-all
    /// arm maps it to `GenericFailure`.
    #[test]
    fn unsupported_language_maps_to_invalid_arg_status() {
        let err = host_error(host::HostError::Scheduler(
            verter_scheduler::job::SchedulerError::UnsupportedLanguage {
                file_id: "/src/Box.svelte".to_string(),
                adapter_id: verter_session::FrameworkAdapterId::svelte(),
            },
        ));
        assert_eq!(err.status, Status::InvalidArg);
        assert!(
            err.reason.contains("svelte"),
            "the message names the adapter: {}",
            err.reason
        );
    }

    #[test]
    fn host_update_to_napi_exposes_module_references() {
        let result = host_update_to_napi(
            host::HostUpdateResult {
                module_references: vec![host::ScriptModuleReference {
                    syntax: verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport,
                    semantics: verter_semantic::analysis::ModuleReferenceSemantics::Import,
                    is_type_only: false,
                    raw_text: "`./${name}.vue`".to_string(),
                    literal_specifier: None,
                    finite_specifiers: vec!["./Foo.vue".to_string()],
                    static_prefix: Some("./".to_string()),
                    analyzability:
                        verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet,
                    span: verter_span::Span::new(4, 22),
                    expr_span: verter_span::Span::new(11, 21),
                }],
                ..host::HostUpdateResult::no_change("/test/App.vue".to_string())
            },
            Some("const x = import(`./${name}.vue`)"),
        );

        assert_eq!(result.moduleReferences.len(), 1);
        assert_eq!(result.moduleReferences[0].syntax, "dynamicImport");
        assert_eq!(result.moduleReferences[0].analyzability, "finiteSet");
        assert_eq!(result.moduleReferences[0].exprSpanStart, 11);
        assert_eq!(
            result.moduleReferences[0].finiteSpecifiers,
            vec!["./Foo.vue"]
        );
    }

    #[test]
    fn host_update_to_napi_exposes_export_signatures() {
        // Use the host to produce real export signatures from a barrel file
        let h = host::VerterHost::new_standalone(host::HostConfig::default());
        let host_result = h
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/barrel.ts".to_string()),
                input_id: "/src/barrel.ts".to_string(),
                source: std::sync::Arc::from(
                    "export { default as Button } from './Button.vue';\nexport type { Props } from './types';",
                ),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();

        assert!(
            !host_result.export_signatures.is_empty(),
            "barrel file must produce export signatures"
        );

        let result = host_update_to_napi(host_result, None);

        // Positive: re-export signatures mapped with camelCase fields
        let button = result.exportSignatures.iter().find(|s| s.name == "Button");
        assert!(button.is_some(), "Button re-export must be present");
        let button = button.unwrap();
        assert!(!button.isType);
        assert_eq!(button.reexportSource, Some("./Button.vue".to_string()));
        assert_eq!(button.reexportLocal, Some("default".to_string()));

        let props = result.exportSignatures.iter().find(|s| s.name == "Props");
        assert!(props.is_some(), "Props type re-export must be present");
        assert!(props.unwrap().isType);
    }

    #[test]
    fn host_update_to_napi_export_signatures_local_exports() {
        let h = host::VerterHost::new_standalone(host::HostConfig::default());
        let host_result = h
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/utils.ts".to_string()),
                input_id: "/src/utils.ts".to_string(),
                source: std::sync::Arc::from(
                    "export function greet() {}\nexport type Color = string;",
                ),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();

        let result = host_update_to_napi(host_result, None);

        let greet = result.exportSignatures.iter().find(|s| s.name == "greet");
        assert!(greet.is_some(), "local export must be present");
        // Negative: local exports must not have reexport fields
        assert!(greet.unwrap().reexportSource.is_none());
        assert!(greet.unwrap().reexportLocal.is_none());

        let color = result.exportSignatures.iter().find(|s| s.name == "Color");
        assert!(color.is_some(), "type export must be present");
        assert!(color.unwrap().isType);
    }

    #[test]
    fn host_update_to_napi_export_signatures_empty_on_no_change() {
        let result = host_update_to_napi(
            host::HostUpdateResult::no_change("/src/Empty.vue".to_string()),
            None,
        );
        assert!(
            result.exportSignatures.is_empty(),
            "no-change result must have empty exportSignatures"
        );
    }

    // the inline `compile_batch_files` helper smoke tests
    // were deleted along with the helper itself (host-bypassing
    // free-fn `compileBatch` is now `host.compileMany`). The
    // host-backed batch path is fully exercised by the host_compile
    // tests in verter_session and the JS-side E2E tests in
    // packages/native/index.spec.ts.
}
