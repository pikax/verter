//! Helper types and free functions for the typed WASM audit
//! entry-points. The actual `#[wasm_bindgen]` impl block lives in
//! `lib.rs` because wasm-bindgen's class registration looks up the
//! `js_name = VerterHost` rename on the struct's containing module.
//! Splitting the impl across modules would lose the rename.

use serde::Deserialize;
use wasm_bindgen::prelude::*;

use verter_audit::{payloads::tags::BundlerKindTag, RequestAuditRecord, RequestKind, WorkspaceOp};
use verter_compiler::compile::CompileTarget;

/// Workspace op argument decoded from a JSON string. Mirrors the
/// NAPI variant. The payload tag is `type`; the WASM transport is a
/// JSON string rather than a `JsValue` so the deserializer is shared.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum WorkspaceOpArgWasm {
    AuditResolve { specifier: String, from: String },
    DepGraphTraverse { root: String },
    ResolverWalk { specifier: String },
}

impl From<WorkspaceOpArgWasm> for WorkspaceOp {
    fn from(arg: WorkspaceOpArgWasm) -> Self {
        match arg {
            WorkspaceOpArgWasm::AuditResolve { specifier, from } => {
                WorkspaceOp::AuditResolve { specifier, from }
            }
            WorkspaceOpArgWasm::DepGraphTraverse { root } => WorkspaceOp::DepGraphTraverse { root },
            WorkspaceOpArgWasm::ResolverWalk { specifier } => {
                WorkspaceOp::ResolverWalk { specifier }
            }
        }
    }
}

/// Filter for `getAuditRecords`. `serde_wasm_bindgen` decodes the
/// JS object into this shape.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuditRecordFilterWasm {
    pub kind: Option<String>,
    pub since_request_id: Option<String>,
    pub limit: Option<u32>,
}

/// Args for `getBundlerBatchSummary`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BundlerBatchSummaryArgsWasm {
    pub kind: Option<String>,
    pub since_request_id: Option<String>,
}

/// Convert a `RequestAuditRecord` into a JSON string `JsValue`.
pub(crate) fn audit_record_to_json_string(record: &RequestAuditRecord) -> Result<JsValue, JsValue> {
    serde_json::to_string(record)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| JsValue::from_str(&format!("audit record serialization error: {e}")))
}

/// Convert a list of `RequestAuditRecord`s into a JSON string `JsValue`.
pub(crate) fn audit_record_list_to_json_string(
    records: &[RequestAuditRecord],
) -> Result<JsValue, JsValue> {
    serde_json::to_string(records)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| JsValue::from_str(&format!("audit record list serialization error: {e}")))
}

/// Map a string target name to a `CompileTarget`. Mirrors the NAPI
/// variant.
pub(crate) fn parse_compile_target_wasm(name: &str) -> Result<CompileTarget, JsValue> {
    match name {
        "BUNDLER" => Ok(CompileTarget::BUNDLER),
        "IDE" => Ok(CompileTarget::IDE),
        "ANALYSIS" => Ok(CompileTarget::ANALYSIS),
        "META" => Ok(CompileTarget::META),
        "TSX" => Ok(CompileTarget::TSX),
        "TSC" => Ok(CompileTarget::TSC),
        other => Err(JsValue::from_str(&format!(
            "unknown compile target '{other}'; expected one of: BUNDLER, IDE, ANALYSIS, META, TSX, TSC"
        ))),
    }
}

/// Match the textual `kind` filter against a `RequestKind`.
pub(crate) fn kind_matches_wasm(filter: &str, kind: &RequestKind) -> bool {
    matches!(
        (filter, kind),
        ("ComponentMeta", RequestKind::ComponentMeta)
            | ("TypeResolution", RequestKind::TypeResolution)
            | ("SemanticAnalysis", RequestKind::SemanticAnalysis)
            | ("Compile", RequestKind::Compile { .. })
            | ("Workspace", RequestKind::Workspace { .. })
            | ("Lsp", RequestKind::Lsp { .. })
            | ("Mcp", RequestKind::Mcp { .. })
            | ("BundlerBatch", RequestKind::BundlerBatch { .. })
            | ("Custom", RequestKind::Custom { .. })
            | ("TypeInfoGraph", RequestKind::TypeInfoGraph)
    )
}

/// Map a textual bundler-kind tag.
pub(crate) fn parse_bundler_kind_wasm(name: Option<&str>) -> BundlerKindTag {
    match name.unwrap_or("Vite") {
        "Vite" => BundlerKindTag::Vite,
        "Webpack" => BundlerKindTag::Webpack,
        "Rollup" => BundlerKindTag::Rollup,
        "Esbuild" => BundlerKindTag::Esbuild,
        "Rolldown" => BundlerKindTag::Rolldown,
        other => BundlerKindTag::Other(other.to_string()),
    }
}

/// Parse a decimal-string request id (matching the JSON
/// serialization on the record) into a `u64`.
pub(crate) fn parse_request_id_str_wasm(s: &str) -> Result<u64, JsValue> {
    s.parse::<u64>()
        .map_err(|e| JsValue::from_str(&format!("expected decimal request id, got '{s}': {e}")))
}
