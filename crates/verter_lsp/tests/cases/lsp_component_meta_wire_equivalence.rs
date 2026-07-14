//! `$/verter/getComponentMeta` WIRE equivalence across three wire stages
//! driven IN-PROCESS: the ACTUAL LSP custom method (JSON), the ACTUAL
//! `verter_ffi::convert::component_meta_output_to_ffi` projection of the
//! same audited entry, and the ACTUAL `verter_protocol` payload encoder
//! (the byte route the NAPI/WASM bindings share — the bindings THEMSELVES
//! are not executed here; their thin `Buffer`/`JsValue` mapping is
//! binding-level code outside this harness).
//!
//! Compares DECODED wire DTOs (the LSP JSON against the ffi projection's
//! JSON) and ENCODED BYTES (the protocol encoding of two independent
//! drives of the audited entry — cold and warm — must be byte-identical),
//! so a divergence anywhere in the handler mapping, the converter, or the
//! encoder fails here — not merely in a host-producer `Debug` comparison.

use std::sync::Arc;

use tower_lsp_server::LspService;
use verter_lsp::server::{GetComponentMetaParams, VerterLanguageServer};
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};
use verter_session::{HostConfig, VerterHost};

fn build_test_server(host: Arc<VerterHost>) -> LspService<VerterLanguageServer> {
    let host_for_server = Arc::clone(&host);
    let (service, _socket) = LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: None,
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });
    service
}

#[tokio::test]
async fn lsp_json_ffi_projection_and_protocol_bytes_are_equivalent() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/WireEq.vue".to_string()),
        input_id: "/WireEq.vue".to_string(),
        source: r#"<script setup lang="ts">
type Named = { x: number }
defineProps<{ label: string; named: Named }>()
defineEmits<{ change: [next: number] }>()
</script>
<template><div /></template>"#
            .into(),
        file_language: verter_session::FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .unwrap();
    let service = build_test_server(Arc::clone(&host));

    // (1) The ACTUAL LSP custom method — the JSON wire DTO.
    let lsp_json = service
        .inner()
        .get_component_meta(GetComponentMetaParams {
            uri: "file:///WireEq.vue".to_string(),
        })
        .await
        .expect("handler ok")
        .expect("component resolves");

    // (2) The ACTUAL converter over the SAME audited entry (a warm drive of
    // the identical route the handler used).
    let (output, _request_id) = {
        let (output, request_id) = host
            .get_component_meta_output_with_resolution("/WireEq.vue")
            .expect("audited output ok");
        (output.expect("resolves"), request_id)
    };
    let ffi = verter_ffi::convert::component_meta_output_to_ffi(output);
    let ffi_json = serde_json::to_value(&ffi).expect("ffi serializes");
    assert_eq!(
        lsp_json, ffi_json,
        "the LSP wire JSON must equal the ffi projection of the same \
         audited entry at the DECODED-DTO level (the D19 equivalence, \
         compared on the real wire shapes — not producer Debug strings)"
    );
    // The payload is REAL (a blank envelope would trivially compare equal).
    assert_eq!(
        lsp_json
            .get("props")
            .and_then(|p| p.as_array())
            .map(|p| p.len()),
        Some(2),
        "fixture premise: the wire payload carries the two props; got {lsp_json:?}"
    );

    // (3) The ACTUAL protocol encoder (the NAPI/WASM byte route): two
    // independent drives of the audited entry encode byte-identically.
    let bytes_first = verter_protocol::component_meta::encode_component_meta_payload(&ffi);
    let (output_again, _request_id) = {
        let (output, request_id) = host
            .get_component_meta_output_with_resolution("/WireEq.vue")
            .expect("audited output ok");
        (output.expect("resolves"), request_id)
    };
    let ffi_again = verter_ffi::convert::component_meta_output_to_ffi(output_again);
    let bytes_second = verter_protocol::component_meta::encode_component_meta_payload(&ffi_again);
    assert!(
        !bytes_first.is_empty(),
        "the encoded payload carries real content"
    );
    assert_eq!(
        bytes_first, bytes_second,
        "the NAPI/WASM protocol encoding of the audited envelope must be \
         byte-identical across drives (warm/cold lanes serve one envelope)"
    );
}
