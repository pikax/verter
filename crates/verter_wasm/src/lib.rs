//! # verter_wasm — WebAssembly bindings for Verter
//!
//! `wasm-bindgen` binding layer that exposes [`verter_session::VerterHost`] to
//! the browser. Used by the Verter playground.
//!
//! ## API parity
//!
//! Exposes the same `VerterHost` API as [`verter_napi`], minus platform-only
//! features that require Node.js:
//!
//! - **Missing:** `processStyle` (CSS preprocessing needs Node.js).
//!
//! ## FFI architecture
//!
//! Uses `verter_ffi` types directly — they derive `Serialize`/`Deserialize`
//! with `#[serde(rename_all = "camelCase")]`, so `serde_wasm_bindgen` maps
//! them to/from JS objects with camelCase field names. All conversion logic
//! is shared via `verter_ffi::convert`.

use std::panic::AssertUnwindSafe;

use serde::{Deserialize, Serialize};
use verter_ffi::convert::*;
use verter_ffi::types::*;
use verter_protocol::types::{FfiComponentMeta, FfiComponentMetaResolution};
use verter_session as host;
use verter_session::component_meta_audit::{
    assertions::RequestAuditRecordAssertions, RequestAuditRecord,
};
use wasm_bindgen::prelude::*;

mod audit;
mod typeinfo;
use audit::{
    audit_record_list_to_json_string, audit_record_to_json_string, kind_matches_wasm,
    parse_bundler_kind_wasm, parse_compile_target_wasm, parse_request_id_str_wasm,
    stored_audit_record_to_json_string, AuditRecordFilterWasm, BundlerBatchSummaryArgsWasm,
    WorkspaceOpArgWasm,
};

/// WASM audit bundle — mirror of the NAPI binding's bundle shape.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmAuditBundle {
    analysis: FfiComponentMeta,
    resolution: FfiComponentMetaResolution,
    record: RequestAuditRecord,
}

/// Minimal decoder for `whyLoadedFromAuditJson` / `whyInstantiatedFromAuditJson`
/// — only `record` round-trips through the walker path. `analysis`
/// and `resolution` fields in the JSON are silently ignored by
/// serde's default unknown-field handling.
#[derive(Deserialize)]
struct WasmAuditBundleForWalker {
    record: RequestAuditRecord,
}

/// Parse a 32-char lowercase hex string into `Hash16`. WASM-error
/// variant of the NAPI helper with the same name.
fn parse_hash16_hex_wasm(hex: &str) -> Result<host::Hash16, JsValue> {
    if hex.len() != 32 {
        return Err(JsValue::from_str(&format!(
            "args_fingerprint_hex must be 32 hex chars (16 bytes), got {} chars",
            hex.len()
        )));
    }
    let mut out = [0u8; 16];
    for (i, byte_out) in out.iter_mut().enumerate() {
        let hi = hex
            .as_bytes()
            .get(i * 2)
            .and_then(|c| (*c as char).to_digit(16))
            .ok_or_else(|| {
                JsValue::from_str(&format!(
                    "args_fingerprint_hex[{idx}] not a hex digit",
                    idx = i * 2
                ))
            })?;
        let lo = hex
            .as_bytes()
            .get(i * 2 + 1)
            .and_then(|c| (*c as char).to_digit(16))
            .ok_or_else(|| {
                JsValue::from_str(&format!(
                    "args_fingerprint_hex[{idx}] not a hex digit",
                    idx = i * 2 + 1
                ))
            })?;
        *byte_out = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

// Re-imports for code actions and diagnostics
use verter_actions::{ActionContext, ActionEngine};
use verter_diagnostics::rules::RuleRegistry;
use verter_diagnostics::Linter;

/// WASM module initialiser — called automatically by the JS runtime when the
/// module is instantiated.
///
/// Installs [`console_error_panic_hook`] (when the feature is enabled) so that
/// Rust panics produce readable stack traces in the browser console instead of
/// the default `unreachable` error.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

// =============================================================================
// Helpers
// =============================================================================

fn parse_wasm_input<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsValue::from_str(&format!("Invalid host input: {}", e)))
}

fn to_wasm_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    // serde_wasm_bindgen 0.6 serializes `serialize_map` calls as JS `Map` objects by default.
    // `JSON.stringify(new Map(...))` returns `{}`, which breaks the playground's Raw view and
    // any JS code that accesses fields via plain property access.
    // All analysis types use `serialize_struct` (not `serialize_map`), so this flag is a
    // defense-in-depth guard against future regressions.
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&format!("Host serialization error: {}", e)))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmDependencyResolution {
    specifier: String,
    resolved_canonical_id: Option<String>,
    possible_canonical_ids: Option<Vec<String>>,
}

/// Run a closure, converting any panic into a `JsValue` error.
/// Prevents Rust panics from crashing the WASM runtime and poisoning
/// RefCell borrow state.
fn catch_panic<T>(f: impl FnOnce() -> T) -> Result<T, JsValue> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|panic_info| {
        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown internal error".to_string()
        };
        JsValue::from_str(&format!("internal compiler error: {msg}"))
    })
}

fn ffi_err(msg: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&msg.to_string())
}

fn host_err(err: host::HostError) -> JsValue {
    JsValue::from_str(&host_error_to_string(&err))
}

fn ffi_module_reference_syntax_from_str(
    syntax: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceSyntax, JsValue> {
    match syntax {
        "staticImport" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::StaticImport),
        "exportFrom" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom),
        "dynamicImport" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport),
        "requireCall" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::RequireCall),
        other => Err(ffi_err(format!("unknown module reference syntax: {other}"))),
    }
}

