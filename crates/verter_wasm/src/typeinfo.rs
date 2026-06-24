//! Typed WASM helpers for the typeinfo host substrate (mirror of
//! `crate::audit`).
//!
//! WASM consumers exchange JSON strings rather than `Buffer` blobs (the
//! existing project convention — `serde_wasm_bindgen` performs the
//! `JsValue` ↔ string boundary). The Rust-side substrate is identical
//! to NAPI; only the encoding shape changes.

use prost::Message;
use serde::Serialize;
use verter_audit::RequestAuditRecord;
use verter_protocol::typeinfo::graph::{TypeInfoGraphRequest, TypeInfoGraphResponse};
use verter_protocol::typeinfo::{FfiEvaluateTypeExpressionRequest, FfiSymbolEntry};
use verter_session::host_resolve_type_audit::TypeResolutionRequestError;
use verter_session::semantic_query::{ProjectionMode, SemanticNodeId};
use verter_type_expr::TypeExpr;
use wasm_bindgen::prelude::*;

/// Combined response shape for `resolveSymbolWithAudit` /
/// `evaluateTypeExpressionWithAudit`. All fields are JSON strings;
/// the consumer parses whichever it needs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WasmTypeInfoResolveResult {
    /// JSON-serialised `TypeExpr`; `null` when the resolution could not
    /// produce one (a non-fault miss, or a dispatch fault — in which
    /// case `error` is set).
    pub type_expr: Option<String>,
    /// JSON-serialised `RequestAuditRecord`; `null` when audit is off.
    pub audit_record: Option<String>,
    /// Human-readable dispatch-fault description; `null` on success or
    /// a non-fault miss.
    pub error: Option<String>,
}

/// Split a resolve / evaluate outcome into the resolved node (if any)
/// and an optional dispatch-fault description. Mirrors
/// `crate::typeinfo` on the NAPI side: a genuine dispatch fault is
/// surfaced through the result DTO's `error` channel instead of being
/// silently erased to a `None` node.
pub(crate) fn split_resolve_outcome(
    outcome: Result<Option<SemanticNodeId>, TypeResolutionRequestError>,
) -> (Option<SemanticNodeId>, Option<String>) {
    match outcome {
        Ok(node) => (node, None),
        Err(fault) => (None, Some(format!("{fault:?}"))),
    }
}

/// Encode a list of `FfiSymbolEntry` to a JSON string.
pub(crate) fn encode_symbol_list(entries: &[FfiSymbolEntry]) -> Result<String, JsValue> {
    serde_json::to_string(entries)
        .map_err(|e| JsValue::from_str(&format!("symbol list serialization error: {e}")))
}

/// Encode a record into the optional `auditRecord` DTO slot,
/// projecting the host carrier's mandatory record through the
/// historical "null when audit is disabled / filtered" contract: only
/// an [`verter_audit::AuditCaptureState::ActiveStored`] record encodes
/// to a JSON string; a filtered or disabled record projects to `None`.
pub(crate) fn encode_stored_audit_record(
    rec: &RequestAuditRecord,
) -> Result<Option<String>, JsValue> {
    match rec.capture_state {
        verter_audit::AuditCaptureState::ActiveStored => encode_audit_record(rec).map(Some),
        verter_audit::AuditCaptureState::FilteredNoop
        | verter_audit::AuditCaptureState::AuditDisabled => Ok(None),
    }
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

/// Combined response shape for `resolveFrameworkSurfaceWithAudit` (WASM).
///
/// `response` is the protobuf-encoded `TypeInfoGraphResponse` (always
/// present — the validation-first executor always yields a typed
/// response). `audit_record` carries the per-request `RequestAuditRecord`
/// JSON string; `null` when audit is off / filtered.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WasmFrameworkSurfaceResult {
    /// Protobuf-encoded `TypeInfoGraphResponse` bytes (always present).
    /// `serde-wasm-bindgen` maps the byte slice to a JS array of numbers;
    /// the consumer reconstructs a `Uint8Array` from it before decoding.
    pub response: Vec<u8>,
    /// JSON-serialised `RequestAuditRecord`; `null` when audit is off.
    pub audit_record: Option<String>,
}

/// Decode a protobuf-encoded [`TypeInfoGraphRequest`] envelope from
/// raw bytes.
pub(crate) fn decode_type_info_graph_request(
    bytes: &[u8],
) -> Result<TypeInfoGraphRequest, JsValue> {
    TypeInfoGraphRequest::decode(bytes)
        .map_err(|e| JsValue::from_str(&format!("type-info graph request decode error: {e}")))
}

/// Encode a [`TypeInfoGraphResponse`] to protobuf bytes.
pub(crate) fn encode_type_info_graph_response(resp: &TypeInfoGraphResponse) -> Vec<u8> {
    resp.encode_to_vec()
}

/// Wrap a typed `TypeInfoRequestError` back into the `error` arm of a
/// [`TypeInfoGraphResponse`] (parity with the NAPI binding — the
/// `AuditedResult` Err arm drops the error-arm response).
pub(crate) fn framework_error_response(
    error: verter_protocol::typeinfo::graph::TypeInfoRequestError,
) -> TypeInfoGraphResponse {
    use verter_protocol::verter::v1::type_info_graph_response;
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::Error(error)),
    }
}
