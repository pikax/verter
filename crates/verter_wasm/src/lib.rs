//! # verter_wasm — WebAssembly bindings for Verter
//!
//! `wasm-bindgen` binding layer that exposes [`verter_host::VerterHost`] to
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

use serde::Serialize;
use verter_ffi::convert::*;
use verter_ffi::types::*;
use verter_host as host;
use wasm_bindgen::prelude::*;

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
    serde_wasm_bindgen::to_value(value)
        .map_err(|e| JsValue::from_str(&format!("Host serialization error: {}", e)))
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

// =============================================================================
// VerterHost (in-memory virtual file host)
//
// API parity with NAPI (crates/verter_napi):
// - Both: new, resolve, upsert, applyStyleOverrides, getVirtualFile,
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
    inner: host::VerterHost,
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
            inner: host::VerterHost::new(ffi_config_to_host(ffi_config).map_err(ffi_err)?),
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

    /// Replaces one or more style blocks with preprocessed CSS and recompiles
    /// affected virtual nodes.
    ///
    /// Used after running a CSS preprocessor on style blocks that have a
    /// `lang` attribute. The host then applies scoping, CSS Modules, and
    /// `v-bind()` replacement on the preprocessed CSS.
    ///
    /// Returns the same changeset structure as [`upsert`](Self::upsert).
    ///
    /// Throws if the canonical ID is unknown or the request is malformed.
    #[wasm_bindgen(js_name = applyStyleOverrides)]
    pub fn apply_style_overrides(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let ffi_req = parse_wasm_input::<FfiStyleOverrideRequest>(request)?;
        let host_req = ffi_style_override_to_host(ffi_req).map_err(ffi_err)?;
        let result =
            catch_panic(|| self.inner.apply_style_overrides(host_req))?.map_err(host_err)?;
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

    /// Retrieves the combined TSX output for LSP type checking.
    ///
    /// This is a dedicated API separate from virtual files. TSX output is
    /// only consumed by the LSP and playground, never by bundlers.
    ///
    /// Returns `{ code: string, sourceMap?: string }` or `null` if no TSX
    /// output is available for the given file and profile.
    #[wasm_bindgen(js_name = getTsx)]
    pub fn get_tsx(&self, canonical_id: &str, profile: JsValue) -> Result<JsValue, JsValue> {
        let ffi_profile: Option<FfiCompileProfile> = if profile.is_undefined() || profile.is_null()
        {
            None
        } else {
            Some(parse_wasm_input(profile)?)
        };
        let host_profile = ffi_profile_to_host(ffi_profile).map_err(ffi_err)?;
        let result = catch_panic(|| self.inner.get_tsx(canonical_id, &host_profile))?;
        to_wasm_value(&result.map(|r| FfiTsxResponse {
            code: r.code.to_string(),
            source_map: r.source_map.map(|s| s.to_string()),
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
    /// Called after resolving the `importSpecifiers` returned by
    /// [`upsert`](Self::upsert). This enables cross-file type resolution
    /// (e.g. following `import type { Props } from './types'` chains) when
    /// recompiling dependent files.
    ///
    /// - `canonical_or_alias` — the file whose dependencies are being set.
    /// - `resolved_deps` — a JS array of canonical ID strings for the
    ///   resolved dependency files.
    #[wasm_bindgen(js_name = setImportDependencies)]
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolved_deps: JsValue,
    ) -> Result<(), JsValue> {
        let deps: Vec<String> = parse_wasm_input(resolved_deps)?;
        catch_panic(|| self.inner.set_import_dependencies(canonical_or_alias, deps))
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
            verter_linter::LintConfig::default()
        } else {
            parse_wasm_input::<verter_linter::LintConfig>(config)?
        };

        let analysis = catch_panic(|| self.inner.get_analysis(canonical_or_alias))?;

        let diagnostics = match analysis {
            Some(snapshot) => {
                let linter = verter_linter::Linter::new(lint_config);
                let script = verter_analysis::types::ScriptAnalysisSnapshot {
                    imports: snapshot.imports,
                    bindings: snapshot.bindings,
                    macros: snapshot.macros,
                    macro_type_deps: snapshot.macro_type_deps,
                    flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
                        snapshot.script_flags,
                    ),
                    exported_functions: Vec::new(),
                    type_enhancements: None,
                };
                linter.lint(Some(&script), snapshot.template.as_ref(), &snapshot.styles)
            }
            None => Vec::new(),
        };
        let source = self.inner.get_source(canonical_or_alias);
        let diagnostics = lint_diagnostics_to_utf16(diagnostics, source.as_deref());
        to_wasm_value(&diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::lint_diagnostics_to_utf16;

    #[test]
    fn lint_utf16_conversion_uses_shared_ffi_helper() {
        let source = "a😀b";
        let diagnostics = vec![verter_linter::LintDiagnostic {
            rule: "r".to_string(),
            category: "c".to_string(),
            severity: verter_linter::Severity::Error,
            message: "m".to_string(),
            span_start: 1,
            span_end: 5,
            fix: None,
        }];

        let out = lint_diagnostics_to_utf16(diagnostics, Some(source));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span_start, 1);
        assert_eq!(out[0].span_end, 3);
        assert!(out[0].fix.is_none());
    }
}
