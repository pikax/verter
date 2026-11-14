//! NAPI result types and error mapping for the shared typeinfo adapter.

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
pub(crate) use shared::{project_resolved_root, split_resolve_outcome};
use verter_ffi::typeinfo as shared;
use verter_protocol::typeinfo::graph::{
    TypeInfoCapabilityHandshakeRequest, TypeInfoCapabilityHandshakeResponse, TypeInfoGraphRequest,
};
use verter_type_engine::semantic_query::ProjectionMode;
use verter_type_expr::TypeExpr;

/// Response shape for `resolveSymbolWithAudit`: JSON Buffers.
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

#[napi(object)]
pub struct NapiFrameworkSurfaceResult {
    /// Protobuf-encoded `TypeInfoGraphResponse` Buffer (always present).
    pub response: Buffer,
    /// JSON-serialised `RequestAuditRecord` Buffer; `null` when audit is
    /// off / filtered.
    pub auditRecord: Option<Buffer>,
}

pub(crate) fn decode_type_expr_list(buf: Option<Buffer>) -> Result<Vec<TypeExpr>> {
    shared::decode_type_expr_list(buf.as_deref()).map_err(|e| Error::new(Status::InvalidArg, e))
}

pub(crate) fn parse_resolve_mode(mode: Option<String>) -> Result<Option<ProjectionMode>> {
    shared::parse_resolve_mode(mode.as_deref()).map_err(|e| Error::new(Status::InvalidArg, e))
}

pub(crate) fn encode_stored_audit_record(
    rec: &verter_audit::RequestAuditRecord,
) -> Result<Option<Buffer>> {
    shared::encode_stored_audit_record(rec)
        .map(|json| json.map(|json| Buffer::from(json.into_bytes())))
        .map_err(|e| Error::new(Status::GenericFailure, e))
}

pub(crate) fn decode_type_info_graph_request(buf: Buffer) -> Result<TypeInfoGraphRequest> {
    shared::decode_type_info_graph_request(&buf).map_err(|e| Error::new(Status::InvalidArg, e))
}

pub(crate) fn decode_capability_handshake_request(
    buf: Buffer,
) -> Result<TypeInfoCapabilityHandshakeRequest> {
    shared::decode_capability_handshake_request(&buf).map_err(|e| Error::new(Status::InvalidArg, e))
}

pub(crate) fn encode_capability_handshake_response(
    resp: &TypeInfoCapabilityHandshakeResponse,
) -> Buffer {
    Buffer::from(shared::encode_capability_handshake_response(resp))
}
