//! FFI-boundary types shared between NAPI and WASM bindings.
//!
//! All structs use `#[serde(rename_all = "camelCase")]` so field names
//! match JavaScript convention when serialized. WASM uses these types
//! directly via `serde_wasm_bindgen`; NAPI maps to/from its own
//! `#[napi(object)]` structs via zero-copy `From` impls.
//!
//! E1 protocol changes:
//! - `FfiComponentMeta` carries `origin: OriginGraphDto` alongside
//!   the primary payload. Compact wire form: dense edge table +
//!   interned edge-meta strings + sequential node ids.
//! - `ProjectionMode::{Identity, Shallow, Expanded}` crosses the FFI
//!   (`Navigate` is dispatch-internal).

use serde::{Deserialize, Serialize};

// =============================================================================
// Shared (both input and output)
// =============================================================================

/// Discriminator for virtual file nodes (both input and output).
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiVirtualNodeKind {
    pub kind: String,
    pub index: Option<u32>,
}

// =============================================================================
// Input types (JS → Rust)
// =============================================================================

/// Host configuration options.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FfiHostConfig {
    pub dev_mode: Option<bool>,
    pub compile_error_policy: Option<String>,
    pub lsp_scheme: Option<String>,
    pub max_profiles_per_file: Option<u32>,
    pub resolve_extensions: Option<Vec<String>>,
    pub analysis_level: Option<String>,
    /// Enable Rust-first native audit for component-meta requests.
    pub audit_enabled: Option<bool>,
    /// Enable per-request semantic footprint capture. Requires
    /// `audit_enabled = true`.
    pub footprint_capture: Option<bool>,
    /// Capacity of the host-owned typeinfo scratch cache. `None`
    /// (default) selects 64 entries; `Some(0)` disables the cache;
    /// other values cap the LRU at the chosen size — used by the
    /// `@verter/typeinfo` LRU eviction tests.
    pub typeinfo_scratch_cache_capacity: Option<u32>,
    /// Worker count for the host-owned CPU pool used by every host
    /// batch API's outer coordinator — `compile_many` and the
    /// component-meta batch
    /// (`verter_scheduler::HostCpuPool`). `None` (the default)
    /// resolves to `std::thread::available_parallelism` at
    /// host-construction time; `Some(0)` is treated as `None`
    /// (rather than rejected, so a misconfigured caller passing `0`
    /// gets the default instead of a panic); other positive values
    /// cap the pool's worker count. The host pool is built once at
    /// host construction and reused across every batch call — to
    /// change the pool size, construct a new host.
    pub host_cpu_threads: Option<u32>,
    /// Enable host performance-metrics collection (upsert/compile/resolve
    /// counters and timers). `None`/absent keeps the default `false` —
    /// counters stay zero and `getMetrics()`/`getStatistics()` merge no
    /// data. Replaces the retired `session_metrics` Cargo feature as the
    /// runtime opt-in for NAPI-constructed hosts.
    pub metrics_enabled: Option<bool>,
}

/// Per-compilation variant options.
///
/// `deny_unknown_fields`: an unrecognized JSON key must refuse at
/// deserialization, not be silently dropped before `ffi_profile_to_host`
/// ever sees it — the decode-boundary half of the same "no silently
/// ignored option" contract `CompileRequest` construction enforces
/// downstream.
#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FfiCompileProfile {
    pub filename: Option<String>,
    pub is_production: Option<bool>,
    pub custom_element: Option<bool>,
    pub ssr: Option<bool>,
    /// SSR asset-collection module id registered on `ssrContext.modules`.
    /// Absent falls back to the canonical id.
    pub ssr_module_id: Option<String>,
    pub hmr_strategy: Option<String>,
    pub component_id: Option<String>,
    pub delimiters: Option<Vec<String>>,
    pub custom_elements: Option<Vec<String>>,
    pub comments: Option<bool>,
    pub runtime_module_name: Option<String>,
    pub types_module_name: Option<String>,
    pub force_vapor: Option<bool>,
    pub force_js: Option<bool>,
    pub source_map: Option<bool>,
    /// Compilation target preset: "bundler" (default), "ide", or "analysis".
    pub target: Option<String>,
    /// Inline the render function inside `setup()` (Vue production topology).
    /// Absent resolves to `isProduction` (official default: inline in prod
    /// builds). VDOM client only; Vapor inline and inline SSR fall back to
    /// non-inline.
    pub inline: Option<bool>,
    /// Experimental: strict slot children type checking.
    pub strict_slots: Option<bool>,
    /// Requested compile cache mode: "stateless", "content", or
    /// "session" (default). `FfiVirtualQuery` carries the mode through
    /// this embedded profile.
    pub requested_mode: Option<String>,
}

/// Request to upsert a file into the host.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiUpsertRequest {
    pub canonical_id: Option<String>,
    pub input_id: String,
    pub source: String,
    pub file_kind: Option<String>,
    pub aliases: Option<Vec<String>>,
}

/// A diagnostic reported by an external preprocessor tool for one
/// [`FfiBlockOverrideEntry`].
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FfiPreprocessorDiagnostic {
    /// `"error"`, `"warning"`, or `"info"`.
    pub severity: String,
    pub message: String,
    /// Line/column within the preprocessor's own source, if reported.
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// A single preprocessed block override (template, script, style, or custom).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FfiBlockOverrideEntry {
    pub correlation_token: String,
    pub block_token: String,
    pub owner_revision: String,
    pub artifact_token: String,
    pub expected_language: String,
    pub prior_basis_token: Option<String>,
    pub basis_token: String,
    pub source_space_token: String,
    /// Preprocessed code.
    pub code: String,
    pub code_hash: String,
    /// Source map from the preprocessor, if available.
    pub source_map: Option<String>,
    pub source_map_hash: Option<String>,
    /// Files the preprocessor's result depended on.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Diagnostics the preprocessor reported for this result.
    #[serde(default)]
    pub diagnostics: Vec<FfiPreprocessorDiagnostic>,
    /// Identifying name of the external tool that produced `code`.
    #[serde(default)]
    pub processor_identity: Option<String>,
    /// Version string of the external tool that produced `code`.
    #[serde(default)]
    pub processor_version: Option<String>,
    /// Opaque fingerprint of the external tool's configuration.
    pub config_fingerprint: Option<String>,
    /// Superseded wire field, replaced by `dependencies`/`diagnostics`/
    /// `processorIdentity`/`processorVersion`/`configFingerprint` above.
    /// Accepted-and-ignored so `deny_unknown_fields` does not hard-reject a
    /// payload from a caller still sending the old shape — never read by
    /// `ffi_block_override_to_host`. Remove once no caller sends it.
    #[serde(default)]
    pub supplied_provenance: Option<String>,
}

/// Request to apply preprocessed block overrides (unified API).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiBlockOverrideRequest {
    pub canonical_id: String,
    pub compile_profile: Option<FfiCompileProfile>,
    pub overrides: Vec<FfiBlockOverrideEntry>,
}

/// Query for a specific virtual file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiVirtualQuery {
    pub raw_id: Option<String>,
    pub canonical_id: Option<String>,
    pub node_kind: Option<FfiVirtualNodeKind>,
    pub compile_profile: Option<FfiCompileProfile>,
}

// =============================================================================
// Framework-discriminated host compile request (JS → Rust)
// =============================================================================
//
// Every struct below is `deny_unknown_fields`, and the framework arm is an
// externally-tagged enum: a Svelte option key inside the Vue arm is an
// unknown field, and an unrecognised framework key is an unknown variant.
// Both refuse at decode time — there is no arm on which a foreign or
// misspelled key can be silently dropped.
//
// Every non-`Option` field is required: an absent key is a decode refusal,
// not a substituted value. `Option` fields carry presence semantics — for
// an option the compiler refuses, `Some(false)` still means "the caller
// supplied it" and is refused on presence.
//
// Presence is measured on the DECODED value, so an omitted key and an
// explicit JSON `null` are indistinguishable: both decode to `None` and
// neither is treated as supplied. `{"codegenMode": false}` is refused as
// a supplied unsupported option; `{"codegenMode": null}` is accepted as
// an absent one. A caller that must distinguish "explicitly cleared" from
// "not stated" cannot express that here.

