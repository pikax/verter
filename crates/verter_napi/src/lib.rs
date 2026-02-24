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
    pub forceVapor: Option<bool>,
    pub forceJs: Option<bool>,
    pub sourceMap: Option<bool>,
    pub enableTypes: Option<bool>,
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
            force_vapor: n.forceVapor,
            force_js: n.forceJs,
            source_map: n.sourceMap,
            enable_types: n.enableTypes,
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

#[napi(object)]
pub struct NapiRemoveResult {
    pub canonicalId: String,
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
        host::VirtualNodeKind::TsxScript => NapiVirtualNodeKind {
            kind: "tsxScript".to_string(),
            index: None,
        },
        host::VirtualNodeKind::TsxTemplate => NapiVirtualNodeKind {
            kind: "tsxTemplate".to_string(),
            index: None,
        },
    }
}

fn host_severity_to_str(severity: &host::HostSeverity) -> &'static str {
    match severity {
        host::HostSeverity::Error => "error",
        host::HostSeverity::Warning => "warning",
        host::HostSeverity::Info => "info",
    }
}

fn host_diagnostics_to_napi(input: &host::DiagnosticsSnapshot) -> NapiDiagnosticsSnapshot {
    NapiDiagnosticsSnapshot {
        diagnostics: input
            .diagnostics
            .iter()
            .map(|d| NapiDiagnostic {
                severity: host_severity_to_str(&d.severity).to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                spanStart: d.span_start,
                spanEnd: d.span_end,
            })
            .collect(),
        hasErrors: input.has_errors,
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

fn host_update_to_napi(input: host::HostUpdateResult) -> NapiUpdateResult {
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
        diagnostics: host_diagnostics_to_napi(&input.diagnostics),
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
        parseDurationMs: input.parse_duration_ms,
    }
}

fn host_virtual_file_to_napi(input: host::VirtualFileResponse) -> NapiVirtualFileResponse {
    NapiVirtualFileResponse {
        id: input.id,
        code: input.code.to_string(),
        sourceMap: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: host_diagnostics_to_napi(&input.diagnostics),
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
// - Both: new, resolve, upsert, applyStyleOverrides, getVirtualFile,
//         listVirtualFiles, remove, setImportDependencies, getAnalysis
// - NAPI-only: processStyle (requires Node.js)
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
        let ffi_req = FfiUpsertRequest {
            canonical_id: request.canonicalId,
            input_id: request.inputId,
            source,
            file_kind: request.fileKind,
            aliases: request.aliases,
        };
        let host_req = ffi_upsert_to_host(ffi_req).map_err(ffi_err)?;
        catch_panic(std::panic::AssertUnwindSafe(|| self.inner.upsert(host_req)))?
            .map(host_update_to_napi)
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
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.apply_style_overrides(host_req)
        }))?
        .map(host_update_to_napi)
        .map_err(host_error)
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
        let ffi_query: FfiVirtualQuery = query.into();
        let host_query = ffi_virtual_query_to_host(ffi_query).map_err(ffi_err)?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_virtual_file(host_query)
        }))?
        .map(host_virtual_file_to_napi)
        .map_err(host_error)
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
}
