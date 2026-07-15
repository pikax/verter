//! Custom LSP handlers for the selective component-meta API (D32 / D102 /
//! D104 / D113).
//!
//! Three custom methods land here:
//! - `$/verter/getComponentMeta` — full Volar-shape payload via the host's
//!   warm-cache fast path. Returns `null` when the canonical does not
//!   resolve to a component. Wire shape: JSON projection of
//!   [`verter_ffi`]'s `FfiComponentMeta` (D19 byte-equivalence with NAPI
//!   / WASM consumers).
//! - `$/verter/getComponentMetaSurface` — selective surface envelope as
//!   protobuf-encoded bytes (D102 wire format). Returns the bytes as a
//!   JSON `Vec<u8>` (serde-default array encoding). `null` when the
//!   canonical does not resolve to a component.
//! - `$/verter/getComponentMetaTypeExpansion` — one-layer `TypeHandle` →
//!   `TypeExpansion` resolution. Decodes the handle from
//!   `params.handle_bytes`, returns the expansion bytes plus an optional
//!   structured `TypeHandleErrorPayload` (D104 + D114).
//!
//! All three delegate to host-level entry points on
//! [`verter_session::VerterHost`]: `get_component_meta_with_resolution`,
//! `get_component_meta_surface`, and `get_component_meta_type_expansion`.
//! The host-level wrappers themselves delegate to the same pure-logic
//! free functions that `MetaSession` uses
//! (`assemble_surface_from_analysis`, `resolve_type_expansion`), keeping
//! a single shared substrate per the "shared optimized codebase"
//! architecture rule.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::Uri;

use verter_session::component_meta_payload::{TypeHandle, TypeHandleError};

use crate::documents::uri_to_canonical_id;

use super::super::protocol_types::{
    GetComponentMetaParams, GetComponentMetaSurfaceParams, GetComponentMetaTypeExpansionParams,
    GetComponentMetaTypeExpansionResponse, TypeHandleErrorPayload,
};
use super::super::VerterLanguageServer;

impl VerterLanguageServer {
    /// Handle `$/verter/getComponentMeta` request — full Volar-shape payload.
    ///
    /// Internally calls the audited
    /// `VerterHost::get_component_meta_output_with_resolution`, which
    /// consults the warm `ComponentMetaResultDb` cache before falling
    /// through to the cold resolver and materializes every wire type lane
    /// inside the request-bound validated view. The envelope is projected
    /// through `verter_ffi::convert::component_meta_output_to_ffi` so the
    /// JSON wire shape matches NAPI/WASM consumers (D19 byte-equivalence).
    /// Returns `null` when the canonical does not resolve to a component.
    pub async fn get_component_meta(
        &self,
        params: GetComponentMetaParams,
    ) -> Result<Option<serde_json::Value>> {
        tracing::debug!("$/verter/getComponentMeta: {}", params.uri);

        // `null` is reserved EXCLUSIVELY for "the canonical does not
        // resolve to a component". A malformed URI is a REQUEST fault —
        // JSON-RPC `InvalidParams` — never the absence result.
        let parsed_uri: Uri = match params.uri.parse() {
            Ok(u) => u,
            Err(_) => return Err(invalid_uri_error(&params.uri)),
        };
        let canonical_id = match self.documents.get_canonical_id(&parsed_uri) {
            Some(id) => id,
            None => uri_to_canonical_id(&parsed_uri),
        };
        self.documents.host().ensure_loaded(&canonical_id);

        let host = self.documents.host();
        // Output-bearing AUDITED entry: the session host materializes every
        // wire type lane inside the request-bound validated view and hands
        // back the context-free envelope (with the resolution sidecar, so
        // the LSP payload matches the NAPI/WASM audited surfaces). A typed
        // output-materialization failure refuses the payload (fail-closed —
        // never a silent `Unknown` on the wire) and surfaces as a JSON-RPC
        // ERROR carrying the failed lane/index — `null` is reserved
        // EXCLUSIVELY for "the canonical does not resolve to a component";
        // a real failure must never be reported as absence.
        let output = match host.get_component_meta_output_with_resolution(&canonical_id) {
            Ok((Some(output), _request_id)) => output,
            Ok((None, _request_id)) => return Ok(None),
            Err((err, _request_id)) => return Err(component_meta_output_error(&err)),
        };

        let ffi = verter_ffi::convert::component_meta_output_to_ffi(output);
        // Serialization failure is a genuine INTERNAL error — propagated,
        // never folded onto the `null` absence result via `.ok()`.
        match serde_json::to_value(&ffi) {
            Ok(value) => Ok(Some(value)),
            Err(err) => Err(tower_lsp_server::jsonrpc::Error {
                code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
                message: std::borrow::Cow::Owned(format!(
                    "getComponentMeta: payload serialization failed: {err}"
                )),
                data: None,
            }),
        }
    }