/// Which Vue client codegen backend a runtime product resolves to.
/// `Inferred` defers to the parsed source's own marker.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostVueBackend")]
pub enum FfiVueBackend {
    Inferred,
    Vdom,
    Vapor,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostVueWhitespace")]
pub enum FfiVueWhitespace {
    Preserve,
    Condense,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostVueParsePad")]
pub enum FfiVueParsePad {
    Space,
    Line,
    Off,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostVueAssetUrlOptions", optional_fields = nullable)]
pub struct FfiVueAssetUrlOptions {
    pub base: Option<String>,
    pub include_absolute: Option<bool>,
    pub tags: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostVueAssetUrlTransform")]
pub enum FfiVueAssetUrlTransform {
    Disabled,
    Enabled(FfiVueAssetUrlOptions),
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostVueCssModuleScopeBehaviour")]
pub enum FfiVueCssModuleScopeBehaviour {
    Local,
    Global,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostVueCssModuleLocalsConvention")]
pub enum FfiVueCssModuleLocalsConvention {
    CamelCase,
    CamelCaseOnly,
    Dashes,
    DashesOnly,
    AsIs,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostVueCssModules", optional_fields = nullable)]
pub struct FfiVueCssModules {
    pub scope_behaviour: Option<FfiVueCssModuleScopeBehaviour>,
    pub hash_prefix: Option<String>,
    pub locals_convention: Option<FfiVueCssModuleLocalsConvention>,
    pub export_globals: Option<bool>,
}

/// Vue-owned compile options. The trailing `compatConfig*` / `codegenMode`
/// slots exist only so a caller who supplies a refused option is told which
/// one — presence is what the conversion refuses, `false` included.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostVueCompileOptions", optional_fields = nullable)]
pub struct FfiVueCompileOptions {
    pub backend: FfiVueBackend,
    /// Mirrors the canonical `ssr` OPTION one-for-one. It is not the SSR
    /// demand: whether a compile produces server output is derived from
    /// the requested product set, and that derivation stays the canonical
    /// request's. The two are independent — this flag disagreeing with the
    /// requested products is not refused when the request is constructed.
    pub ssr: bool,
    pub is_custom_element: Vec<String>,
    /// Exactly two elements; any other length is a typed malformed-value
    /// refusal, never a fallback to the framework's own delimiters.
    pub delimiters: Option<Vec<String>>,
    pub whitespace: Option<FfiVueWhitespace>,
    pub comments: Option<bool>,
    pub hoist_static: Option<bool>,
    pub cache_handlers: Option<bool>,
    pub hmr: Option<bool>,
    pub optimize_imports: Option<bool>,
    pub runtime_module_name: Option<String>,
    pub ssr_runtime_module_name: Option<String>,
    pub parse_pad: Option<FfiVueParsePad>,
    pub ignore_empty: Option<bool>,
    pub babel_parser_plugins: Vec<String>,
    pub gen_default_as: Option<String>,
    pub props_destructure: Option<bool>,
    pub script_custom_element: Option<bool>,
    pub transform_asset_urls: Option<FfiVueAssetUrlTransform>,
    pub style_trim: Option<bool>,
    pub css_modules: Option<FfiVueCssModules>,

    pub compat_config: Option<bool>,
    pub compat_config_mode: Option<bool>,
    pub compat_config_compiler_is_on_element: Option<bool>,
    pub compat_config_compiler_v_bind_sync: Option<bool>,
    pub compat_config_compiler_v_if_v_for_precedence: Option<bool>,
    pub compat_config_compiler_v_bind_object_order: Option<bool>,
    pub compat_config_compiler_v_on_native: Option<bool>,
    pub compat_config_compiler_native_template: Option<bool>,
    pub compat_config_compiler_inline_template: Option<bool>,
    pub compat_config_compiler_filters: Option<bool>,
    pub transform_compat_config: Option<bool>,
    pub codegen_mode: Option<bool>,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostSvelteNamespace")]
pub enum FfiSvelteNamespace {
    Html,
    Svg,
    MathMl,
    Foreign,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostSvelteFragments")]
pub enum FfiSvelteFragments {
    Html,
    Tree,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostSvelteRunes")]
pub enum FfiSvelteRunes {
    True,
    False,
    Infer,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "HostSvelteCss")]
pub enum FfiSvelteCss {
    Injected,
    External,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostSvelteCustomElementProp", optional_fields = nullable)]
pub struct FfiSvelteCustomElementProp {
    pub attribute: Option<String>,
    pub reflect: Option<bool>,
    /// The caller's prop-type spelling, carried verbatim. The wire owns no
    /// membership over the custom-element prop-type vocabulary: an
    /// unrecognised spelling decodes fine here and is refused at canonical
    /// request construction, the one place that decides the vocabulary, so
    /// this boundary cannot drift from it or refuse at a different stage
    /// than the direct canonical entry point does.
    pub prop_type: Option<String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostSvelteCustomElementDescriptor", optional_fields = nullable)]
pub struct FfiSvelteCustomElementDescriptor {
    pub tag: Option<String>,
    pub shadow: Option<bool>,
    pub props: std::collections::BTreeMap<String, FfiSvelteCustomElementProp>,
}

/// Presence-only marker for the `compatibility` object: the one inventoried
/// field it may carry (`componentApi`) is refused, so it has no wire slot.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
// `ts-rs` projects a field-less struct as `Record<symbol, never>`, which is
// WEAKER than this declaration: it closes a fresh object literal but admits
// any already-typed object, so `{ componentApi: true }` type-checks and then
// decodes to a refusal. `Record<string, never>` is what the decoder means.
//
// The override is not the hand-written-TypeScript hole this projection
// closes. That hole is a declaration that DIVERGES from the schema; this one
// exists because the automatic projection is less closed than the schema is,
// and it restores what `deny_unknown_fields` on a field-less struct already
// says. An override that asserted a shape the decoder does not accept would
// be the lie, and is not licensed by this one. (`rename_all` is dropped: a
// no-op on a field-less struct, and `ts-rs` refuses it beside an explicit
// `type`.)
//
// This declaration is the ONE line here the byte pin does not bind to the
// Rust shape: `ts-rs` short-circuits projection on a container `type`
// override and never reads the fields, so ADDING A FIELD BELOW CHANGES WHAT
// THE DECODER ACCEPTS AND REDDENS NOTHING. Field-lessness is what makes the
// override true, and it is held only by this struct staying empty. Give this
// type a field and the override must go with it.
#[ts(rename = "HostSvelteCompatibility", type = "Record<string, never>")]
pub struct FfiSvelteCompatibility {}

// The witness for the paragraph above: an exhaustive empty destructuring is
// E0027 the moment this struct gains a field, so the override cannot outlive
// the field-lessness that makes it true. This is the binding the byte pin
// cannot provide here.
const _: fn() = || {
    let FfiSvelteCompatibility {} = FfiSvelteCompatibility {};
};

/// Svelte-owned compile options. `generateModule` / `experimentalAsync`
/// are well-formed options whose module-compilation capability is refused;
/// the trailing slots are the unconditionally refused rows.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostSvelteCompileOptions", optional_fields = nullable)]
pub struct FfiSvelteCompileOptions {
    pub dev: Option<bool>,
    pub generate_module: Option<bool>,
    pub experimental_async: Option<bool>,
    pub custom_element: Option<bool>,
    pub custom_element_descriptor: Option<FfiSvelteCustomElementDescriptor>,
    pub namespace: Option<FfiSvelteNamespace>,
    pub css: Option<FfiSvelteCss>,
    pub preserve_comments: Option<bool>,
    pub preserve_whitespace: Option<bool>,
    pub fragments: Option<FfiSvelteFragments>,
    pub runes: Option<FfiSvelteRunes>,
    pub disclose_version: Option<bool>,
    pub compatibility: Option<FfiSvelteCompatibility>,

