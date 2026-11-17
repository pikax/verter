//! Shared component-meta payload and audit-bundle encodings.

use serde::Serialize;
use verter_audit::RequestAuditRecord;
use verter_protocol::types::FfiComponentMeta;

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
    pub record: RequestAuditRecord,
}

/// Encode the audit bundle as UTF-8 JSON bytes.
pub fn encode_audit_bundle(bundle: &AuditBundle) -> Result<Vec<u8>, String> {
    serde_json::to_vec(bundle).map_err(|e| format!("audit bundle serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_audit_bundle_contains_analysis_and_record() {
        let host = verter_session::VerterHost::new_standalone(Default::default());
        let _ = host
            .upsert(verter_session::UpsertRequest {
                canonical_id: Some("/Flag.vue".into()),
                input_id: "/Flag.vue".into(),
                source: std::sync::Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ on: boolean }>()</script>",
                ),
                file_language: verter_session::FileLanguage::vue(),
                aliases: vec![],
            })
            .unwrap();
        let (output, _) = host
            .get_component_meta_output_with_resolution("/Flag.vue")
            .unwrap();
        let record = host.audit_workspace_op(verter_audit::WorkspaceOp::DepGraphTraverse {
            root: "/Flag.vue".into(),
        });
        let bundle = AuditBundle {
            analysis: crate::convert::component_meta_output_to_ffi(output.unwrap()),
            record,
        };
        let json: serde_json::Value =
            serde_json::from_slice(&encode_audit_bundle(&bundle).unwrap()).unwrap();
        assert_eq!(
            json.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["analysis", "record"]
        );
        assert!(json["analysis"]["resolution"].is_object());
    }
}
