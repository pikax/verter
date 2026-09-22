//! Render-cadence decorations stay off the type provider while the workspace is
//! being published into it.

use std::sync::Arc;

use tower_lsp_server::ls_types::{
    InlayHintParams, PartialResultParams, Position, Range, SemanticTokensParams,
    TextDocumentIdentifier, WorkDoneProgressParams,
};
use tower_lsp_server::LanguageServer;

use crate::server::nav_features_navigation::nav_features_rename_completeness_tests::fixture_with_provider;
use crate::type_provider::mock::{MockCall, MockTypeProvider};

const SOURCE: &str = "<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>\n";

fn decoration_queries(provider: &MockTypeProvider) -> usize {
    provider
        .calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                MockCall::GetInlayHints { .. } | MockCall::GetSemanticTokens { .. }
            )
        })
        .count()
}

/// The editor asks for decorations for every visible document the moment the
/// server is up. Answering one from an engine that holds only part of the
/// workspace makes a lazy engine build a program it then has to rebuild at many
/// times the cost — so while the workspace is being published the provider is
/// not asked, and once it is whole it is asked again.
#[tokio::test(flavor = "multi_thread")]
async fn decorations_do_not_reach_the_provider_while_the_workspace_is_being_published() {
    let provider = Arc::new(MockTypeProvider::new());
    let fixture = fixture_with_provider(SOURCE, Some(provider.clone())).await;
    let server = fixture.server();
    server.ensure_current_file_synced(&fixture.uri).await;

    let inlay_hints = || InlayHintParams {
        work_done_progress_params: WorkDoneProgressParams::default(),
        text_document: TextDocumentIdentifier {
            uri: fixture.uri.clone(),
        },
        range: Range {
            start: Position::new(0, 0),
            end: Position::new(4, 0),
        },
    };
    let semantic_tokens = || SemanticTokensParams {
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        text_document: TextDocumentIdentifier {
            uri: fixture.uri.clone(),
        },
    };

    server.sync_coordinator.set_workspace_scan_in_progress(true);
    let _ = server.inlay_hint(inlay_hints()).await;
    let _ = server.semantic_tokens_full(semantic_tokens()).await;
    assert_eq!(
        decoration_queries(&provider),
        0,
        "no decoration may reach the provider mid-publication: {:?}",
        provider.calls()
    );

    // Discrimination: the SAME requests do reach the provider once the
    // publication is over, so the silence above is the gate and not a fixture
    // that never had a provider surface to ask about.
    server
        .sync_coordinator
        .set_workspace_scan_in_progress(false);
    let _ = server.inlay_hint(inlay_hints()).await;
    let _ = server.semantic_tokens_full(semantic_tokens()).await;
    assert_eq!(
        decoration_queries(&provider),
        2,
        "both decorations are provider-backed once the workspace is whole: {:?}",
        provider.calls()
    );

    fixture.shutdown().await;
}