    pub loose: Option<bool>,
    pub accessors: Option<bool>,
    pub immutable: Option<bool>,
    pub compatibility_component_api: Option<bool>,
    pub hmr: Option<bool>,
    pub custom_element_extend: Option<bool>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostRuntimeProductOptions", optional_fields = nullable)]
pub struct FfiRuntimeProductRequest {
    /// Absent resolves to the request's own `isProduction` — the framework's
    /// documented derivation, computed by the canonical request rather than
    /// substituted here.
    pub inline: Option<bool>,
    pub runtime_source_map: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostIdeProductOptions", optional_fields = nullable)]
pub struct FfiIdeProductRequest {
    pub want_source_map: bool,
    pub embed_ambient_types: bool,
    pub conditional_root_narrowing: bool,
    pub strict_slots: bool,
    pub types_module_name: Option<String>,
    pub ide_chunk_boundaries: bool,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostAnalysisProductOptions")]
pub struct FfiAnalysisProductRequest {
    pub want_script_bindings: bool,
    pub want_template_data: bool,
}

/// One requested compiler product, 1:1 with the canonical product
/// vocabulary. The product set is the demand document: there is no target
/// string and no preset that expands into a bundle of products.
///
/// `PublicApi` and `Declarations` are unit variants because the canonical
/// requests for those products carry only host-resolved profile
/// identities, which the wire never supplies.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiRequestedProduct {
    RuntimeClient(FfiRuntimeProductRequest),
    RuntimeServer(FfiRuntimeProductRequest),
    IdeCompanion(FfiIdeProductRequest),
    PublicApi,
    Declarations,
    Analysis(FfiAnalysisProductRequest),
}

/// Source identity and dev/prod profile shared by every product requested
/// in one compile.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename = "HostCompileIdentity", optional_fields = nullable)]
pub struct FfiHostCompileIdentity {
    pub filename: Option<String>,
    pub component_id: Option<String>,
    pub is_production: bool,
    pub force_js: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FfiVueHostCompileRequest {
    pub identity: FfiHostCompileIdentity,
    pub products: Vec<FfiRequestedProduct>,
    pub options: FfiVueCompileOptions,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FfiSvelteHostCompileRequest {
    pub identity: FfiHostCompileIdentity,
    pub products: Vec<FfiRequestedProduct>,
    pub options: FfiSvelteCompileOptions,
}

/// A host compile request discriminated by framework at the outermost
/// level, so framework-owned options are structurally unreachable from the
/// other framework's arm.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiHostCompileRequest {
    Vue(FfiVueHostCompileRequest),
    Svelte(FfiSvelteHostCompileRequest),
}

// =============================================================================
// Output types (Rust → JS)
// =============================================================================

/// Granular slice-level change breakdown.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiSliceChanges {
    pub script_changed: bool,
    pub template_changed: bool,
    pub style_indices_changed: Vec<u32>,
    pub custom_indices_changed: Vec<u32>,
    pub structure_changed: bool,
    pub descriptor_changed: bool,
}

/// A single diagnostic (error, warning, or info).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub span_start: u32,
    pub span_end: u32,
}

/// Collection of diagnostics with a precomputed `hasErrors` flag.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDiagnosticsSnapshot {
    pub diagnostics: Vec<FfiDiagnostic>,
    pub has_errors: bool,
}

/// An external `src="..."` request that needs caller-side resolution.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiExternalSourceRequest {
    pub owner_canonical_id: String,
    pub block_kind: String,
    pub specifier: String,
    pub resolved_canonical_id: String,
    pub block_token: String,
    pub owner_revision: String,
    pub artifact_token: String,
    pub carrier_source_space_token: String,
}

/// Summary of a single import statement found in a script block.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiScriptImportInfo {
    pub source: String,
    pub is_type_only: bool,
    pub bindings: Vec<String>,
}

/// Summary of a single module reference found in a script block.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiModuleReference {
    pub syntax: String,
    pub semantics: String,
    pub is_type_only: bool,
    pub raw_text: String,
    pub literal_specifier: Option<String>,
    pub finite_specifiers: Vec<String>,
    pub static_prefix: Option<String>,
    pub analyzability: String,
    pub span_start: u32,
    pub span_end: u32,
    pub expr_span_start: u32,
    pub expr_span_end: u32,
}

/// A block that needs external preprocessing before compilation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiPreprocessorRequest {
    pub content_class: String,
    /// The `lang` attribute value (e.g., "pug", "coffee", "scss").
    pub lang: String,
    /// Raw content of the block that needs preprocessing.
    pub content: String,
    pub availability: String,
    pub correlation_token: String,
    pub block_token: String,
    pub owner_revision: String,
    pub artifact_token: String,
    pub expected_language: String,
    /// Caller-held basis before the host captured this request. Absent for a
    /// tokenless first resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_basis_token: Option<String>,
    pub basis_token: String,
    pub source_space_token: String,
    pub content_hash: String,
    pub custom_type: Option<String>,
}

/// A single export signature extracted from a file's script block.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiExportSignature {
    pub name: String,
    pub is_type: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reexport_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reexport_local: Option<String>,
}

/// A fully resolved export after following re-export chains.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedExport {
    /// Exported name as seen by importers.
    pub name: String,
    /// Whether this is a type-only export.
    pub is_type: bool,
    /// Ultimate source file canonical ID (None = local to the queried file).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_canonical_id: Option<String>,
    /// Name in the ultimate source file (may differ, e.g. "default" → "Button").
    pub source_name: String,
}

/// Result of an upsert or style override operation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiUpdateResult {
    pub canonical_id: String,
    pub changed: bool,
    pub slice_changes: FfiSliceChanges,
    pub changed_virtual_nodes: Vec<FfiVirtualNodeKind>,
    pub removed_virtual_nodes: Vec<FfiVirtualNodeKind>,
    pub changed_virtual_ids: Vec<String>,
    pub removed_virtual_ids: Vec<String>,
    pub changed_lsp_ids: Vec<String>,
    pub removed_lsp_ids: Vec<String>,
    pub diagnostics: FfiDiagnosticsSnapshot,
    pub external_source_requests: Vec<FfiExternalSourceRequest>,
    pub import_specifiers: Vec<FfiScriptImportInfo>,
    pub module_references: Vec<FfiModuleReference>,
    pub preprocessor_requests: Vec<FfiPreprocessorRequest>,
    pub export_signatures: Vec<FfiExportSignature>,
    pub parse_duration_ms: f64,
}

/// Result of resolving a raw import ID.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedId {
    pub canonical_id: String,
    pub node_kind: FfiVirtualNodeKind,
    pub exists_in_host: bool,
    pub bundler_id: String,
    pub lsp_id: String,
}

/// Block-specific metadata attached to a virtual file.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiVirtualMeta {
    pub scope_id: Option<String>,
    pub block_type: Option<String>,
}

/// Response containing a compiled virtual file.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiVirtualFileResponse {
    pub id: String,
    pub code: String,
    pub source_map: Option<String>,
    pub lang: Option<String>,
    pub stale: bool,
    pub diagnostics: FfiDiagnosticsSnapshot,
    pub meta: FfiVirtualMeta,
    /// `true` iff this response was served from a warm cache slot (the
    /// fact-validated session slot OR the content-addressed store).
    pub cache_hit: bool,
    /// Requested compile cache mode ("stateless" / "content" / "session").
    pub requested_mode: String,
    /// Actual compile cache mode the runtime ran under.
    pub actual_mode: String,
    /// Highest-priority downgrade reason (e.g. "HasMacroTypeDeps"), or
    /// `None` when no reason fired.
    pub downgrade_reason: Option<String>,
}

/// A single destructured binding's source mapping (target encoding).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDestructuredBinding {
    /// Binding identifier name.
    pub name: String,
    /// SFC-absolute start offset of the original source declaration (target encoding).
    pub source_start: u32,
    /// SFC-absolute end offset of the original source declaration (target encoding).
    pub source_end: u32,
}

