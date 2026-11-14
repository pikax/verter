//! WASM result types and error mapping for the shared typeinfo adapter.

use serde::Serialize;
pub(crate) use shared::{
    encode_capability_handshake_response, project_resolved_root, split_resolve_outcome,
};
use verter_ffi::typeinfo as shared;
use verter_protocol::typeinfo::graph::{TypeInfoCapabilityHandshakeRequest, TypeInfoGraphRequest};
use verter_type_engine::semantic_query::ProjectionMode;
use verter_type_expr::TypeExpr;
use wasm_bindgen::prelude::*;

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

pub(crate) fn decode_type_expr_list(json: Option<String>) -> Result<Vec<TypeExpr>, JsValue> {
    let bytes = json
        .as_deref()
        .filter(|json| *json != "null")
        .map(str::as_bytes);
    shared::decode_type_expr_list(bytes).map_err(|e| JsValue::from_str(&e))
}

pub(crate) fn parse_resolve_mode(mode: Option<String>) -> Result<Option<ProjectionMode>, JsValue> {
    shared::parse_resolve_mode(mode.as_deref()).map_err(|e| JsValue::from_str(&e))
}

pub(crate) fn encode_stored_audit_record(
    rec: &verter_audit::RequestAuditRecord,
) -> Result<Option<String>, JsValue> {
    shared::encode_stored_audit_record(rec).map_err(|e| JsValue::from_str(&e))
}

pub(crate) fn decode_type_info_graph_request(
    bytes: &[u8],
) -> Result<TypeInfoGraphRequest, JsValue> {
    shared::decode_type_info_graph_request(bytes).map_err(|e| JsValue::from_str(&e))
}

pub(crate) fn decode_capability_handshake_request(
    bytes: &[u8],
) -> Result<TypeInfoCapabilityHandshakeRequest, JsValue> {
    shared::decode_capability_handshake_request(bytes).map_err(|e| JsValue::from_str(&e))
}

/// The symbol inventory of `canonical`, answered by the list-symbols
/// operation of the one typeinfo entry, as the browser host's JSON array.
pub(crate) fn list_symbols_json(
    host: &verter_session::VerterHost,
    canonical: &str,
) -> Result<String, JsValue> {
    use verter_protocol::typeinfo::graph as wire;
    use verter_protocol::verter::v1::{type_info_graph_request, type_info_graph_response};
    let request = wire::TypeInfoGraphRequest {
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: wire::Operation::ListSymbols as i32,
        payload: Some(type_info_graph_request::Payload::ListSymbols(
            wire::ListSymbolsRequest {
                canonical_id: canonical.to_string(),
            },
        )),
    };
    let (outcome, _record) = host.type_info_with_audit(request).into_parts();
    let entries = match outcome {
        Ok(wire::TypeInfoGraphResponse {
            kind: Some(type_info_graph_response::Kind::SymbolList(list)),
        }) => list.entries,
        Ok(other) => {
            return Err(JsValue::from_str(&format!(
                "listSymbols: unexpected response {other:?}"
            )))
        }
        Err(error) => {
            return Err(JsValue::from_str(&format!(
                "listSymbols refused: {:?}",
                error.kind
            )))
        }
    };
    let rows: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.name,
                "kind": entry.kind,
                "spanEncoding": "utf8.v1",
                "spanStart": entry.span_start,
                "spanEnd": entry.span_end,
                "hasSpan": entry.has_span,
                "isExported": entry.is_exported,
            })
        })
        .collect();
    serde_json::to_string(&rows)
        .map_err(|e| JsValue::from_str(&format!("symbol list serialization error: {e}")))
}
