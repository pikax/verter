//! FFI-boundary types shared between NAPI and WASM bindings.
//!
//! All structs use `#[serde(rename_all = "camelCase")]` so field names
//! match JavaScript convention when serialized. WASM uses these types
//! directly via `serde_wasm_bindgen`; NAPI maps to/from its own
//! `#[napi(object)]` structs via zero-copy `From` impls.

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
    pub force_vapor: Option<bool>,
    pub force_js: Option<bool>,
    pub source_map: Option<bool>,
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

/// A single preprocessed style block override.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiStyleOverrideEntry {
    pub index: u32,
    pub code: String,
    pub source_map: Option<String>,
}

/// Request to apply preprocessed style overrides.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiStyleOverrideRequest {
    pub canonical_id: String,
    pub compile_profile: Option<FfiCompileProfile>,
    pub overrides: Vec<FfiStyleOverrideEntry>,
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
}

/// Result of removing a file from the host.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiRemoveResult {
    pub canonical_id: String,
}
