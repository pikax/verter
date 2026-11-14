//! Boundary-neutral typeinfo decoding, execution and encoding.

use prost::Message;
use verter_audit::{AuditCaptureState, RequestAuditRecord};
use verter_protocol::typeinfo::graph::{
    TypeInfoCapabilityHandshakeRequest, TypeInfoCapabilityHandshakeResponse, TypeInfoGraphRequest,
    TypeInfoGraphResponse, TypeInfoRequestError,
};
use verter_session::host_resolve_type_audit::TypeResolutionRequestError;
use verter_session::VerterHost;
use verter_type_engine::semantic_query::ProjectionMode;
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;
use verter_type_expr::TypeExpr;

/// Preserve dispatch faults separately from non-fault misses.
pub fn split_resolve_outcome(
    outcome: Result<Option<NodeRoot>, TypeResolutionRequestError>,
) -> (Option<NodeRoot>, Option<String>) {
    match outcome {
        Ok(node) => (node, None),
        Err(fault) => (None, Some(format!("{fault:?}"))),
    }
}

/// Project through the session's output boundary, preserving unsettled-view faults.
pub fn project_resolved_root(
    host: &VerterHost,
    resolved: Option<NodeRoot>,
) -> (Option<Vec<u8>>, Option<String>) {
    let Some(node) = resolved else {
        return (None, None);
    };
    match host.project_node_to_type_expr_json_bytes(&node) {
        Ok(bytes) => (bytes, None),
        Err(unsettled) => (
            None,
            Some(format!("{:?}", TypeResolutionRequestError::from(unsettled))),
        ),
    }
}

/// Only captured, stored records occupy the optional audit response slot.
pub fn encode_stored_audit_record(rec: &RequestAuditRecord) -> Result<Option<String>, String> {
    match rec.capture_state {
        AuditCaptureState::ActiveStored => encode_audit_record(rec).map(Some),
        AuditCaptureState::FilteredNoop
        | AuditCaptureState::AuditDisabled
        | AuditCaptureState::Unavailable => Ok(None),
    }
}

/// Encode a record as UTF-8 JSON, also usable as bytes at native boundaries.
pub fn encode_audit_record(rec: &RequestAuditRecord) -> Result<String, String> {
    serde_json::to_string(rec).map_err(|e| format!("audit record serialization error: {e}"))
}

/// Decode generic arguments; absent or empty input means no instantiation.
pub fn decode_type_expr_list(bytes: Option<&[u8]>) -> Result<Vec<TypeExpr>, String> {
    let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_slice(bytes).map_err(|e| format!("type-expr list decode error: {e}"))
}

/// Parse an optional projection mode, leaving the host default unselected.
pub fn parse_resolve_mode(mode: Option<&str>) -> Result<Option<ProjectionMode>, String> {
    mode.map(|tag| {
        crate::convert::parse_projection_mode(tag)
            .map_err(|e| format!("resolve-symbol mode error: {e}"))
    })
    .transpose()
}

/// Decode the graph envelope before validation-first semantic execution.
pub fn decode_type_info_graph_request(bytes: &[u8]) -> Result<TypeInfoGraphRequest, String> {
    TypeInfoGraphRequest::decode(bytes)
        .map_err(|e| format!("type-info graph request decode error: {e}"))
}

/// Decode the capability handshake.
pub fn decode_capability_handshake_request(
    bytes: &[u8],
) -> Result<TypeInfoCapabilityHandshakeRequest, String> {
    TypeInfoCapabilityHandshakeRequest::decode(bytes)
        .map_err(|e| format!("capability handshake decode error: {e}"))
}

/// Encode the capability handshake response.
pub fn encode_capability_handshake_response(resp: &TypeInfoCapabilityHandshakeResponse) -> Vec<u8> {
    resp.encode_to_vec()
}

/// Encode a response and attribute its encoded size exactly once.
pub fn encode_type_info_graph_response(resp: &TypeInfoGraphResponse) -> Vec<u8> {
    let encoded = resp.encode_to_vec();
    verter_audit::attribute_n!(TypeInfoGraphEncode, encoded.len());
    encoded
}

/// Restore the uniform wire error arm from an audited request rejection.
pub fn framework_error_response(error: TypeInfoRequestError) -> TypeInfoGraphResponse {
    use verter_protocol::verter::v1::type_info_graph_response;
    TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::Error(error)),
    }
}

/// Execute a decoded request, returning protobuf response bytes and optional audit JSON.
pub fn type_info_with_audit(
    host: &VerterHost,
    request: TypeInfoGraphRequest,
) -> Result<(Vec<u8>, Option<String>), String> {
    let (outcome, record) = host.type_info_with_audit(request).into_parts();
    let response = match outcome {
        Ok(response) => response,
        Err(error) => framework_error_response(error),
    };
    let response = encode_type_info_graph_response(&response);
    let audit_record = encode_stored_audit_record(&record)?;
    Ok((response, audit_record))
}
