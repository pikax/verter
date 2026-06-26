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

use prost::Message;
use verter_audit::{AuditCaptureState, RequestAuditRecord};
use verter_protocol::typeinfo::graph::{TypeInfoGraphRequest, TypeInfoGraphResponse};
use verter_protocol::typeinfo::FfiSymbolEntry;
use verter_session::host_resolve_type_audit::TypeResolutionRequestError;
use verter_session::semantic_query::{ProjectionMode, SemanticNodeId};
use verter_type_expr::TypeExpr;

/// Split a resolve / evaluate outcome into the resolved node (if any)
/// and an optional dispatch-fault description.
///
/// - `Ok(Some(node))` → `(Some(node), None)`.
/// - `Ok(None)` (non-fault miss) → `(None, None)`.
/// - `Err(fault)` (dispatch fault) → `(None, Some(description))`.
///
/// This is the FFI projection of the carrier's `Err` arm: a genuine
/// dispatch fault is surfaced through the result DTO's `error` channel
/// instead of being silently erased to a `None` node.
pub(crate) fn split_resolve_outcome(
    outcome: std::result::Result<Option<SemanticNodeId>, TypeResolutionRequestError>,
) -> (Option<SemanticNodeId>, Option<String>) {
    match outcome {
        Ok(node) => (node, None),
        Err(fault) => (None, Some(format!("{fault:?}"))),
    }
}

/// Combined response shape for `resolveSymbolWithAudit` /
/// `evaluateTypeExpressionWithAudit`. Both JSON Buffers — the consumer
/// decodes whichever it needs.
///
/// `typeExpr` carries a serde-JSON `TypeExpr` payload. `null` when
/// resolution produced no node — either a non-fault miss (`Ok(None)`)
/// or a dispatch fault (in which case `error` is set).
/// `auditRecord` carries the per-request `RequestAuditRecord`. `null`
/// when audit is disabled (the resolution still ran).
/// `error` carries a human-readable description of a genuine dispatch
/// fault (`BudgetExceeded` / `UnstableState` / `AliasCycle` /
/// `UnsupportedIntrinsic` / `Other`). `null` on success or a non-fault
/// miss — distinguishing "resolved nothing because the request was
/// well-formed but empty" from "resolution faulted".
#[napi(object)]
pub struct NapiTypeInfoResolveResult {
    /// JSON-serialised `TypeExpr` Buffer; `null` when the resolution
    /// could not produce one.
    pub typeExpr: Option<Buffer>,
    /// JSON-serialised `RequestAuditRecord` Buffer; `null` when audit
    /// is off.
    pub auditRecord: Option<Buffer>,
    /// Human-readable dispatch-fault description; `null` on success or
    /// a non-fault miss.
    pub error: Option<String>,
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

/// Encode a record into the optional `auditRecord` DTO slot, projecting
/// the host carrier's mandatory record through the historical "null
/// when audit is disabled / filtered" contract: only an
/// [`AuditCaptureState::ActiveStored`] record encodes to a `Buffer`; a
/// filtered or disabled record projects to `None`.
pub(crate) fn encode_stored_audit_record(rec: &RequestAuditRecord) -> Result<Option<Buffer>> {
    match rec.capture_state {
        AuditCaptureState::ActiveStored => encode_audit_record(rec).map(Some),
        AuditCaptureState::FilteredNoop | AuditCaptureState::AuditDisabled => Ok(None),
    }
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

/// Combined response shape for `resolveFrameworkSurfaceWithAudit`.
///
/// `response` is the protobuf-encoded `TypeInfoGraphResponse` — the
/// `framework_surface` arm on success, the `error` arm on a typed
/// rejection. It is ALWAYS present (the validation-first executor always
/// produces a typed response). `auditRecord` carries the per-request
/// `RequestAuditRecord` as a JSON Buffer; `null` when audit is disabled
/// or the record was filtered.
#[napi(object)]
pub struct NapiFrameworkSurfaceResult {
    /// Protobuf-encoded `TypeInfoGraphResponse` Buffer (always present).
    pub response: Buffer,
    /// JSON-serialised `RequestAuditRecord` Buffer; `null` when audit is
    /// off / filtered.
    pub auditRecord: Option<Buffer>,
}

/// Decode a `Buffer` carrying a protobuf-encoded
/// [`TypeInfoGraphRequest`] envelope into the host wire type.
///
/// The framework-surface operation rides the existing graph envelope;
/// this is the binding-side decode that hands the validated request to
/// the host's validation-first executor.
pub(crate) fn decode_type_info_graph_request(buf: Buffer) -> Result<TypeInfoGraphRequest> {
    TypeInfoGraphRequest::decode(buf.as_ref()).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("type-info graph request decode error: {e}"),
        )
    })
}

/// Encode a [`TypeInfoGraphResponse`] to a protobuf Buffer.
pub(crate) fn encode_type_info_graph_response(resp: &TypeInfoGraphResponse) -> Buffer {
    Buffer::from(resp.encode_to_vec())
}

/// Wrap a typed [`TypeInfoRequestError`] back into the `error` arm of a
/// [`TypeInfoGraphResponse`].
///
/// The host's `AuditedResult` Err arm carries only the typed error (the
/// error-arm response it built is dropped on the carrier). The binding
/// re-forms the wire response so the JS side always decodes a uniform
/// `TypeInfoGraphResponse` regardless of success / rejection.
pub(crate) fn framework_error_response(
    error: verter_protocol::typeinfo::graph::TypeInfoRequestError,
) -> TypeInfoGraphResponse {
    use verter_protocol::verter::v1::type_info_graph_response;
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::Error(error)),
    }
}
