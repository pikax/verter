//! Custom LSP handler for full component metadata.
//!
//! The host materializes the payload under its validated request view;
//! the shared FFI converter projects it to the JSON response.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::Uri;

use crate::documents::uri_to_canonical_id;

use super::super::protocol_types::GetComponentMetaParams;
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
    pub(super) async fn get_component_meta_admitted(
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
            Err((verter_session::meta_resolve::ComponentMetaFailure::Output(err), _request_id)) => {
                return Err(component_meta_output_error(&err))
            }
            // An aborted computation publishes nothing: the request is
            // answered as cancelled, or as outdated by a newer view.
            Err((
                verter_session::meta_resolve::ComponentMetaFailure::Aborted(abort),
                _request_id,
            )) => {
                return Err(component_meta_aborted_error(
                    abort.is_superseded(),
                    format!("{abort:?}"),
                ))
            }
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
fn component_meta_aborted_error(
    superseded: bool,
    abort: String,
) -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: if superseded {
            tower_lsp_server::jsonrpc::ErrorCode::ContentModified
        } else {
            tower_lsp_server::jsonrpc::ErrorCode::RequestCancelled
        },
        message: std::borrow::Cow::Owned(format!("getComponentMeta: aborted ({abort})")),
        data: None,
    }
}

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