/// Metadata for the destructured block region in the generated TSX output
/// (target encoding, not source spans).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDestructuredBlockMeta {
    pub bindings: Vec<FfiDestructuredBinding>,
    /// Start offset of the destructured block in the generated TSX output (target encoding).
    pub block_start: u32,
    /// End offset of the destructured block in the generated TSX output (target encoding).
    pub block_end: u32,
}

/// IDE output for type checking (TSX or JSX, dedicated API, not a virtual file).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiIdeResponse {
    pub code: String,
    pub source_map: Option<String>,
    pub is_jsx: bool,
    pub destructured_block: Option<FfiDestructuredBlockMeta>,
}

/// TSC public-API projection payload.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiTscResponse {
    pub code: String,
    pub source_map: Option<String>,
}

/// Dependency-neutral syntax carrier for a failed public-API projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PublicApiProjectionSubject {
    Macro { syntax_index: u32 },
    ScriptSetupAttrs { source_range: verter_span::Span },
}

impl std::fmt::Display for PublicApiProjectionSubject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Macro { syntax_index } => {
                write!(formatter, "macro syntax index {syntax_index}")
            }
            Self::ScriptSetupAttrs { source_range } => write!(
                formatter,
                "script setup attrs source range {}..{}",
                source_range.start, source_range.end
            ),
        }
    }
}

#[cfg(test)]
mod public_api_projection_subject_tests {
    use super::PublicApiProjectionSubject;

    #[test]
    fn serializes_as_closed_discriminated_union() {
        assert_eq!(
            serde_json::to_value(PublicApiProjectionSubject::Macro { syntax_index: 7 }).unwrap(),
            serde_json::json!({ "kind": "macro", "syntaxIndex": 7 })
        );
        assert_eq!(
            serde_json::to_value(PublicApiProjectionSubject::ScriptSetupAttrs {
                source_range: verter_span::Span::new(31, 37),
            })
            .unwrap(),
            serde_json::json!({
                "kind": "scriptSetupAttrs",
                "sourceRange": { "start": 31, "end": 37 },
            })
        );
    }
}

/// Stable structured identity for a failed public-API projection.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicApiProjectionError {
    pub code: String,
    pub detail_code: String,
    pub subject: PublicApiProjectionSubject,
    pub declaration_shape_reason: Option<String>,
    pub member_ordinal: Option<u32>,
    pub outcome_kind: Option<String>,
    pub outcome_reason: Option<String>,
    pub outcome_diagnostic: Option<String>,
}

/// Explicit tri-state public-API result: value, ordinary absence, or failure.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicApiResult {
    pub value: Option<FfiTscResponse>,
    pub error: Option<FfiPublicApiProjectionError>,
}

/// Result of removing a file from the host.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiRemoveResult {
    pub canonical_id: String,
}

/// Result of cross-file optimization analysis.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiCrossFileResult {
    /// Per-file const prop sets (canonical_id → list of const prop names).
    pub const_prop_overrides: std::collections::HashMap<String, Vec<String>>,
    /// Files whose constness changed since last computation (need recompilation).
    pub changed_files: Vec<String>,
    /// Diagnostics emitted during analysis.
    pub diagnostics: Vec<FfiCrossFileDiagnostic>,
}

/// A diagnostic from cross-file analysis.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiCrossFileDiagnostic {
    pub file_id: String,
    pub code: String,
    pub message: String,
}

// =============================================================================
// Code action types
// =============================================================================

/// A code action (quick fix, refactoring, or source action) for the playground.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiCodeAction {
    /// Human-readable title displayed in the IDE.
    pub title: String,
    /// Action kind: "quickfix", "refactor", or "source".
    pub kind: String,
    /// Text edits to apply.
    pub edits: Vec<FfiTextEdit>,
    /// Whether this is the preferred action for the diagnostic.
    pub is_preferred: bool,
    /// The lint rule this action fixes (if any).
    pub diagnostic_rule: Option<String>,
}

/// A single text edit within a code action.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiTextEdit {
    /// Start offset (UTF-16 for browser consumption).
    pub span_start: u32,
    /// End offset (UTF-16 for browser consumption).
    pub span_end: u32,
    /// Replacement text.
    pub new_text: String,
}

// =============================================================================
// Lint rule metadata
// =============================================================================

/// Metadata for a single lint rule, used by the rule browser UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiLintRuleMetadata {
    /// Rule name (e.g., "require-v-for-key").
    pub name: String,
    /// Rule category (e.g., "vue-essential").
    pub category: String,
    /// Default severity: "error", "warning", "info", or "hint".
    pub default_severity: String,
}

// =============================================================================
// Document symbol types
// =============================================================================

/// A document symbol for the outline/go-to-symbol panel.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDocumentSymbol {
    /// Symbol name.
    pub name: String,
    /// Additional detail (e.g., type annotation).
    pub detail: Option<String>,
    /// Symbol kind (Monaco SymbolKind number).
    pub kind: u32,
    /// Full span start (UTF-16).
    pub span_start: u32,
    /// Full span end (UTF-16).
    pub span_end: u32,
    /// Selection span start (UTF-16).
    pub selection_start: u32,
    /// Selection span end (UTF-16).
    pub selection_end: u32,
    /// Child symbols.
    pub children: Vec<FfiDocumentSymbol>,
}

// =============================================================================
// CSS selector match types
// =============================================================================

/// Result of matching all CSS selectors against all template elements.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiSelectorMatchResult {
    /// The CSS selector text.
    pub selector_text: String,
    /// Selector span start (UTF-16).
    pub selector_start: u32,
    /// Selector span end (UTF-16).
    pub selector_end: u32,
    /// Match results against template elements.
    pub matches: Vec<FfiElementMatch>,
}

/// Match result for a single element against a selector.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiElementMatch {
    /// Element tag name.
    pub tag: String,
    /// Element span start (UTF-16).
    pub span_start: u32,
    /// Element span end (UTF-16).
    pub span_end: u32,
    /// Match result: "match", "maybe", or "no".
    pub result: String,
}

// =============================================================================
// Component-meta result types (Rust → JS)
// =============================================================================

/// Compact wire form of the reachable origin subgraph for a component's
/// semantic results. Dense edge table with sequential node indices and
/// interned edge-meta strings.
#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct OriginGraphDto {
    pub nodes: Vec<OriginNodeDto>,
    pub edges: Vec<OriginEdgeDto>,
    pub meta_strings: Vec<String>,
}

/// One node in the origin subgraph.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OriginNodeDto {
    pub id: u32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One edge in the origin subgraph.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OriginEdgeDto {
    pub source: u32,
    pub target: u32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_index: Option<u32>,
}

