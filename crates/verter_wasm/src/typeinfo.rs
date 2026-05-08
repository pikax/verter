//! Typed WASM helpers for the typeinfo host substrate (mirror of
//! `crate::audit`).
//!
//! WASM consumers exchange JSON strings rather than `Buffer` blobs (the
//! existing project convention — `serde_wasm_bindgen` performs the
//! `JsValue` ↔ string boundary). The Rust-side substrate is identical
//! to NAPI; only the encoding shape changes.

use serde::Serialize;
use verter_audit::RequestAuditRecord;
use verter_protocol::typeinfo::{FfiEvaluateTypeExpressionRequest, FfiSymbolEntry};
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_session::semantic_query::ProjectionMode;
use wasm_bindgen::prelude::*;

/// Combined response shape for `resolveSymbolWithAudit` /
/// `evaluateTypeExpressionWithAudit`. Both fields are JSON strings;
/// the consumer parses whichever it needs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WasmTypeInfoResolveResult {
    /// JSON-serialised `TypeExpr`; `null` when the resolution could not
    /// produce one.
    pub type_expr: Option<String>,
    /// JSON-serialised `RequestAuditRecord`; `null` when audit is off.
    pub audit_record: Option<String>,
}

/// Encode a list of `FfiSymbolEntry` to a JSON string.
pub(crate) fn encode_symbol_list(entries: &[FfiSymbolEntry]) -> Result<String, JsValue> {
    serde_json::to_string(entries)
        .map_err(|e| JsValue::from_str(&format!("symbol list serialization error: {e}")))
}

/// Encode a `TypeExpr` to a JSON string.
pub(crate) fn encode_type_expr(expr: &TypeExpr) -> Result<String, JsValue> {
    serde_json::to_string(expr)
        .map_err(|e| JsValue::from_str(&format!("type-expr serialization error: {e}")))
}

/// Encode a `RequestAuditRecord` to a JSON string.
pub(crate) fn encode_audit_record(rec: &RequestAuditRecord) -> Result<String, JsValue> {
    serde_json::to_string(rec)
        .map_err(|e| JsValue::from_str(&format!("audit record serialization error: {e}")))
}

/// Decode a JSON string array of `TypeExpr` values into the host slice
/// shape used by `resolve_named_symbol_with_audit`.
pub(crate) fn decode_type_expr_list(json: Option<String>) -> Result<Vec<TypeExpr>, JsValue> {
    let Some(s) = json else {
        return Ok(Vec::new());
    };
    if s.is_empty() || s == "null" {
        return Ok(Vec::new());
    }
    serde_json::from_str(&s)
        .map_err(|e| JsValue::from_str(&format!("type-expr list decode error: {e}")))
}

/// Decode an `EvaluateTypeExpressionRequest` JSON string.
pub(crate) fn decode_evaluate_request(
    json: &str,
) -> Result<verter_session::typeinfo::types::EvaluateTypeExpressionRequest, JsValue> {
    let ffi: FfiEvaluateTypeExpressionRequest = serde_json::from_str(json)
        .map_err(|e| JsValue::from_str(&format!("evaluate-request decode error: {e}")))?;
    verter_ffi::convert::ffi_to_host_evaluate_request(ffi)
        .map_err(|e| JsValue::from_str(&format!("evaluate-request lowering error: {e}")))
}

/// Convert the FFI mode tag to the host enum, with `None` selecting the
/// host default.
pub(crate) fn parse_resolve_mode(mode: Option<String>) -> Result<Option<ProjectionMode>, JsValue> {
    let Some(tag) = mode else {
        return Ok(None);
    };
    let parsed = verter_ffi::convert::parse_projection_mode(&tag)
        .map_err(|e| JsValue::from_str(&format!("resolve-symbol mode error: {e}")))?;
    Ok(Some(parsed))
}
