//! Typed NAPI helpers for the typeinfo host substrate.
//!
//! Mirror to `crate::audit` — keeps free functions / object types
//! that the inline `impl NapiVerterHost` block in `lib.rs` consumes.
//! The methods themselves MUST live in the lib.rs impl block so the
//! napi-derive class registration picks up the
//! `js_name = "VerterHost"` rename.

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;

use verter_audit::RequestAuditRecord;
use verter_protocol::typeinfo::FfiSymbolEntry;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_session::semantic_query::ProjectionMode;

/// Combined response shape for `resolveSymbolWithAudit` /
/// `evaluateTypeExpressionWithAudit`. Both JSON Buffers — the consumer
/// decodes whichever it needs.
///
/// `typeExpr` carries a serde-JSON `TypeExpr` payload. `null` when
/// resolution failed (e.g. unknown symbol, lowering miss, suppressed).
/// `auditRecord` carries the per-request `RequestAuditRecord`. `null`
/// when audit is disabled (the resolution still ran).
#[napi(object)]
pub struct NapiTypeInfoResolveResult {
    /// JSON-serialised `TypeExpr` Buffer; `null` when the resolution
    /// could not produce one.
    pub typeExpr: Option<Buffer>,
    /// JSON-serialised `RequestAuditRecord` Buffer; `null` when audit
    /// is off.
    pub auditRecord: Option<Buffer>,
}

/// Encode a list of `FfiSymbolEntry` to a JSON Buffer.
pub(crate) fn encode_symbol_list(entries: &[FfiSymbolEntry]) -> Result<Buffer> {
    let bytes = serde_json::to_vec(entries).map_err(|e| {
        Error::new(
            Status::GenericFailure,
            format!("symbol list serialization error: {e}"),
        )
    })?;
    Ok(Buffer::from(bytes))
}

/// Encode a single `TypeExpr` to a JSON Buffer.
pub(crate) fn encode_type_expr(expr: &TypeExpr) -> Result<Buffer> {
    let bytes = serde_json::to_vec(expr).map_err(|e| {
        Error::new(
            Status::GenericFailure,
            format!("type-expr serialization error: {e}"),
        )
    })?;
    Ok(Buffer::from(bytes))
}

/// Encode a `RequestAuditRecord` to a JSON Buffer (parity with
/// `crate::audit::encode_record`).
pub(crate) fn encode_audit_record(rec: &RequestAuditRecord) -> Result<Buffer> {
    let bytes = serde_json::to_vec(rec).map_err(|e| {
        Error::new(
            Status::GenericFailure,
            format!("audit record serialization error: {e}"),
        )
    })?;
    Ok(Buffer::from(bytes))
}

/// Decode a `Buffer` containing a JSON array of `TypeExpr` values
/// into the host slice shape used by `resolve_named_symbol_with_audit`.
///
/// An empty / `None` buffer maps to an empty slice; this is the common
/// "no generic instantiation" case at the JS boundary.
pub(crate) fn decode_type_expr_list(buf: Option<Buffer>) -> Result<Vec<TypeExpr>> {
    let Some(buf) = buf else {
        return Ok(Vec::new());
    };
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let exprs: Vec<TypeExpr> = serde_json::from_slice(&buf).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("type-expr list decode error: {e}"),
        )
    })?;
    Ok(exprs)
}

/// Decode a `Buffer` containing a JSON
/// `verter_protocol::typeinfo::FfiEvaluateTypeExpressionRequest` into
/// the host substrate type.
pub(crate) fn decode_evaluate_request(
    buf: Buffer,
) -> Result<verter_session::typeinfo::types::EvaluateTypeExpressionRequest> {
    use verter_protocol::typeinfo::FfiEvaluateTypeExpressionRequest;
    let ffi: FfiEvaluateTypeExpressionRequest = serde_json::from_slice(&buf).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("evaluate-request decode error: {e}"),
        )
    })?;
    verter_ffi::convert::ffi_to_host_evaluate_request(ffi).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("evaluate-request lowering error: {e}"),
        )
    })
}

/// Convert an FFI/JS-supplied projection-mode string to the host enum.
/// `None` selects the host's default-mode policy (Navigate for generic
/// carriers, Expanded otherwise) — see
/// `verter_session::typeinfo::ResolveMode`.
pub(crate) fn parse_resolve_mode(mode: Option<String>) -> Result<Option<ProjectionMode>> {
    let Some(tag) = mode else {
        return Ok(None);
    };
    let parsed = verter_ffi::convert::parse_projection_mode(&tag).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("resolve-symbol mode error: {e}"),
        )
    })?;
    Ok(Some(parsed))
}