/// NAPI/WASM boundary DTO for component metadata.
/// Derived from `ComponentMetaAnalysis` in `verter_semantic::analysis::component_meta`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentMeta {
    pub component_public_contract: FfiComponentContractAvailability,
    pub props: Vec<FfiPropMeta>,
    pub events: Vec<FfiEventMeta>,
    pub slots: Vec<FfiSlotMeta>,
    pub models: Vec<FfiModelMeta>,
    pub exposed: Vec<FfiExposedMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_instance: Option<FfiPublicInstanceMeta>,
    pub ordered_sfc_structure: FfiOrderedSfcStructure,
    pub type_registry: Vec<FfiResolvedTypeMeta>,
    pub components: Vec<FfiComponentUsage>,
    pub template_refs: Vec<FfiTemplateRefMeta>,
    pub imports: Vec<FfiImportMeta>,
    pub bindings: Vec<FfiBindingMeta>,
    pub vue_api_calls: Vec<FfiVueApiCallMeta>,
    pub styles: Vec<FfiStyleMeta>,
    pub flags: FfiComponentMetaFlags,
    pub accepted_props: Vec<FfiAcceptedPropMeta>,
    pub accepted_events: Vec<FfiAcceptedEventMeta>,
    pub accepted_surface_completeness: FfiAcceptedSurfaceCompleteness,
    pub root_info: FfiRootInfo,
    pub root_reachability: FfiRootReachability,
    pub fallthrough_surface: FfiFallthroughSurface,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub macro_expansion_diagnostics: Vec<FfiMacroExpansionDiagnostics>,
    pub options_api: bool,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<FfiComponentMetaResolution>,
    /// Typed status of the payload's type-resolution position: `Resolved`
    /// when the payload was produced with the `resolution` sidecar (the
    /// resolved type-registry overlay applied), else the typed
    /// `Unavailable(ResolutionProviderAbsent)` — a sidecar-less payload
    /// self-describes as partial on this axis and is NEVER an
    /// exact/successful-looking silence. Always serialized (additive wire
    /// field; every pre-existing field is unchanged).
    pub resolution_status: FfiComponentMetaResolutionStatus,
    /// Typed completeness of the PUBLISHED SURFACE: `Complete` when the
    /// payload is the full surface this component declares, `Partial` (with
    /// its typed reasons) when the producing compute degraded and members may
    /// be missing or unverified. Distinct from
    /// [`Self::accepted_surface_completeness`], which describes only the
    /// computed call-site accepted surface.
    ///
    /// Without it an empty-because-degraded payload is byte-identical to an
    /// empty-because-nothing-is-declared payload — the wrong-complete
    /// outcome. A `Partial` payload is also refused warm admission by the
    /// producer, so an identical follow-up request recomputes.
    pub result_completeness: FfiResultCompleteness,
    #[serde(skip_serializing_if = "origin_graph_is_empty")]
    pub origin: OriginGraphDto,
}

/// Typed completeness of a published component-meta surface.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiResultCompleteness {
    /// Every demanded position resolved; the payload is the full surface.
    Complete,
    /// The producing compute degraded: the payload is structurally
    /// incomplete and carries the typed reasons why.
    Partial {
        /// The recorded reasons, in the producer's stable order. Never
        /// empty — a partial state without a reason is indistinguishable
        /// from a complete one.
        reasons: Vec<FfiSurfacePartialReason>,
    },
}