fn ffi_module_reference_semantics_from_str(
    semantics: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceSemantics, JsValue> {
    match semantics {
        "import" => Ok(verter_semantic::analysis::ModuleReferenceSemantics::Import),
        "require" => Ok(verter_semantic::analysis::ModuleReferenceSemantics::Require),
        other => Err(ffi_err(format!(
            "unknown module reference semantics: {other}"
        ))),
    }
}

fn ffi_module_reference_analyzability_from_str(
    analyzability: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceAnalyzability, JsValue> {
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

fn ffi_module_reference_to_analysis(
    input: FfiModuleReference,
) -> Result<verter_semantic::analysis::AnalyzedModuleReference, JsValue> {
    Ok(verter_semantic::analysis::AnalyzedModuleReference {
        syntax: ffi_module_reference_syntax_from_str(&input.syntax)?,
        semantics: ffi_module_reference_semantics_from_str(&input.semantics)?,
        is_type_only: input.is_type_only,
        span: verter_span::Span::new(input.span_start, input.span_end),
        expr_span: verter_span::Span::new(input.expr_span_start, input.expr_span_end),
        raw_text: input.raw_text,
        literal_specifier: input.literal_specifier,
        finite_specifiers: input.finite_specifiers,
        static_prefix: input.static_prefix,
        analyzability: ffi_module_reference_analyzability_from_str(&input.analyzability)?,
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

// =============================================================================
// VerterHost (in-memory virtual file host)
//
// API parity with NAPI (crates/verter_napi):
// - Both: new, resolve, upsert, applyBlockOverrides, getVirtualFile,
//         listVirtualFiles, remove, setImportDependencies, getAnalysis
// - NAPI-only: processStyle (requires Node.js)
// =============================================================================

/// In-memory virtual file host for Vue SFC compilation (WASM variant).
///
/// Manages a collection of Vue SFCs and their compiled virtual files (script,
/// template, styles). Files are upserted as source, then lazily compiled into
/// virtual outputs that can be requested individually.
///
/// This is the browser-side counterpart of `NapiVerterHost`, used by the
/// Verter playground.
#[wasm_bindgen(js_name = VerterHost)]
pub struct WasmVerterHost {
    inner: std::sync::Arc<host::VerterHost>,
}

#[wasm_bindgen(js_class = VerterHost)]
impl WasmVerterHost {
    /// Creates a new `VerterHost` with the given configuration.
    ///
    /// - `config` — a JS object with optional host settings (dev mode,
    ///   compile error policy, analysis level, etc.). Pass `undefined` or
    ///   `null` to use defaults.
    ///
    /// Throws if the configuration contains invalid values (e.g. an
    /// unrecognised `compileErrorPolicy` string).
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmVerterHost, JsValue> {
        let ffi_config = if config.is_undefined() || config.is_null() {
            FfiHostConfig::default()
        } else {
            parse_wasm_input::<FfiHostConfig>(config)?
        };
        Ok(Self {
            inner: std::sync::Arc::new(host::VerterHost::new_standalone(
                ffi_config_to_host(ffi_config).map_err(ffi_err)?,
            )),
        })
    }

    /// Resolves a raw import ID (e.g. `./Foo.vue?type=style&index=0`) into its
    /// canonical ID, virtual node kind, and bundler/LSP identifiers.
    ///
    /// Returns `null` (serialised via `serde_wasm_bindgen`) if the ID does
    /// not match any file tracked by this host.
    #[wasm_bindgen]
    pub fn resolve(&self, raw_id: &str) -> Result<JsValue, JsValue> {
        let output = catch_panic(|| self.inner.resolve(raw_id).map(host_resolved_id_to_ffi))?;
        to_wasm_value(&output)
    }

    /// Inserts or updates a file in the host.
    ///
    /// Parses the SFC source, diffs it against the previously stored version
    /// (if any), and returns a detailed changeset describing which virtual
    /// nodes changed, any diagnostics, and external source requests that the
    /// caller must resolve.
    ///
    /// - `request` — a JS object with `inputId` (string), `source` (string),
    ///   and optional `canonicalId`, `fileKind`, and `aliases` fields.
    ///
    /// Throws if the request is malformed or the file kind is unrecognised.
    #[wasm_bindgen]
    pub fn upsert(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let ffi_req = parse_wasm_input::<FfiUpsertRequest>(request)?;
        let source_for_spans = ffi_req.source.clone();
        let host_req = ffi_upsert_to_host(ffi_req).map_err(ffi_err)?;
        let result = catch_panic(|| self.inner.upsert(host_req))?.map_err(host_err)?;
        to_wasm_value(&host_update_to_ffi(result, Some(source_for_spans.as_str())))
    }

    /// Replaces one or more blocks with preprocessed content (e.g. the output
    /// of Pug, CoffeeScript, SCSS, or custom block preprocessors) and
    /// recompiles affected virtual nodes.
    ///
    /// This is the unified API that handles template, script, style, AND
    /// custom block preprocessing.
    ///
    /// Returns the same changeset structure as [`upsert`](Self::upsert).
    ///
    /// Throws if the canonical ID is unknown or the request is malformed.
    #[wasm_bindgen(js_name = applyBlockOverrides)]
    pub fn apply_block_overrides(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let ffi_req = parse_wasm_input::<FfiBlockOverrideRequest>(request)?;
        let host_req = ffi_block_override_to_host(ffi_req).map_err(ffi_err)?;
        let result =
            catch_panic(|| self.inner.apply_block_overrides(host_req))?.map_err(host_err)?;
        let source = self.inner.get_source(&result.canonical_id);
        to_wasm_value(&host_update_to_ffi(result, source.as_deref()))
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
    /// Throws if the query is invalid or the file is not found.
    #[wasm_bindgen(js_name = getVirtualFile)]
    pub fn get_virtual_file(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let ffi_query = parse_wasm_input::<FfiVirtualQuery>(query)?;
        let canonical_for_source = if let Some(canonical) = ffi_query.canonical_id.as_ref() {
            Some(canonical.clone())
        } else if let Some(raw_id) = ffi_query.raw_id.as_ref() {
            self.inner.resolve(raw_id).map(|r| r.canonical_id)
        } else {
            None
        };
        let host_query = ffi_virtual_query_to_host(ffi_query).map_err(ffi_err)?;
        let result = catch_panic(|| self.inner.get_virtual_file(host_query))?.map_err(host_err)?;
        let source = canonical_for_source
            .as_deref()
            .and_then(|canonical| self.inner.get_source(canonical));
        to_wasm_value(&host_virtual_file_to_ffi(result, source.as_deref()))
    }

    /// Lists all virtual node kinds for a given canonical file ID.
    ///
    /// Returns a JS array of node kind objects (e.g. `{ kind: "style", index: 0 }`)
    /// that can be passed to [`get_virtual_file`](Self::get_virtual_file).
    /// Returns an empty array if the canonical ID is not tracked by the host.
    #[wasm_bindgen(js_name = listVirtualFiles)]
    pub fn list_virtual_files(&self, canonical_id: &str) -> Result<JsValue, JsValue> {
        let output: Vec<FfiVirtualNodeKind> = catch_panic(|| {
            self.inner
                .list_virtual_files(canonical_id)
                .iter()
                .map(host_node_kind_to_ffi)
                .collect()
        })?;
        to_wasm_value(&output)
    }

    /// Removes a file from the host by its canonical ID or any registered alias.
    ///
    /// All associated virtual nodes and cached compilations are discarded.
    /// Returns `null` if no file matched the given ID.
    #[wasm_bindgen]
    pub fn remove(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let output = catch_panic(|| {
            self.inner
                .remove(canonical_or_alias)
                .map(host_remove_to_ffi)
        })?;
        to_wasm_value(&output)
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    ///
    /// Returns `null` if the file does not exist in the host. When
    /// `analysis_level` is not `"full"`, computes analysis on demand from
    /// the stored source.
    ///
    /// **Note:** Returns a native JS object (via `serde_wasm_bindgen`).
    /// The NAPI variant (`verter_napi`) returns a JSON *string* instead —
    /// consumers must `JSON.parse()` the NAPI result.
    #[wasm_bindgen(js_name = getAnalysis)]
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let output = catch_panic(|| self.inner.get_analysis(canonical_or_alias))?;
        to_wasm_value(&output)
    }

    /// Retrieves the combined IDE output (TSX or JSX) for type checking.
    ///
    /// This is a dedicated API separate from virtual files. IDE output is
    /// only consumed by the LSP and playground, never by bundlers.
    ///
    /// Returns `{ code: string, sourceMap?: string, isJsx: boolean }` or `null` if no IDE
    /// output is available for the given file and profile.
    #[wasm_bindgen(js_name = getIde)]
    pub fn get_ide(&self, canonical_id: &str, profile: JsValue) -> Result<JsValue, JsValue> {
        let ffi_profile: Option<FfiCompileProfile> = if profile.is_undefined() || profile.is_null()
        {
            None
        } else {
            Some(parse_wasm_input(profile)?)
        };
        let host_profile = ffi_profile_to_host(ffi_profile).map_err(ffi_err)?;
        let result = catch_panic(|| self.inner.get_ide(canonical_id, &host_profile))?;
        let sfc_source = self.inner.get_source(canonical_id);
        to_wasm_value(&result.map(|r| {
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
                verter_ffi::convert::convert_destructured_block_meta(
                    &bindings,
                    meta.block_start,
                    meta.block_end,
                    sfc,
                    &r.code,
                    verter_ffi::convert::OffsetEncoding::Utf16,
                )
            });
            FfiIdeResponse {
                code: r.code.to_string(),
                source_map: r.source_map.map(|s| s.to_string()),
                is_jsx: r.is_jsx,
                destructured_block,
            }
        }))
    }

    /// Ensure the IDE (`CachedTsx`) projection exists for a file + profile.
    ///
    /// The explicit IDE-ensure path: it compiles the carrier's IDE surface
    /// (never requesting the runtime `Main` node), so a Main-less carrier
    /// (Svelte) populates its `CachedTsx` and a subsequent [`get_ide`](Self::get_ide)
    /// succeeds. `getIde` itself stays a pure cached read.
    ///
    /// The caller profile is OPTIONAL and is normalized to an IDE/TSX-bearing
    /// target INTERNALLY, so a default / bundler profile (no TSX bit) still
    /// produces the IDE surface. Returns `true` whenever the carrier HAS an IDE
    /// surface — regardless of the caller's runtime target — and `false` ONLY
    /// for a genuine no-IDE-surface file (a non-carrier / plain script). A real
    /// failure (missing source / compile error) throws.
    #[wasm_bindgen(js_name = ensureIdeCompiled)]
    pub fn ensure_ide_compiled(
        &self,
        canonical_id: &str,
        profile: JsValue,
    ) -> Result<bool, JsValue> {
        let ffi_profile: Option<FfiCompileProfile> = if profile.is_undefined() || profile.is_null()
        {
            None
        } else {
            Some(parse_wasm_input(profile)?)
        };
        let host_profile = ffi_profile_to_host(ffi_profile).map_err(ffi_err)?;
        catch_panic(|| self.inner.ensure_ide_compiled(canonical_id, &host_profile))?
            .map_err(host_err)
    }

    /// Retrieve TSC declaration output for a file.
    ///
    /// Generates a minimal TypeScript declaration file for a Vue SFC.
    /// Unlike `getIde`, this does NOT require a prior compilation pass.
    ///
    /// Returns `{ code: string, sourceMap?: string, isJsx: boolean }` or `null`.
    #[wasm_bindgen(js_name = getPublicApi)]
    pub fn get_public_api(&self, canonical_id: &str) -> Result<JsValue, JsValue> {
        let result = catch_panic(|| self.inner.get_public_api(canonical_id))?;
        to_wasm_value(&result.map(|r| FfiIdeResponse {
            code: r.code.to_string(),
            source_map: r.source_map.map(|s| s.to_string()),
            is_jsx: false,
            destructured_block: None,
        }))
    }

    /// Runs cross-file analysis and returns prop constness optimizations.
    ///
    /// Builds a render tree from all compiled SFCs' template analysis data,
    /// aggregates prop constness across all parent call sites, and validates
    /// provide/inject chains. Returns which files have changed constness
    /// hints and any diagnostics.
    ///
    /// Should be called after all files are upserted and compiled (e.g.
    /// after a preCompile pass).
    #[wasm_bindgen(js_name = computeCrossFileOptimizations)]
    pub fn compute_cross_file_optimizations(&self) -> Result<JsValue, JsValue> {
        let result = catch_panic(|| self.inner.compute_cross_file_optimizations())?;
        let ffi = host_cross_file_result_to_ffi(result);
        to_wasm_value(&ffi)
    }

    /// Records the resolved import dependencies for a file.
    ///
    /// Called after resolving the exact/finite `moduleReferences` returned by
    /// [`upsert`](Self::upsert). This enables cross-file type resolution
    /// (e.g. following `import type { Props } from './types'` chains) when
    /// recompiling dependent files.
    ///
    /// - `canonical_or_alias` — the file whose dependencies are being set.
    /// - `resolutions` — a JS array of `{ specifier, resolvedCanonicalId?, possibleCanonicalIds? }`.
    #[wasm_bindgen(js_name = setImportDependencies)]
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolutions: JsValue,
    ) -> Result<(), JsValue> {
        let records: Vec<WasmDependencyResolution> = parse_wasm_input(resolutions)?;
        let resolutions = records
            .into_iter()
            .map(|r| host::DependencyResolution {
                specifier: r.specifier,
                resolved_canonical_id: r.resolved_canonical_id,
                possible_canonical_ids: r.possible_canonical_ids.unwrap_or_default(),
            })
            .collect();
        catch_panic(|| {
            self.inner
                .set_import_dependencies(canonical_or_alias, resolutions)
        })
    }

    /// Returns the exact and finite-set module reference candidates in encounter order.
    ///
    /// Unknown-dynamic references are skipped entirely.
    #[wasm_bindgen(js_name = collectResolvableModuleReferenceSpecifiers)]
    pub fn collect_resolvable_module_reference_specifiers(
        &self,
        module_references: JsValue,
    ) -> Result<JsValue, JsValue> {
        let module_references = parse_wasm_input::<Vec<FfiModuleReference>>(module_references)?
            .into_iter()
            .map(ffi_module_reference_to_analysis)
            .collect::<Result<Vec<_>, _>>()?;
        let specifiers =
            verter_semantic::analysis::project_resolver::collect_resolvable_module_reference_specifiers(
                &module_references,
            );
        to_wasm_value(&specifiers)
    }

    /// Resolves exact and finite module reference candidates against a caller-provided
    /// in-memory known-file set, without reading from disk.
    #[wasm_bindgen(js_name = resolveKnownModuleReferenceDependencies)]
    pub fn resolve_known_module_reference_dependencies(
        &self,
        owner_id: &str,
        module_references: JsValue,
        known_ids: JsValue,
        extensions: JsValue,
    ) -> Result<JsValue, JsValue> {
        let module_references = parse_wasm_input::<Vec<FfiModuleReference>>(module_references)?
            .into_iter()
            .map(ffi_module_reference_to_analysis)
            .collect::<Result<Vec<_>, _>>()?;
        let known_ids: Vec<String> = parse_wasm_input(known_ids)?;
        let extensions = if extensions.is_undefined() || extensions.is_null() {
            default_known_dependency_extensions()
        } else {
            parse_wasm_input::<Vec<String>>(extensions)?
        };
        let resolved =
            verter_semantic::analysis::project_resolver::resolve_known_module_reference_dependencies(
                owner_id,
                &module_references,
                &known_ids,
                &extensions,
            );
        to_wasm_value(&resolved)
    }

    /// Runs lint rules against a file's analysis data and returns diagnostics.
    ///
    /// Takes a canonical ID (or alias), retrieves its analysis data from the
    /// host, and runs the linter with the given config. Returns an array of
    /// lint diagnostics.
    ///
    /// - `canonical_or_alias` — the file to lint.
    /// - `config` — optional JS object with lint config (preset, rule overrides).
    ///   Pass `undefined` or `null` for defaults.
    #[wasm_bindgen]
    pub fn lint(&self, canonical_or_alias: &str, config: JsValue) -> Result<JsValue, JsValue> {
        let lint_config = if config.is_undefined() || config.is_null() {
            verter_diagnostics::LintConfig::default()
        } else {
            parse_wasm_input::<verter_diagnostics::LintConfig>(config)?
        };

        let analysis = catch_panic(|| self.inner.get_analysis(canonical_or_alias))?;

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
        let source = self.inner.get_source(canonical_or_alias);
        let diagnostics = lint_diagnostics_to_utf16(diagnostics, source.as_deref());
        to_wasm_value(&diagnostics)
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
    #[wasm_bindgen(js_name = getCodeActions)]
    pub fn get_code_actions(
        &self,
        canonical_or_alias: &str,
        offset: u32,
    ) -> Result<JsValue, JsValue> {
        let analysis = catch_panic(|| self.inner.get_analysis(canonical_or_alias))?;
        let source = self.inner.get_source(canonical_or_alias);

        let actions = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => {
                // Convert UTF-16 offset to byte offset for the action engine
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
                    file_id: canonical_or_alias,
                    diagnostics: &diag_set,
                    template: snapshot.template.as_deref(),
                    script: Some(&script),
                    styles: &snapshot.styles,
                };

                // Collect fixes for diagnostics that overlap the cursor
                let mut actions = Vec::new();
                for diag in diag_set.iter() {
                    if diag.span.start <= byte_offset && byte_offset <= diag.span.end {
                        actions.extend(engine.fixes_for(diag, &ctx));
                    }
                }

                // Also collect position-based actions (refactorings)
                actions.extend(engine.actions_at(byte_offset, &ctx));

                // Deduplicate by title
                let mut seen = std::collections::HashSet::new();
                actions.retain(|a| seen.insert(a.title.clone()));

                actions
                    .iter()
                    .map(|a| code_action_to_ffi(a, source))
                    .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        };

        to_wasm_value(&actions)
    }

    /// Returns metadata for all registered lint rules.
    ///
    /// Used by the lint rule browser UI to display available rules,
    /// their categories, and default severities.
    #[wasm_bindgen(js_name = getLintRuleMetadata)]
    pub fn get_lint_rule_metadata(&self) -> Result<JsValue, JsValue> {
        let registry = RuleRegistry::default();
        let metadata: Vec<FfiLintRuleMetadata> = registry
            .rules()
            .iter()
            .map(|rule| lint_rule_to_ffi_metadata(rule.as_ref()))
            .collect();
        to_wasm_value(&metadata)
    }

    /// Returns document symbols for a file (outline / Ctrl+Shift+O).
    ///
    /// Generates a hierarchical tree of symbols: SFC blocks at the top,
    /// with script bindings, template components, and style classes as
    /// children.
    #[wasm_bindgen(js_name = getDocumentSymbols)]
    pub fn get_document_symbols(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let analysis = catch_panic(|| self.inner.get_analysis(canonical_or_alias))?;
        let source = self.inner.get_source(canonical_or_alias);

        let symbols = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => {
                build_document_symbols_from_analysis(&snapshot, source)
            }
            _ => Vec::new(),
        };

        to_wasm_value(&symbols)
    }

    /// Matches CSS selectors against template elements, returning a
    /// three-valued match matrix.
    ///
    /// Each selector is tested against each template element, producing
    /// "match", "maybe", or "no" results. Used by the CSS selector
    /// matching visualization panel.
    #[wasm_bindgen(js_name = matchCssSelectors)]
    pub fn match_css_selectors(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let analysis = catch_panic(|| self.inner.get_analysis(canonical_or_alias))?;
        let source = self.inner.get_source(canonical_or_alias);

        let results = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => build_selector_match_results(&snapshot, source),
            _ => Vec::new(),
        };

        to_wasm_value(&results)
    }

    // =========================================================================
    // Typed audit entry-points (mirrors the NAPI surface)
    // =========================================================================

    /// Run a single type-resolution query through the shared dispatch
    /// and return the produced `RequestAuditRecord` as a JSON string.
    /// Resolves `decl_name` in the top-level scope of `canonical_id`.
    /// Returns `null` when audit is disabled.
    #[wasm_bindgen(js_name = "resolveTypeWithAudit")]
    pub fn resolve_type_with_audit(
        &self,
        canonical_id: &str,
        decl_name: &str,
    ) -> Result<JsValue, JsValue> {
        use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
        let host = std::sync::Arc::clone(&self.inner);
        let canonical_id_owned = canonical_id.to_string();
        let decl_name_owned = decl_name.to_string();
        catch_panic(AssertUnwindSafe(move || {
            let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: ScopeId {
                    canonical_id: std::sync::Arc::<str>::from(canonical_id_owned.as_str()),
                    local_scope: None,
                },
                name: std::sync::Arc::<str>::from(decl_name_owned.as_str()),
            });
            let record = host
                .resolve_type_with_audit(key, &canonical_id_owned)
                .audit()
                .clone();
            stored_audit_record_to_json_string(&record)
        }))?
    }

    /// Compile `canonical_id` for the requested codegen target and
    /// return the produced `RequestAuditRecord` as a JSON string.
    /// Accepted target names: `BUNDLER`, `IDE`, `ANALYSIS`, `META`,
    /// `TSX`, `TSC`. Returns `null` when audit is disabled.
    #[wasm_bindgen(js_name = "compileWithAudit")]
    pub fn compile_with_audit(&self, canonical_id: &str, target: &str) -> Result<JsValue, JsValue> {
        let target_value = parse_compile_target_wasm(target)?;
        let host = std::sync::Arc::clone(&self.inner);
        let canonical_id_owned = canonical_id.to_string();
        catch_panic(AssertUnwindSafe(move || {
            let record = host
                .compile_with_audit(&canonical_id_owned, target_value)
                .audit()
                .clone();
            stored_audit_record_to_json_string(&record)
        }))?
    }

    /// Materialise the `AnalysisReady` artifact for `canonical_id`
    /// under audit and return the produced `RequestAuditRecord` as a
    /// JSON string. Returns `null` when audit is disabled or the
    /// canonical does not exist.
    ///
    /// WASM-only stub: the underlying
    /// `VerterHost::analyze_with_audit` requires the scheduler-backed
    /// `IndexedReady` materialisation path which is not built for the
    /// `wasm32` target. Calling this throws on WASM; consumers should
    /// drive the analysis through the native `@verter/native` package.
    #[wasm_bindgen(js_name = "analyzeWithAudit")]
    pub fn analyze_with_audit(&self, _canonical_id: &str) -> Result<JsValue, JsValue> {
        Err(JsValue::from_str(
            "analyzeWithAudit is unavailable in WASM (scheduler-backed analysis not built for wasm32); \
             use @verter/native for audited analysis requests",
        ))
    }

    /// Drive a workspace operation under audit and return the
    /// produced `RequestAuditRecord` as a JSON string. The `op_json`
    /// argument is shaped as `{ "type": "AuditResolve", "specifier",
    /// "from" }` / `{ "type": "DepGraphTraverse", "root" }` / `{
    /// "type": "ResolverWalk", "specifier" }`.
    #[wasm_bindgen(js_name = "auditWorkspaceOp")]
    pub fn audit_workspace_op(&self, op_json: &str) -> Result<JsValue, JsValue> {
        let arg: WorkspaceOpArgWasm = serde_json::from_str(op_json)
            .map_err(|e| JsValue::from_str(&format!("invalid workspace op shape: {e}")))?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(AssertUnwindSafe(move || {
            let workspace_op: verter_audit::WorkspaceOp = arg.into();
            let record = host.audit_workspace_op(workspace_op);
            audit_record_to_json_string(&record)
        }))?
    }

    /// Drain the most-recent `RequestAuditRecord` from the host's
    /// audit store. Returns `null` when the store is empty. The
    /// returned record is removed from the store.
    #[wasm_bindgen(js_name = "getLastAuditRecord")]
    pub fn get_last_audit_record(&self) -> Result<JsValue, JsValue> {
        use verter_audit::batch::AuditRecordSource;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let mut latest_id: Option<u64> = None;
            let mut latest_at: Option<verter_audit::instant::Instant> = None;
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
                return Ok(JsValue::NULL);
            };
            match store.take(id) {
                Some(rec) => audit_record_to_json_string(&rec),
                None => Ok(JsValue::NULL),
            }
        }))?
    }

    /// Non-destructive filtered query over the host's audit store.
    /// Returns a JSON-string array of matching records. The
    /// `filter_json` argument carries `{ kind?, sinceRequestId?,
    /// limit? }` (any combination — independent narrowing).
    #[wasm_bindgen(js_name = "getAuditRecords")]
    pub fn get_audit_records(&self, filter_json: JsValue) -> Result<JsValue, JsValue> {
        use verter_audit::batch::AuditRecordSource;
        let filter: AuditRecordFilterWasm = if filter_json.is_undefined() || filter_json.is_null() {
            AuditRecordFilterWasm::default()
        } else {
            parse_wasm_input(filter_json)?
        };
        let kind_filter = filter.kind;
        let since = match filter.since_request_id.as_deref() {
            Some(s) => Some(parse_request_id_str_wasm(s)?),
            None => None,
        };
        let limit = filter.limit.map(|n| n as usize);
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let mut collected: Vec<verter_audit::RequestAuditRecord> = Vec::new();
            store.for_each_record(&mut |_inserted_at, record| {
                if let Some(filter_kind) = kind_filter.as_deref() {
                    if !kind_matches_wasm(filter_kind, &record.kind) {
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
            audit_record_list_to_json_string(&collected)
        }))?
    }

    /// Run the bundler-batch aggregator over the host's audit store
    /// and return the produced `BundlerBatchPayload` as a JSON
    /// string. The `args_json` argument carries `{ kind?,
    /// sinceRequestId? }` (defaults: `Vite`, no-watermark).
    #[wasm_bindgen(js_name = "getBundlerBatchSummary")]
    pub fn get_bundler_batch_summary(&self, args_json: JsValue) -> Result<JsValue, JsValue> {
        use verter_audit::batch::{AuditRecordSource, BatchAuditAggregator};
        let args: BundlerBatchSummaryArgsWasm = if args_json.is_undefined() || args_json.is_null() {
            BundlerBatchSummaryArgsWasm::default()
        } else {
            parse_wasm_input(args_json)?
        };
        let kind = parse_bundler_kind_wasm(args.kind.as_deref());
        let since_id = match args.since_request_id.as_deref() {
            Some(s) => Some(parse_request_id_str_wasm(s)?),
            None => None,
        };
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let since_instant: Option<verter_audit::instant::Instant> = match since_id {
                None => None,
                Some(target_id) => {
                    let mut best: Option<verter_audit::instant::Instant> = None;
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
            serde_json::to_string(&payload)
                .map(|s| JsValue::from_str(&s))
                .map_err(|e| {
                    JsValue::from_str(&format!("bundler batch summary serialization error: {e}"))
                })
        }))?
    }

    // =========================================================================
    // Typeinfo entry-points (typeinfo public host substrate) — WASM
    //
    // Mirror the NAPI surface (`NapiVerterHost::list_symbols`,
    // `resolveSymbolWithAudit`, `evaluateTypeExpressionWithAudit`).
    // The encoding shape is JSON strings rather than `Buffer` blobs to
    // match the existing WASM convention; the wire schema is identical.
    // =========================================================================

    /// Return the top-level symbol inventory for `canonical_id` as a
    /// JSON string carrying a `Vec<FfiSymbolEntry>`.
    #[wasm_bindgen(js_name = "listSymbols")]
    pub fn list_symbols(&self, canonical_id: &str) -> Result<String, JsValue> {
        let host = std::sync::Arc::clone(&self.inner);
        let canonical_id_owned = canonical_id.to_string();
        catch_panic(AssertUnwindSafe(move || {
            let entries = host.list_file_symbols(&canonical_id_owned);
            let ffi: Vec<verter_protocol::typeinfo::FfiSymbolEntry> = entries
                .into_iter()
                .map(verter_ffi::convert::host_to_ffi_symbol_entry)
                .collect();
            crate::typeinfo::encode_symbol_list(&ffi)
        }))?
    }

    /// Resolve `name` in `canonical_id`'s scope. Returns a JSON string
    /// carrying `{ typeExpr, auditRecord }` per the typeinfo contract.
    ///
    /// `type_args_json` is an optional JSON string carrying an array of
    /// `TypeExpr` values. `mode` is one of the projection-mode tags or
    /// `null` for the host default.
    #[wasm_bindgen(js_name = "resolveSymbolWithAudit")]
    pub fn resolve_symbol_with_audit(
        &self,
        canonical_id: &str,
        name: &str,
        type_args_json: Option<String>,
        mode: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let exprs = crate::typeinfo::decode_type_expr_list(type_args_json)?;
        let resolve_mode = crate::typeinfo::parse_resolve_mode(mode)?;
        let host = std::sync::Arc::clone(&self.inner);
        let canonical_id_owned = canonical_id.to_string();
        let name_owned = name.to_string();
        catch_panic(AssertUnwindSafe(move || {
            let arc_args: Vec<std::sync::Arc<verter_type_expr::TypeExpr>> =
                exprs.into_iter().map(std::sync::Arc::new).collect();
            // Wire-boundary resolution: the symbolic `TypeExpr` payloads
            // lower to semantic-graph node ids INSIDE the audited request,
            // under the SAME store view the resolution runs against (the
            // transient symbolic IR stops at this boundary). A lowering miss
            // surfaces as a `null` typeExpr WITH its audit record.
            let (outcome, record) = host
                .resolve_named_symbol_wire_with_audit(
                    &canonical_id_owned,
                    &name_owned,
                    &arc_args,
                    resolve_mode,
                )
                .into_parts();
            let (resolved, error) = crate::typeinfo::split_resolve_outcome(outcome);
            // Bytes facade: `verter_session` wire-encodes the `TypeExpr` to
            // UTF-8 JSON internally (through the sealed output capability);
            // the WASM adapter only decodes the bytes to a `String`.
            let type_expr_json = match resolved {
                Some(node_id) => host
                    .project_node_to_type_expr_json_bytes(node_id)
                    .map(|bytes| {
                        String::from_utf8(bytes).map_err(|e| {
                            JsValue::from_str(&format!("type-expr utf-8 decode error: {e}"))
                        })
                    })
                    .transpose()?,
                None => None,
            };
            let audit_json = crate::typeinfo::encode_stored_audit_record(&record)?;
            let result = crate::typeinfo::WasmTypeInfoResolveResult {
                type_expr: type_expr_json,
                audit_record: audit_json,
                error,
            };
            to_wasm_value(&result)
        }))?
    }

    /// Evaluate a synthetic type expression in a file scope. Returns
    /// `{ typeExpr, auditRecord }` per the typeinfo contract.
    ///
    /// `request_json` is a JSON string carrying a
    /// `verter_protocol::typeinfo::FfiEvaluateTypeExpressionRequest`.
    #[wasm_bindgen(js_name = "evaluateTypeExpressionWithAudit")]
    pub fn evaluate_type_expression_with_audit(
        &self,
        request_json: &str,
    ) -> Result<JsValue, JsValue> {
        let req = crate::typeinfo::decode_evaluate_request(request_json)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(AssertUnwindSafe(move || {
            let (outcome, record) = host.evaluate_type_expression_with_audit(req).into_parts();
            let (resolved, error) = crate::typeinfo::split_resolve_outcome(outcome);
            // Bytes facade: `verter_session` wire-encodes the `TypeExpr` to
            // UTF-8 JSON internally (through the sealed output capability);
            // the WASM adapter only decodes the bytes to a `String`.
            let type_expr_json = match resolved {
                Some(node_id) => host
                    .project_node_to_type_expr_json_bytes(node_id)
                    .map(|bytes| {
                        String::from_utf8(bytes).map_err(|e| {
                            JsValue::from_str(&format!("type-expr utf-8 decode error: {e}"))
                        })
                    })
                    .transpose()?,
                None => None,
            };
            let audit_json = crate::typeinfo::encode_stored_audit_record(&record)?;
            let result = crate::typeinfo::WasmTypeInfoResolveResult {
                type_expr: type_expr_json,
                audit_record: audit_json,
                error,
            };
            to_wasm_value(&result)
        }))?
    }

    /// Resolve a component's framework surfaces, returning the wire
    /// `TypeInfoGraphResponse` (as protobuf bytes) plus the per-request
    /// audit record. Mirrors the NAPI `resolveFrameworkSurfaceWithAudit`.
    ///
    /// `request` is the protobuf-encoded
    /// `verter_protocol::typeinfo::graph::TypeInfoGraphRequest` envelope
    /// carrying the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation. The
    /// host runs the envelope validator FIRST, so a malformed envelope
    /// returns the typed wire `error` arm BEFORE any registry lookup or
    /// semantic dispatch.
    ///
    /// Returns `{ response, auditRecord }` — `response` is the
    /// protobuf-encoded `TypeInfoGraphResponse` byte array (always
    /// present); `auditRecord` is the JSON `RequestAuditRecord` or `null`.
    #[wasm_bindgen(js_name = "resolveFrameworkSurfaceWithAudit")]
    pub fn resolve_framework_surface_with_audit(&self, request: &[u8]) -> Result<JsValue, JsValue> {
        let envelope = crate::typeinfo::decode_type_info_graph_request(request)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(AssertUnwindSafe(move || {
            let (outcome, record) = host
                .resolve_framework_surface_with_audit(envelope)
                .into_parts();
            let response = match outcome {
                Ok(response) => response,
                Err(error) => crate::typeinfo::framework_error_response(error),
            };
            let response_bytes = crate::typeinfo::encode_type_info_graph_response(&response);
            let audit_json = crate::typeinfo::encode_stored_audit_record(&record)?;
            let result = crate::typeinfo::WasmFrameworkSurfaceResult {
                response: response_bytes,
                audit_record: audit_json,
            };
            to_wasm_value(&result)
        }))?
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Build a `ScriptAnalysisSnapshot` from a `FileAnalysisSnapshot`.
///
/// Extracts all script-related fields, preserving `vue_api_calls` and
/// `dom_query_calls` from the snapshot (fixes zeroed-fields bug).
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
///
/// Generates a hierarchical tree of SFC blocks → children.
fn build_document_symbols_from_analysis(
    snapshot: &host::FileAnalysisSnapshot,
    source: &str,
) -> Vec<FfiDocumentSymbol> {
    let mut symbols = Vec::new();

    // Script block with bindings/imports/macros
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

    // Template block with components
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

    // Style blocks with classes
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

/// Safe UTF-16 conversion that handles 0 as identity.
fn byte_offset_to_utf16_safe(source: &str, byte_offset: u32) -> u32 {
    verter_ffi::convert::byte_offset_to_utf16(source, byte_offset)
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
            // Use pre-parsed structure if available, otherwise parse
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

#[cfg(test)]
mod tests {
    use super::lint_diagnostics_to_utf16;

    #[test]
    fn lint_utf16_conversion_uses_shared_ffi_helper() {
        let source = "a😀b";
        let diagnostics = vec![verter_diagnostics::LintDiagnostic {
            rule: "r".to_string(),
            category: "c".to_string(),
            severity: verter_diagnostics::Severity::Error,
            message: "m".to_string(),
            span: verter_span::Span::new(1, 5),
            tags: vec![],
            span_kind: verter_diagnostics::DiagnosticSpanKind::ElementOpenTag,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }];

        let out = lint_diagnostics_to_utf16(diagnostics, Some(source));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span.start, 1);
        assert_eq!(out[0].span.end, 3);
        assert!(out[0].tags.is_empty());
    }
}

// =============================================================================
// ComponentMetaHost — direct host for component-meta (WASM)
// =============================================================================

/// A component-meta host wrapping one native host (WASM variant).
#[wasm_bindgen(js_name = MetaProject)]
pub struct WasmMetaProject {
    inner: std::sync::Arc<host::component_meta_host::ComponentMetaHost>,
}

#[wasm_bindgen(js_class = MetaProject)]
impl WasmMetaProject {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmMetaProject, JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            let ffi_config: FfiHostConfig = if config.is_null() || config.is_undefined() {
                FfiHostConfig::default()
            } else {
                parse_wasm_input(config)?
            };
            let host_config = ffi_config_to_host(ffi_config).map_err(ffi_err)?;
            Ok(WasmMetaProject {
                inner: std::sync::Arc::new(
                    host::component_meta_host::ComponentMetaHost::new_standalone(host_config),
                ),
            })
        }))?
    }

    #[wasm_bindgen(js_name = "upsertBase")]
    pub fn upsert_base(&self, canonical_id: &str, source: &str) -> Result<(), JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            self.inner
                .upsert_base(canonical_id, source)
                .map_err(ffi_err)
        }))?
    }

    #[wasm_bindgen(js_name = "openSession")]
    pub fn open_session(&self) -> Result<WasmMetaSession, JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            let session = self.inner.open_session().map_err(ffi_err)?;
            Ok(WasmMetaSession {
                inner: Some(session),
            })
        }))?
    }

    #[wasm_bindgen(js_name = "clearCaches")]
    pub fn clear_caches(&self) -> Result<(), JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            self.inner.clear_caches().map_err(ffi_err)
        }))?
    }

    pub fn shutdown(&self) -> Result<(), JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            self.inner.shutdown();
            Ok(())
        }))?
    }

    #[wasm_bindgen(js_name = "isShutdown", getter)]
    pub fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    #[wasm_bindgen(js_name = "sessionCount", getter)]
    pub fn session_count(&self) -> u32 {
        self.inner.session_count() as u32
    }
}

