//! Regression pin: `$/verter/documentStructure` must never hold a
//! `DocumentRegistry` shard guard across an await point.
//!
//! [`crate::documents::DocumentRegistry::get`] hands out a LIVE DashMap shard
//! READ guard; `did_change` takes the WRITE side of the same shard through a
//! blocking `get_mut`. The production stdio loop polls the server inline on a
//! single task, so a read guard held across the handler's `yield_now().await`
//! parks the whole server: the write side blocks the only worker thread while
//! the guard can only be released by a future that thread will never poll
//! again.
//!
//! The test drives exactly that interleaving on a current-thread runtime:
//! `tokio::join!` polls `document_structure` first (it suspends at its await
//! point), then runs the write-side `did_change` on the same thread. With a
//! guard held across the await this deadlocks and the watchdog fails the
//! test; with the guards dropped before the await both futures complete and
//! the post-await fence reports `Superseded` for the concurrent edit.

use std::sync::Arc;

use tower_lsp_server::ls_types::{TextDocumentIdentifier, TextDocumentItem, Uri};

use super::super::protocol_types::{DocumentStructureRequestV1, DocumentStructureResponseV1};
use super::super::VerterLanguageServer;

fn build_service() -> tower_lsp_server::LspService<VerterLanguageServer> {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            crate::LspConfig {
                host: Arc::clone(&host),
                type_provider: None,
                type_provider_topology: crate::TypeProviderTopology::None,
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    service
}

#[test]
fn document_structure_holds_no_shard_guard_across_await() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("document-structure-guard-probe".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("current-thread runtime");
            let outcome = runtime.block_on(async {
                let service = build_service();
                let server = service.inner();
                let uri: Uri = "file:///workspace/GuardProbe.vue".parse().unwrap();
                let _ = server.documents.did_open(&TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".to_string(),
                    version: 1,
                    text: "<template><div>guard</div></template>".to_string(),
                });
                assert!(
                    server.documents.feature_snapshot(&uri).is_some(),
                    "carrier snapshot must exist so the handler reaches its await point"
                );
                let request = DocumentStructureRequestV1 {
                    request_token: "t-1".to_string(),
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    client_open_epoch: "e-1".to_string(),
                    expected_client_version: 1,
                };
                // `join!` polls in order: the handler runs to its await point
                // first, then the second future performs the WRITE-side edit on
                // the SAME thread. A shard guard held across the await
                // deadlocks right here.
                let (response, ()) = tokio::join!(server.document_structure(request), async {
                    let _ = server.documents.did_change(
                        &uri,
                        2,
                        "<template><div>edited</div></template>",
                    );
                });
                response.expect("document_structure response")
            });
            tx.send(outcome).ok();
        })
        .expect("spawn probe thread");

    let response = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .unwrap_or_else(|_| {
            panic!(
                "deadlock: document_structure held a DocumentRegistry shard guard \
                 across its await while did_change took the write side of the same shard"
            )
        });
    assert!(
        matches!(response, DocumentStructureResponseV1::Superseded { .. }),
        "post-await fence must observe the concurrent edit as Superseded"
    );
}
