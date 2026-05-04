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
    /// Internally calls `VerterHost::get_component_meta_with_resolution`
    /// which consults the warm `ComponentMetaResultDb` cache before falling
    /// through to the cold resolver. The returned analysis is projected
    /// through `verter_ffi::convert::component_meta_analysis_to_ffi` so the
    /// JSON wire shape matches NAPI/WASM consumers (D19 byte-equivalence).
    /// Returns `null` when the canonical does not resolve to a component.
    pub async fn get_component_meta(
        &self,
        params: GetComponentMetaParams,
    ) -> Result<Option<serde_json::Value>> {
        tracing::debug!("$/verter/getComponentMeta: {}", params.uri);

        let parsed_uri: Uri = match params.uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };
        let canonical_id = match self.documents.get_canonical_id(&parsed_uri) {
            Some(id) => id,
            None => uri_to_canonical_id(&parsed_uri),
        };
        self.documents.host().ensure_loaded(&canonical_id);

        let host = self.documents.host();
        let Some((analysis, _resolution)) = host.get_component_meta_with_resolution(&canonical_id)
        else {
            return Ok(None);
        };

        let ffi = verter_ffi::convert::component_meta_analysis_to_ffi(analysis);
        Ok(serde_json::to_value(&ffi).ok())
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

        let parsed_uri: Uri = match params.uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
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