/// Direct session handle — no overlay isolation (WASM variant).
#[wasm_bindgen(js_name = MetaSession)]
pub struct WasmMetaSession {
    inner: Option<host::component_meta_host::ComponentMetaSession>,
}

impl WasmMetaSession {
    fn session(&self) -> Result<&host::component_meta_host::ComponentMetaSession, JsValue> {
        self.inner
            .as_ref()
            .ok_or_else(|| JsValue::from_str("session is closed"))
    }
}

#[wasm_bindgen(js_class = MetaSession)]
impl WasmMetaSession {
    pub fn upsert(&self, canonical_id: &str, source: &str) -> Result<(), JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            session
                .upsert(canonical_id, source.to_string())
                .map_err(ffi_err)
        }))?
    }

    pub fn delete(&self, canonical_id: &str) -> Result<(), JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            session.delete(canonical_id).map_err(ffi_err)
        }))?
    }

    #[wasm_bindgen(js_name = "getAnalysis")]
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            let result = session.get_analysis(canonical_or_alias).map_err(ffi_err)?;
            match result {
                Some(snapshot) => to_wasm_value(&snapshot),
                None => Ok(JsValue::NULL),
            }
        }))?
    }

    /// Synchronous audit bundle — returns
    /// `{ analysis: FfiComponentMeta, resolution: FfiComponentMetaResolution,
    ///   record: RequestAuditRecord } | null` as a JS object. Host must
    /// have `audit_enabled` + `footprint_capture` set; otherwise throws.
    ///
    /// NOT a Promise. Consumer-side Promise ergonomics (if desired)
    /// live in `packages/wasm/audit.ts`.
    #[wasm_bindgen(js_name = "getComponentMetaWithAudit")]
    pub fn get_component_meta_with_audit(
        &self,
        canonical_or_alias: &str,
    ) -> Result<JsValue, JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            let (outcome, record) = session
                .get_component_meta_with_audit(canonical_or_alias)
                .into_parts();
            let Some(output) = outcome.map_err(ffi_err)? else {
                return Ok(JsValue::NULL);
            };
            let ffi = verter_ffi::convert::component_meta_output_to_ffi(output);
            let Some(ffi_resolution) = ffi.resolution.clone() else {
                return Err(ffi_err(
                    "audited component-meta output carries no resolution sidecar",
                ));
            };
            let bundle = WasmAuditBundle {
                analysis: ffi,
                resolution: ffi_resolution,
                record,
            };
            to_wasm_value(&bundle)
        }))?
    }

    /// Component metadata as a JS object (FFI projection), or `null`
    /// when the canonical does not resolve.
    ///
    /// This lane runs WITHOUT a type-resolution seed: the payload's typed
    /// `resolutionStatus` field reports
    /// `{ kind: "unavailable", reason: "resolutionProviderAbsent" }` so the
    /// un-overlaid registry is never mistaken for an exact resolved
    /// surface. The type-resolution-seeded payload is
    /// `getComponentMetaWithAudit` (`resolutionStatus.kind == "resolved"`).
    #[wasm_bindgen(js_name = "getComponentMeta")]
    pub fn get_component_meta(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            let Some(output) = session
                .get_component_meta_output(canonical_or_alias)
                .map_err(ffi_err)?
            else {
                return Ok(JsValue::NULL);
            };
            let ffi = verter_ffi::convert::component_meta_output_to_ffi(output);
            to_wasm_value(&ffi)
        }))?
    }

    /// Batch surface for `getComponentMeta`: compute metadata for
    /// `canonicalsOrAliases` under one shared overlay view. The host batch
    /// coordinator runs the N queries inline/sequentially on wasm (no
    /// coordinator pool) and accounts the submission once per non-empty
    /// batch (skipped for an empty batch). Returns one
    /// slot per input in input order as a JS array; each slot is the FFI
    /// projection of the
    /// analysis, or `null` EXCLUSIVELY for a genuinely missing canonical.
    ///
    /// Throws on project-level shutdown AND on a real per-id failure (a
    /// budget overrun or a fail-closed output-materialization failure) —
    /// batch failure semantics match the scalar `getComponentMeta` throw
    /// (scalar ≡ batch); a real failure is never collapsed onto the
    /// missing `null` sentinel.
    #[wasm_bindgen(js_name = "getComponentMetaBatch")]
    pub fn get_component_meta_batch(
        &self,
        canonicals_or_aliases: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            let results = session
                .get_component_meta_output_batch(&canonicals_or_aliases)
                .map_err(ffi_err)?;
            let ffi_results: Vec<Option<FfiComponentMeta>> = results
                .into_iter()
                .map(|slot| slot.map(verter_ffi::convert::component_meta_output_to_ffi))
                .collect();
            to_wasm_value(&ffi_results)
        }))?
    }

    /// Run the Rust walker against a committed audit record (JSON
    /// string from a prior `getComponentMetaWithAudit` round-trip
    /// through `JSON.stringify`) rooted at `canonical_id`. Returns
    /// the `ProvenanceChain` encoded as JSON string. Single walker
    /// implementation; TS helpers format the JSON via pure rendering.
    #[wasm_bindgen(js_name = "whyLoadedFromAuditJson")]
    pub fn why_loaded_from_audit_json(
        &self,
        audit_json: &str,
        canonical_id: &str,
    ) -> Result<String, JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            let bundle: WasmAuditBundleForWalker =
                serde_json::from_str(audit_json).map_err(|e| {
                    JsValue::from_str(&format!("audit_json is not a valid AuditBundle: {e}"))
                })?;
            let chain = bundle.record.why_loaded(canonical_id);
            serde_json::to_string(&chain)
                .map_err(|e| JsValue::from_str(&format!("chain serialization error: {e}")))
        }))?
    }

    /// Run the Rust walker rooted at the instantiation keyed by
    /// `(decl_canonical_id, decl_symbol_name, args_fingerprint_hex)`.
    /// `args_fingerprint_hex` is the 32-char lowercase hex rendering
    /// of the 16-byte `Hash16`.
    #[wasm_bindgen(js_name = "whyInstantiatedFromAuditJson")]
    pub fn why_instantiated_from_audit_json(
        &self,
        audit_json: &str,
        decl_canonical_id: &str,
        decl_symbol_name: &str,
        args_fingerprint_hex: &str,
    ) -> Result<String, JsValue> {
        catch_panic(AssertUnwindSafe(|| {
            let bundle: WasmAuditBundleForWalker =
                serde_json::from_str(audit_json).map_err(|e| {
                    JsValue::from_str(&format!("audit_json is not a valid AuditBundle: {e}"))
                })?;
            let fingerprint = parse_hash16_hex_wasm(args_fingerprint_hex)?;
            let chain =
                bundle
                    .record
                    .why_instantiated(decl_canonical_id, decl_symbol_name, fingerprint);
            serde_json::to_string(&chain)
                .map_err(|e| JsValue::from_str(&format!("chain serialization error: {e}")))
        }))?
    }

    #[wasm_bindgen(js_name = "getEffectiveSource")]
    pub fn get_effective_source(&self, canonical_id: &str) -> Result<JsValue, JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            let result = session
                .get_effective_source(canonical_id)
                .map_err(ffi_err)?;
            match result {
                Some(src) => Ok(JsValue::from_str(&src)),
                None => Ok(JsValue::NULL),
            }
        }))?
    }

    #[wasm_bindgen(js_name = "hasFile")]
    pub fn has_file(&self, canonical_id: &str) -> Result<bool, JsValue> {
        let session = self.session()?;
        catch_panic(AssertUnwindSafe(|| {
            session.has_file(canonical_id).map_err(ffi_err)
        }))?
    }

    pub fn close(&mut self) {
        if let Some(session) = self.inner.take() {
            session.close();
        }
    }

    #[wasm_bindgen(js_name = "isClosed", getter)]
    pub fn is_closed(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(|session| session.is_closed())
    }
}
