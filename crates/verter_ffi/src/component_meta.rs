//! Shared component-meta payload and audit-bundle encodings.

use serde::Serialize;
use verter_audit::RequestAuditRecord;
use verter_protocol::types::{FfiComponentMeta, FfiComponentMetaResolution};

/// Consume the session-owned materialized envelope and encode its FFI projection.
pub fn encode_component_meta_payload(
    output: verter_session::meta_resolve::ComponentMetaOutput,
) -> Vec<u8> {
    let ffi = crate::convert::component_meta_output_to_ffi(output);
    verter_protocol::component_meta::encode_component_meta_payload(&ffi)
}

/// Audited component metadata; record integers retain their decimal-string encoding.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditBundle {
    pub analysis: FfiComponentMeta,
    pub resolution: FfiComponentMetaResolution,
    pub record: RequestAuditRecord,
}

/// Encode the audit bundle as UTF-8 JSON bytes.
pub fn encode_audit_bundle(bundle: &AuditBundle) -> Result<Vec<u8>, String> {
    serde_json::to_vec(bundle).map_err(|e| format!("audit bundle serialization error: {e}"))
}
