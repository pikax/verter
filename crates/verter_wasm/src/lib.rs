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
    serde_wasm_bindgen::to_value(value)
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
) -> Result<verter_analysis::ModuleReferenceSyntax, JsValue> {
    match syntax {
        "staticImport" => Ok(verter_analysis::ModuleReferenceSyntax::StaticImport),
        "exportFrom" => Ok(verter_analysis::ModuleReferenceSyntax::ExportFrom),
        "dynamicImport" => Ok(verter_analysis::ModuleReferenceSyntax::DynamicImport),
        "requireCall" => Ok(verter_analysis::ModuleReferenceSyntax::RequireCall),
        other => Err(ffi_err(format!("unknown module reference syntax: {other}"))),
    }
}

fn ffi_module_reference_semantics_from_str(
    semantics: &str,
) -> Result<verter_analysis::ModuleReferenceSemantics, JsValue> {
    match semantics {
        "import" => Ok(verter_analysis::ModuleReferenceSemantics::Import),
        "require" => Ok(verter_analysis::ModuleReferenceSemantics::Require),
        other => Err(ffi_err(format!(
            "unknown module reference semantics: {other}"
        ))),
    }
}

fn ffi_module_reference_analyzability_from_str(
    analyzability: &str,
) -> Result<verter_analysis::ModuleReferenceAnalyzability, JsValue> {
    match analyzability {
        "exact" => Ok(verter_analysis::ModuleReferenceAnalyzability::Exact),
        "finiteSet" => Ok(verter_analysis::ModuleReferenceAnalyzability::FiniteSet),
        "unknownDynamic" => Ok(verter_analysis::ModuleReferenceAnalyzability::UnknownDynamic),
        other => Err(ffi_err(format!(
            "unknown module reference analyzability: {other}"
        ))),
    }
}

fn ffi_module_reference_to_analysis(
    input: FfiModuleReference,
) -> Result<verter_analysis::AnalyzedModuleReference, JsValue> {
    Ok(verter_analysis::AnalyzedModuleReference {
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
            verter_analysis::project_resolver::collect_resolvable_module_reference_specifiers(
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
            verter_analysis::project_resolver::resolve_known_module_reference_dependencies(
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
}

// =============================================================================
// Helper functions
// =============================================================================

/// Build a `ScriptAnalysisSnapshot` from a `FileAnalysisSnapshot`.
///
/// Extracts all script-related fields, preserving `vue_api_calls` and
/// `dom_query_calls` from the snapshot (fixes Phase 2 zeroed-fields bug).
fn build_script_snapshot(
    snapshot: &host::FileAnalysisSnapshot,
) -> verter_analysis::types::ScriptAnalysisSnapshot {
    verter_analysis::types::ScriptAnalysisSnapshot {
        imports: snapshot.imports.clone(),
        module_references: snapshot.module_references.to_vec(),
        bindings: snapshot.bindings.clone(),
        macros: snapshot.macros.to_vec(),
        macro_type_deps: snapshot.macro_type_deps.to_vec(),
        flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(snapshot.script_flags),
        exported_functions: Vec::new(),
        vue_api_calls: snapshot.vue_api_calls.to_vec(),
        dom_query_calls: snapshot.dom_query_calls.to_vec(),
        css_var_manipulations: snapshot.css_var_manipulations.to_vec(),
        script_binding_occurrences: snapshot.script_binding_occurrences.to_vec(),
        first_await_offset: None,
        type_enhancements: None,
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