    /// Handle `$/verter/getComponentMetaSurface` request — selective surface
    /// envelope as protobuf-encoded bytes.
    ///
    /// Wire shape: JSON `Vec<u8>` (serde-default array encoding). The bytes
    /// are produced by `ComponentMetaSurface::to_proto_bytes()` (D100), so
    /// the JS consumer can decode them via the same proto generated for
    /// the NAPI / WASM bindings. Returns `null` when the canonical does
    /// not resolve to a component.
    pub async fn get_component_meta_surface(
        &self,
        params: GetComponentMetaSurfaceParams,
    ) -> Result<Option<Vec<u8>>> {
        tracing::debug!("$/verter/getComponentMetaSurface: {}", params.uri);

        // A malformed URI is a REQUEST fault — `InvalidParams`; `null`
        // stays reserved for genuine component absence (same rule as
        // `$/verter/getComponentMeta`).
        let parsed_uri: Uri = match params.uri.parse() {
            Ok(u) => u,
            Err(_) => return Err(invalid_uri_error(&params.uri)),
        };
        let canonical_id = match self.documents.get_canonical_id(&parsed_uri) {
            Some(id) => id,
            None => uri_to_canonical_id(&parsed_uri),
        };
        self.documents.host().ensure_loaded(&canonical_id);

        let host = self.documents.host();
        Ok(host
            .get_component_meta_surface(&canonical_id)
            .map(|s| s.to_proto_bytes()))
    }

    /// Handle `$/verter/getComponentMetaTypeExpansion` request — one-layer
    /// type-handle expansion.
    ///
    /// Decodes the protobuf-encoded `TypeHandle` from `params.handle_bytes`,
    /// resolves it to a `TypeExpansion` via
    /// `VerterHost::get_component_meta_type_expansion`, and re-encodes the
    /// result. Errors are projected to a structured `TypeHandleErrorPayload`
    /// (D104 + D114) — the `error` field on the response carries the
    /// discriminated kind plus reason metadata. On success, `error` is
    /// `None` and `expansion_bytes` carries the encoded `TypeExpansion`.
    pub async fn get_component_meta_type_expansion(
        &self,
        params: GetComponentMetaTypeExpansionParams,
    ) -> Result<GetComponentMetaTypeExpansionResponse> {
        tracing::debug!(
            "$/verter/getComponentMetaTypeExpansion: handle_bytes_len={}",
            params.handle_bytes.len()
        );

        let handle = match TypeHandle::from_proto_bytes(&params.handle_bytes) {
            Ok(h) => h,
            Err(e) => {
                return Ok(GetComponentMetaTypeExpansionResponse {
                    expansion_bytes: Vec::new(),
                    error: Some(TypeHandleErrorPayload::Other {
                        message: format!("decode TypeHandle: {e}"),
                    }),
                });
            }
        };

        let host = self.documents.host();
        let depth = params.depth.map(|d| d as usize);
        match host.get_component_meta_type_expansion(handle, depth) {
            Ok(expansion) => Ok(GetComponentMetaTypeExpansionResponse {
                expansion_bytes: expansion.to_proto_bytes(),
                error: None,
            }),
            Err(err) => Ok(GetComponentMetaTypeExpansionResponse {
                expansion_bytes: Vec::new(),
                error: Some(type_handle_error_to_payload(err)),
            }),
        }
    }
}

/// JSON-RPC `InvalidParams` for a request URI that does not parse — a
/// request fault, never the `null` absence result.
fn invalid_uri_error(uri: &str) -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::InvalidParams,
        message: std::borrow::Cow::Owned(format!("invalid uri: {uri}")),
        data: None,
    }
}

/// Build the structured JSON-RPC error for a typed component-meta
/// output-materialization failure: the failed lane path, positional
/// indices, and the failure class ride the `data` payload so a client can
/// distinguish a real materialization failure from the `null`
/// "component does not resolve" absence result.
fn component_meta_output_error(
    err: &verter_session::meta_resolve::ComponentMetaOutputError,
) -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
        message: std::borrow::Cow::Owned(format!("getComponentMeta: {err}")),
        data: Some(serde_json::json!({
            "lane": err.lane.path(),
            "index": err.index,
            "innerIndex": err.inner_index,
            "failure": format!("{:?}", err.failure),
        })),
    }
}

fn type_handle_error_to_payload(err: TypeHandleError) -> TypeHandleErrorPayload {
    match err {
        TypeHandleError::ProjectMismatch { expected, actual } => {
            TypeHandleErrorPayload::ProjectMismatch { expected, actual }
        }
        TypeHandleError::StaleHandle { reason } => TypeHandleErrorPayload::StaleHandle {
            reason: format!("{reason:?}"),
        },
        TypeHandleError::EvictedNode { handle } => TypeHandleErrorPayload::Other {
            message: format!("evicted node: handle={handle:?}"),
        },
    }
}