/// Why a published component-meta surface is partial. Closed taxonomy,
/// one variant per producer-recorded reason class.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum FfiSurfacePartialReason {
    /// An armed runaway fuse tripped mid-compute.
    BudgetExceeded,
    /// The request was cancelled mid-compute.
    Cancelled,
    /// The world generation advanced and superseded the compute.
    SupersededGeneration,
    /// An unstable / torn intermediate state was observed.
    UnstableState,
    /// Same-path recursion produced a non-terminating self-reference.
    SamePathRecursion,
    /// The terminal-surface walker hit a fatal diagnostic and contributed
    /// no surface.
    WalkerFatal,
    /// Partiality inherited from a contributing read whose own reason class
    /// was not captured at the propagation site.
    Propagated,
    /// A deferred-evaluation ceiling was reached.
    DeferredEvaluationLimit,
    /// A structural-fact demand ceiling was reached.
    StructuralFactDemandLimit,
    /// A resolution read returned a query fault that no narrower class
    /// already describes.
    SemanticQueryFault,
    /// A demanded node had no live data in the shared semantic graph.
    MissingSemanticNodeData,
    /// The connected demand exceeded its total projection/evaluation work
    /// envelope.
    ProjectionWorkLimit,
    /// A nested cold-query chain exceeded the host-recursion ceiling.
    ConnectedQueryDepthLimit,
    /// An exact authored declaration was available but at least one imported
    /// dependency owner could not be resolved.
    MissingDependency,
    /// A body-derived return produced a usable value in which one interior
    /// position has no modelled type and says so.
    FlowReturnUninferred,
    /// A body-derived return produced a usable value the producer could not
    /// fully verify: every member is present, one may be wrong.
    FlowReturnUnverified,
    /// A body-derived demand produced no value at all.
    FlowReturnNoSurface,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiComponentContractAvailability {
    Supported {
        contract: FfiComponentPublicContract,
    },
    Unsupported {
        unsupported: FfiComponentContractUnsupported,
    },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentContractUnsupported {
    pub adapter_id: String,
    pub reason: FfiComponentContractUnsupportedReason,
    pub diagnostics: Vec<FfiResolutionDiagnostic>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiComponentContractUnsupportedReason {
    AdapterUnavailable,
    ComponentMetaUnavailable,
    OutputMaterializationFailed {
        lane: FfiComponentMetaOutputLane,
        index: u32,
        inner_index: Option<u32>,
        failure: FfiComponentMetaOutputFailure,
    },
    PublicationFailed {
        surface: FfiContractSurface,
        failure: FfiTypePublicationFailure,
        provenance: FfiResolutionProvenance,
    },
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum FfiComponentMetaOutputLane {
    Prop,
    EventPayload,
    EventReturn,
    SlotBinding,
    SlotReturn,
    Model,
    Exposed,
    PublicInstanceMember,
    TypeRegistryEntry,
    AcceptedProp,
    AcceptedEventPayload,
    FallthroughProp,
    FallthroughEventPayload,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiComponentMetaOutputFailure {
    UnraisableSource,
    RequiredSourceUnavailable { failure: FfiTypePublicationFailure },
    InteriorSourceMiss,
    ShellMaterializationMiss,
    UnknownMaterializingSourceInterior,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentPublicContract {
    pub adapter_id: String,
    pub exactness: FfiContractExactness,
    pub degradation: Vec<FfiContractDegradation>,
    pub provenance: FfiContractProvenance,
    pub props: Vec<FfiPublicProp>,
    pub events: Vec<FfiPublicEvent>,
    pub slots: Vec<FfiPublicSlot>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum FfiContractExactness {
    Exact,
    Degraded,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum FfiContractProvenance {
    ComponentMetaOutput,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiContractDegradation {
    pub surface: FfiContractSurface,
    pub reason: FfiContractDegradationReason,
    pub diagnostics: Vec<FfiResolutionDiagnostic>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum FfiContractDegradationReason {
    Absent,
    Incomplete,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiContractSurface {
    Prop { name: String },
    Event { name: String, overload_index: u32 },
    SlotBinding { slot: String, binding: String },
    SlotReturn { slot: String },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolutionDiagnostic {
    pub kind: FfiResolutionDiagnosticKind,
    pub context: String,
    pub property_name: Option<String>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum FfiResolutionDiagnosticKind {
    BudgetExceeded,
    ProjectionWorkLimit,
    ConnectedQueryDepthLimit,
    MappedDepthExceeded,
    UnresolvedReference,
    IndeterminateConditional,
    InfiniteKeySpace,
    UnsupportedOperator,
    ConditionalContextTruncated,
    IdempotentArm,
    CyclicReference,
    CyclicInstantiation,
    InstantiationError,
    EmptyUnionArm,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicTypeReference {
    pub r#type: Option<verter_type_expr::TypeExpr>,
    pub publication: FfiTypePublication,
    pub terminal_display: FfiTerminalTypeDisplay,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicProp {
    pub name: String,
    pub optional: bool,
    pub has_default: bool,
    pub ty: FfiPublicTypeReference,
    pub exactness: FfiContractExactness,
    pub degradation: Vec<FfiContractDegradation>,
    pub provenance: FfiContractProvenance,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicEvent {
    pub name: String,
    pub overloads: Vec<FfiPublicCallSignature>,
    pub derived_handler: FfiPublicDerivedHandlerShape,
    pub exactness: FfiContractExactness,
    pub degradation: Vec<FfiContractDegradation>,
    pub provenance: FfiContractProvenance,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicCallSignature {
    pub source: FfiPublicTypeReference,
    pub parameters: Vec<FfiPublicParameter>,
    pub return_type: verter_type_expr::TypeExpr,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicParameter {
    pub name: Option<String>,
    pub optional: bool,
    pub rest: bool,
    pub ty: verter_type_expr::TypeExpr,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicDerivedHandlerShape {
    pub overloads: Vec<FfiPublicHandlerSignature>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicHandlerSignature {
    pub parameters: Vec<FfiPublicParameter>,
    pub return_type: verter_type_expr::TypeExpr,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicSlot {
    pub name: String,
    pub optional: bool,
    pub input: FfiPublicSlotInput,
    pub return_type: Option<FfiPublicTypeReference>,
    pub exactness: FfiContractExactness,
    pub degradation: Vec<FfiContractDegradation>,
    pub provenance: FfiContractProvenance,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicSlotInput {
    pub bindings: Vec<FfiPublicSlotBinding>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicSlotBinding {
    pub name: String,
    pub ty: FfiPublicTypeReference,
}

/// Typed resolution status of a component-meta payload — see
/// [`FfiComponentMeta::resolution_status`].
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", tag = "kind", content = "reason")]
pub enum FfiComponentMetaResolutionStatus {
    /// The payload carries the `resolution` sidecar (the resolved
    /// type-registry overlay is applied).
    Resolved,
    /// The payload was produced WITHOUT the resolution sidecar: the
    /// type registry and declaration metadata are the un-overlaid
    /// view — a typed partial status, never an implied exact success.
    Unavailable(FfiResolutionUnavailableReason),
}

/// Why a payload's type-resolution position is unavailable.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum FfiResolutionUnavailableReason {
    /// The producing entry ran without the resolution sidecar/seed (the
    /// sidecar-less output-envelope surfaces — e.g. the plain WASM lane,
    /// which has no filesystem/project resolver behind it).
    ResolutionProviderAbsent,
}

fn origin_graph_is_empty(g: &OriginGraphDto) -> bool {
    g.edges.is_empty()
}

/// Macro-wide expansion diagnostics that apply to an entire macro, not to a
/// specific property. One entry per macro that has global diagnostics.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiMacroExpansionDiagnostics {
    pub macro_kind: String,
    pub macro_index: u32,
    pub exactness: String,
    pub execution_status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<FfiExpansionDiagnostic>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiExpansionDiagnostic {
    pub reason: String,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_name: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiExpansionMetadata {
    pub exactness: String,
    pub execution_status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<FfiExpansionDiagnostic>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiTerminalTypeDisplay {
    pub text: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiTypePublication {
    Failed {
        failure: FfiTypePublicationFailure,
        provenance: FfiResolutionProvenance,
    },
    Absent {
        absence: FfiTypePublicationAbsence,
        provenance: FfiResolutionProvenance,
    },
    Published {
        semantic_authority: FfiPublicationSemanticAuthority,
        exactness: FfiPublicationExactness,
        reason: FfiPublicationReason,
        provenance: FfiPublicationProvenance,
    },
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiTypePublicationFailure {
    UnrepresentableRequiredMemberValue,
    UnrepresentableRequiredPayload,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiTypePublicationAbsence {
    Unannotated,
    BranchDivergent,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiResolutionProvenance {
    SemanticEvaluator,
    SessionProjector,
    FrameworkSurface,
    FallthroughInheritance,
    Schema,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiAuthoredProvenance {
    MacroPayload,
    DeclarationBody,
    AugmentationBody,
    JsdocTypedefBody,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FfiPublicationProvenance {
    Resolved(FfiResolutionProvenance),
    Authored(FfiAuthoredProvenance),
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiPublicationSemanticAuthority {
    Resolved,
    AuthoredFallback,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiPublicationExactness {
    ExactConcrete,
    ExactSymbolic,
    Incomplete,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiPublicationReason {
    ResolvedExactConcrete,
    ResolvedExactSymbolic,
    ResolvedIncomplete,
    AuthoredForIncomplete { policy: FfiPublicationPolicyReason },
    AuthoredSymbolicRepresentation { proof: FfiSymbolicEquivalenceKind },
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiPublicationPolicyReason {
    ImportedMacroCompound,
    ImportedIndexedAccess,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FfiSymbolicEquivalenceKind {
    ImportedMacroCompound,
    ImportedIndexedAccess,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPropMeta {
    pub name: String,
    /// Structured type IR (passes through unchanged — TypeExpr implements Serialize).
    pub r#type: Option<verter_type_expr::TypeExpr>,
    pub publication: FfiTypePublication,
    pub terminal_display: FfiTerminalTypeDisplay,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
    pub required: bool,
    pub has_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
    /// Producer fact: did the SFC author write this prop name explicitly as
    /// a member of the `defineProps<T>()` type argument's own body (or its
    /// directly-referenced interface's own body)? Distinguishes
    /// author-declared names from names that arrived via heritage / utility-
    /// type expansion. Consumed by
    /// `@verter/component-meta/published-surface`'s `Refined` policy to
    /// preserve Vue intrinsics (`class`/`style`/etc.) and `on{Event}`
    /// shadow-emit props when the author kept them on purpose.
    pub declared_in_macro_type_arg: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiEventMeta {
    pub name: String,
    pub payload: verter_type_expr::TypeExpr,
    pub publication: FfiTypePublication,
    pub terminal_display: FfiTerminalTypeDisplay,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_expansion: Option<FfiExpansionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiSlotMeta {
    pub name: String,
    pub is_scoped: bool,
    pub bindings: Vec<FfiSlotBindingMeta>,
    pub is_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    pub return_value: Option<verter_type_expr::TypeExpr>,
    pub return_publication: Option<FfiTypePublication>,
    pub return_terminal_display: Option<FfiTerminalTypeDisplay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
    /// Producer fact: does this slot come from the component's own AUTHORED
    /// slots surface (the resolved `defineSlots<T>()` macro surface or a
    /// template `<slot>` element)? `false` only for rows arriving purely
    /// through the evaluated type-expansion channel. Consumed by
    /// `@verter/component-meta/published-surface`'s `Compat` / `Refined`
    /// slot blocklist — an author-declared slot is never blocked.
    pub declared_in_macro_type_arg: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiSlotBindingMeta {
    pub name: String,
    pub r#type: Option<verter_type_expr::TypeExpr>,
    pub publication: FfiTypePublication,
    pub terminal_display: FfiTerminalTypeDisplay,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiModelMeta {
    pub name: String,
    pub r#type: verter_type_expr::TypeExpr,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiExposedMeta {
    pub name: String,
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicInstanceMeta {
    pub completeness: String,
    pub members: Vec<FfiPublicInstanceMemberMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiPublicInstanceMemberMeta {
    pub name: String,
    pub kind: String,
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiOrderedSfcStructure {
    pub schema_version: u32,
    pub artifact_token: String,
    pub blocks: Vec<FfiStructureBlock>,
    pub markup_nodes: Vec<FfiMarkupNode>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiStructureBlock {
    Section {
        section: Box<FfiStructureSection>,
        markup_root_tokens: Vec<String>,
    },
    MarkupRoot {
        block_token: String,
        markup_root_token: String,
    },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiStructureSection {
    pub block_token: String,
    pub role: FfiCarrierBlockRole,
    pub authored_name: FfiAuthoredName,
    pub opening_range: FfiStructureRange,
    pub opening_name_range: FfiStructureRange,
    pub content_range: FfiStructureRange,
    pub closing_range: Option<FfiStructureRange>,
    pub closing_name_range: Option<FfiStructureRange>,
    pub full_range: FfiStructureRange,
    pub termination: FfiSyntaxTermination,
    pub attributes: Vec<FfiCarrierAttribute>,
    /// Reserved content basis slot; structure-only producers leave it absent.
    pub block_content_basis_token: Option<String>,
    pub attribute_insertion_anchor: FfiStructureRange,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiCarrierBlockRole {
    TemplateHost,
    Script {
        role: String,
        dialect: String,
    },
    Style {
        dialect: String,
        scoped: bool,
        module: String,
    },
    Custom {
        normalized_name: String,
    },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiAuthoredName {
    pub spelling: String,
    pub normalized: String,
    pub range: FfiStructureRange,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiStructureRange {
    pub source_space_token: String,
    pub start: u32,
    pub end: u32,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiSyntaxTermination {
    Closed,
    SelfClosing,
    Void,
    UnclosedEof,
    Recovered {
        reason: String,
        recovery_range: Option<FfiStructureRange>,
    },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiCarrierAttribute {
    pub attribute_token: String,
    pub kind: String,
    pub name: Option<FfiAuthoredName>,
    pub value: Option<String>,
    pub full_range: FfiStructureRange,
    pub duplicate_of: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiMarkupNode {
    pub node_token: String,
    pub parent_node_token: Option<String>,
    pub child_node_tokens: Vec<String>,
    pub syntax: FfiMarkupSyntax,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiMarkupSyntax {
    Element {
        authored_name: FfiAuthoredName,
        namespace: String,
        element_kind: String,
        opening_range: FfiStructureRange,
        opening_name_range: FfiStructureRange,
        attribute_insertion_anchor: FfiStructureRange,
        content_range: FfiStructureRange,
        closing_range: Option<FfiStructureRange>,
        closing_name_range: Option<FfiStructureRange>,
        full_range: FfiStructureRange,
        self_closing: bool,
        void_element: bool,
        raw_text: bool,
        termination: FfiSyntaxTermination,
        attributes: Vec<FfiCarrierAttribute>,
    },
    Text {
        content_range: FfiStructureRange,
    },
    Comment {
        opening_range: FfiStructureRange,
        content_range: FfiStructureRange,
        closing_range: Option<FfiStructureRange>,
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
    },
    Interpolation {
        family: String,
        opening_range: FfiStructureRange,
        expression_range: FfiStructureRange,
        closing_range: Option<FfiStructureRange>,
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
    },
    SvelteControlBlock {
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
    },
    SvelteClause {
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
    },
    SvelteStandaloneTag {
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
    },
    Recovered {
        opening_range: Option<FfiStructureRange>,
        opening_name_range: Option<FfiStructureRange>,
        content_range: Option<FfiStructureRange>,
        closing_range: Option<FfiStructureRange>,
        closing_name_range: Option<FfiStructureRange>,
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
        expected: String,
        reason: String,
    },
    Unknown {
        opening_range: Option<FfiStructureRange>,
        opening_name_range: Option<FfiStructureRange>,
        content_range: Option<FfiStructureRange>,
        closing_range: Option<FfiStructureRange>,
        closing_name_range: Option<FfiStructureRange>,
        full_range: FfiStructureRange,
        termination: FfiSyntaxTermination,
        authored_head: Option<String>,
        reason: String,
    },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedTypeMeta {
    pub name: String,
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration: Option<FfiResolvedTypeDeclaration>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentMetaResolution {
    pub mode: String,
    pub macros: Vec<FfiResolvedMacroMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedMacroMeta {
    pub macro_index: u32,
    pub macro_kind: String,
    pub type_name: String,
    pub import_source: String,
    pub declaration: FfiResolvedTypeDeclaration,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub native_props: Vec<FfiResolvedNativeProp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsdoc: Option<FfiResolvedJsdocBlock>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedTypeDeclaration {
    pub requested_name: String,
    pub resolved_name: String,
    pub canonical_source: String,
    pub span_start: u32,
    pub span_end: u32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    pub visibility: String,
    pub span_start: u32,
    pub span_end: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedJsdocBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiResolvedJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedJsdocTag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_name: Option<String>,
    /// The SEALED resolved-type output snapshot (display + wire-node graph) —
    /// never a raw `TypeExpr`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_type: Option<crate::graph::snapshot::ResolvedJsdocTypeOutput>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentUsage {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_source: Option<String>,
    pub is_dynamic: bool,
    pub props: Vec<FfiComponentPropUsage>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_spread: bool,
    pub slots_used: Vec<String>,
    pub static_classes: Vec<String>,
    pub has_dynamic_class: bool,
    pub v_models: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub v_model_entries: Vec<FfiComponentVModelEntry>,
    /// Framework-neutral two-way bindings (the Svelte `bind:` family). Empty for
    /// Vue.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<FfiComponentBindingUsage>,
    /// Framework-neutral events (the legacy Svelte `on:` directive only — a
    /// plain `on*` attribute is a prop, never an event). Empty for Vue.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<FfiComponentEventUsage>,
}

/// A two-way binding passed to a child component (the Svelte `bind:` family).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentBindingUsage {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

/// An event listened on a child component via the legacy Svelte `on:`
/// directive. A plain `on*` attribute is a prop, never an event (the
/// props/events split is syntactic — the child component-meta, not a name
/// guess, decides which passed props are callback events).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentEventUsage {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handler_expression: Option<String>,
    pub is_inline: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentPropUsage {
    pub name: String,
    pub is_bound: bool,
    pub constness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub referenced_bindings: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_spread: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_shorthand: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentVModelEntry {
    pub binding_name: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiTemplateRefMeta {
    pub name: String,
    pub is_dynamic: bool,
    pub target_tag: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiImportMeta {
    pub source: String,
    pub is_type_only: bool,
    pub bindings: Vec<FfiImportBindingMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiImportBindingMeta {
    pub name: String,
    pub kind: String,
    pub imported_name: Option<String>,
    pub is_type_only: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiBindingMeta {
    pub name: String,
    pub kind: String,
    pub reactivity_kind: String,
    /// The demand-resolved whole-return reactive-wrapper role of a composable
    /// binding, as a CLOSED vocabulary discriminant. Absent when no role was
    /// demanded for the binding. `reactivity_kind` cannot carry this: it has no
    /// degraded arm, so "proven not a Vue wrapper" and "could not resolve" are
    /// indistinguishable on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_wrapper_role: Option<String>,
    /// The exact typed reason for a `"unresolved"` role. Present only with that
    /// role, so a degradation never collapses onto the bare discriminant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_wrapper_unresolved_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    pub used_in_template: bool,
    pub used_in_style: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiVueApiCallMeta {
    pub api: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg_value: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiStyleMeta {
    pub lang: String,
    pub scoped: bool,
    pub is_module: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_name: Option<String>,
    /// Opaque sealed block token binding this style analysis to its
    /// structure block (same vocabulary as the ordered-structure block
    /// tokens). Absent when the sealed identity could not be revalidated —
    /// consumers treat absence as typed unavailable, never ordinal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_token: Option<String>,
    pub classes: Vec<String>,
    pub ids: Vec<String>,
    pub custom_properties: Vec<String>,
    pub v_binds: Vec<String>,
    pub selectors: Vec<FfiSelectorMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiSelectorMeta {
    pub text: String,
    pub specificity: (u32, u32, u32),
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiComponentMetaFlags {
    pub async_setup: bool,
    pub has_reactive_state: bool,
    pub has_computed: bool,
    pub has_watchers: bool,
    pub has_lifecycle_hooks: bool,
    pub has_provide: bool,
    pub has_inject: bool,
    pub has_inherit_attrs_false: bool,
    pub has_store_usage: bool,
    /// D123 (Tier 1A) — set when lowering produced a `LoweringError`
    /// (macro-impacting unsupported AST kind). Paired with a
    /// `macro_expansion_diagnostics` entry under D117. NAPI does NOT
    /// throw exceptions for macro failures; this flag plus the
    /// diagnostic is the consumer-visible surface.
    #[serde(default)]
    pub has_macro_failure: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiJsdocTag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

// =============================================================================
// Component-meta: fallthrough surface types (Rust → JS)
// =============================================================================

/// Root reachability classification for fallthrough inheritance.
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiRootReachability {
    /// No fallthrough inheritance is possible.
    #[serde(rename_all = "camelCase")]
    NoFallthrough { reason: FfiNoFallthroughReason },
    /// One or more conditional branches, each with exactly one root target.
    #[serde(rename_all = "camelCase")]
    Branches { branches: Vec<FfiRootBranch> },
}

/// Why a component has no fallthrough surface.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FfiNoFallthroughReason {
    InheritAttrsFalse,
    MultiRoot,
    BranchNotSingleRoot,
    RootVFor,
    NoTemplate,
    EmptyTemplate,
    TextOrInterpolationRoot,
}

/// A single root render branch.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiRootBranch {
    pub branch_index: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_text: Option<String>,
    pub target: FfiRootTargetRef,
    pub consumed: FfiConsumedRootBindings,
    pub has_unknown_spread: bool,
}

/// The kind of root render target.
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiRootTargetRef {
    #[serde(rename_all = "camelCase")]
    NativeElement { element_index: u32, tag: String },
    #[serde(rename_all = "camelCase")]
    DynamicComponentUsage {
        element_index: u32,
        usage_index: u32,
    },
    #[serde(rename_all = "camelCase")]
    ComponentUsage {
        element_index: u32,
        usage_index: u32,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        import_source: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    UnresolvedTarget {
        element_index: u32,
        tag: String,
        reason: FfiUnresolvedRootTargetReason,
    },
}

/// Why a root target cannot be resolved for fallthrough inheritance.
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiUnresolvedRootTargetReason {
    DynamicComponentIs,
    SlotOutlet,
    #[serde(rename_all = "camelCase")]
    UnsupportedBuiltin {
        tag: String,
    },
    MissingUsageLink,
    UnresolvedImport,
    UnknownRootTarget,
}

/// Attrs/listeners explicitly bound on the root element.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiConsumedRootBindings {
    pub attrs: Vec<String>,
    pub listeners: Vec<String>,
    pub has_dynamic_attr_name: bool,
    pub has_dynamic_listener_name: bool,
}

/// First-class root summary for consumers that do not want to reconstruct it
/// from the full branch graph.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiRootInfo {
    pub kind: FfiRootInfoKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<FfiNoFallthroughReason>,
    pub targets: Vec<FfiRootTargetRef>,
}

/// Coarse root summary kind.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FfiRootInfoKind {
    None,
    Single,
    Conditional,
    Multiple,
}

/// Why generic-root specialization could not resolve a concrete instantiation.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FfiGenericResolutionFailure {
    SpreadInput,
    DynamicKey,
    MissingType,
    UnsupportedExpression,
    MissingUsageLink,
    UnresolvedChildGenericSurface,
}

/// Known lower-bound causes for a partially resolved fallthrough branch.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiPartialBranchReason {
    DynamicAttrName,
    DynamicListenerName,
    UnknownSpread,
    #[serde(rename_all = "camelCase")]
    GenericResolution {
        failure: FfiGenericResolutionFailure,
    },
}

/// Why a fallthrough branch could not be resolved at all.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiUnresolvedBranchReason {
    #[serde(rename_all = "camelCase")]
    Cycle {
        canonical_id: String,
    },
    DynamicComponentIs,
    ChildResolutionFailed,
    #[serde(rename_all = "camelCase")]
    UnresolvedChildImport {
        #[serde(skip_serializing_if = "Option::is_none")]
        import_source: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    RootTarget {
        reason: FfiUnresolvedRootTargetReason,
    },
    #[serde(rename_all = "camelCase")]
    GenericResolution {
        failure: FfiGenericResolutionFailure,
    },
}

/// How a member arrived on the accepted surface.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiMemberProvenance {
    /// Member is declared locally.
    Declared,
    /// Member is inherited from one or more fallthrough sources.
    #[serde(rename_all = "camelCase")]
    Inherited { sources: Vec<FfiInheritedSource> },
}

/// A single inheritance source.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiInheritedSource {
    /// Inherited from a native HTML element.
    #[serde(rename_all = "camelCase")]
    NativeTag { tag: String },
    /// Inherited from a child component.
    #[serde(rename_all = "camelCase")]
    Component { canonical_id: String },
}

/// Whether a member is always available or only in certain branches.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiMemberAvailability {
    /// Available in all branches.
    Always,
    /// Available only in specific branches.
    #[serde(rename_all = "camelCase")]
    Conditional { branch_keys: Vec<String> },
}

/// Kind of accepted prop (camelCase string).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FfiAcceptedPropKind {
    DeclaredProp,
    Attr,
}

/// Kind of accepted event (camelCase string).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FfiAcceptedEventKind {
    DeclaredEmit,
    Listener,
}

/// Whether the accepted surface is exact or only a lower bound (camelCase string).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum FfiAcceptedSurfaceCompleteness {
    Exact,
    LowerBound,
}

/// An accepted prop on the computed call-site surface.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiAcceptedPropMeta {
    pub name: String,
    pub r#type: Option<verter_type_expr::TypeExpr>,
    pub publication: FfiTypePublication,
    pub terminal_display: FfiTerminalTypeDisplay,
    pub required: bool,
    pub provenance: FfiMemberProvenance,
    pub availability: FfiMemberAvailability,
    pub kind: FfiAcceptedPropKind,
}

/// An accepted event on the computed call-site surface.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiAcceptedEventMeta {
    pub name: String,
    pub payload: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_signature: Option<String>,
    pub provenance: FfiMemberProvenance,
    pub availability: FfiMemberAvailability,
    pub kind: FfiAcceptedEventKind,
}

/// The branch-structured inherited surface.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiFallthroughSurface {
    /// No fallthrough inheritance.
    #[serde(rename_all = "camelCase")]
    None { reason: FfiNoFallthroughReason },
    /// Branch-structured inherited props and events.
    #[serde(rename_all = "camelCase")]
    Branches { branches: Vec<FfiFallthroughBranch> },
}

/// An inherited prop entry in a fallthrough branch.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiFallthroughPropEntry {
    pub name: String,
    pub r#type: Option<verter_type_expr::TypeExpr>,
    pub publication: FfiTypePublication,
    pub terminal_display: FfiTerminalTypeDisplay,
    pub sources: Vec<FfiInheritedSource>,
}

/// An inherited event entry in a fallthrough branch.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiFallthroughEventEntry {
    pub name: String,
    pub payload: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_signature: Option<String>,
    pub sources: Vec<FfiInheritedSource>,
}

/// Status of a fallthrough branch.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiBranchStatus {
    /// All members in this branch are exactly known.
    Resolved,
    /// Some members are known but the branch may have additional unknown members.
    #[serde(rename_all = "camelCase")]
    PartiallyUnresolved {
        reasons: Vec<FfiPartialBranchReason>,
    },
    /// This branch could not be resolved at all.
    #[serde(rename_all = "camelCase")]
    Unresolved { reason: FfiUnresolvedBranchReason },
}

/// A single step in the root resolution chain.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FfiResolvedRootStep {
    /// Native HTML element target.
    #[serde(rename_all = "camelCase")]
    NativeTag { tag: String },
    /// Resolved child component target.
    #[serde(rename_all = "camelCase")]
    Component {
        canonical_id: String,
        component_name: String,
    },
    /// Unresolved root target.
    #[serde(rename_all = "camelCase")]
    Unresolved {
        tag: String,
        reason: FfiUnresolvedBranchReason,
    },
}

/// A single branch in the fallthrough surface.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiFallthroughBranch {
    pub branch_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_text: Option<String>,
    pub props: Vec<FfiFallthroughPropEntry>,
    pub events: Vec<FfiFallthroughEventEntry>,
    pub root_chain: Vec<FfiResolvedRootStep>,
    pub status: FfiBranchStatus,
}
