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
}

/// Per-compilation variant options.
#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiCompileProfile {
    pub filename: Option<String>,
    pub is_production: Option<bool>,
    pub ssr: Option<bool>,
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

/// A single preprocessed block override (template, script, style, or custom).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiBlockOverrideEntry {
    /// Block type: "template", "script", "style", or "custom".
    pub block_type: String,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: u32,
    /// Preprocessed code.
    pub code: String,
    /// Source map from the preprocessor, if available.
    pub source_map: Option<String>,
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
    pub span_start: Option<u32>,
    pub span_end: Option<u32>,
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
    pub index: u32,
    pub specifier: String,
    pub resolved_canonical_id: String,
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
    /// Block type: "template", "script", "style", or "custom".
    pub block_type: String,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: u32,
    /// The `lang` attribute value (e.g., "pug", "coffee", "scss").
    pub lang: String,
    /// Raw content of the block that needs preprocessing.
    pub content: String,
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
    pub style_index: Option<u32>,
    pub custom_index: Option<u32>,
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
    pub props: Vec<FfiPropMeta>,
    pub events: Vec<FfiEventMeta>,
    pub slots: Vec<FfiSlotMeta>,
    pub models: Vec<FfiModelMeta>,
    pub exposed: Vec<FfiExposedMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_instance: Option<FfiPublicInstanceMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sfc_blocks: Option<FfiSfcBlocksMeta>,
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
    #[serde(skip_serializing_if = "origin_graph_is_empty")]
    pub origin: OriginGraphDto,
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
pub struct FfiPropMeta {
    pub name: String,
    /// Structured type IR (passes through unchanged — TypeExpr implements Serialize).
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiSlotBindingMeta {
    pub name: String,
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_expansion: Option<FfiExpansionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
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
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiSfcBlocksMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<FfiTemplateBlockMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<FfiScriptBlockMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_setup: Option<FfiScriptBlockMeta>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<FfiStyleBlockMeta>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<FfiCustomBlockMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiSfcAttributeMeta {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiTemplateBlockMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<FfiSfcAttributeMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiScriptBlockMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attrs_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<FfiSfcAttributeMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiStyleBlockMeta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    pub scoped: bool,
    pub is_module: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<FfiSfcAttributeMeta>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiCustomBlockMeta {
    pub index: u32,
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<FfiSfcAttributeMeta>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<FfiResolvedPropField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<FfiResolvedEmitField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<FfiResolvedSlotField>,
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
pub struct FfiResolvedPropField {
    pub name: String,
    pub is_optional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedEmitField {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedSlotField {
    pub name: String,
    pub is_required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<FfiResolvedSlotBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FfiJsdocTag>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FfiResolvedSlotBinding {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_type: Option<verter_type_expr::TypeExpr>,
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
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
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
    pub r#type: verter_type_expr::TypeExpr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
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
