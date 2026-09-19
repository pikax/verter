//! Audit-aware wrappers for the LSP navigation feature handlers.
//!
//! Each `handle_<method>_with_audit` thunks into the matching
//! `handle_<method>` body via [`crate::audit_harness::run_with_audit`],
//! supplying the per-method timeout budget read from
//! [`verter_session::types::LspMethodTimeoutsConfig`] and a
//! per-method payload populator that captures response counts and
//! sizes. The trait impl in `mod.rs` calls the `*_with_audit`
//! variants directly so the production code path always carries the
//! audit registration when `audit_enabled = true`.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use super::nav_features::{handle_completion, handle_hover};
use super::nav_features_navigation::{handle_goto_definition, handle_references, handle_rename};
use super::VerterLanguageServer;
use crate::interaction_trace::{ProtocolStage, TraceSpan};

async fn with_protocol_trace<T>(
    span: TraceSpan<'_>,
    fut: impl std::future::Future<Output = Result<T>>,
    byte_len: impl FnOnce(&T) -> u32,
) -> Result<T> {
    span.mark(ProtocolStage::Admitted);
    span.mark(ProtocolStage::ProviderWork);
    let result = fut.await;
    span.mark(ProtocolStage::Serialize);
    if let Ok(value) = result.as_ref() {
        // Byte sizing is trace-only work: run it just for live spans.
        let n = span.request_epoch().map(|_| u64::from(byte_len(value)));
        span.mark_with_bytes(ProtocolStage::OutboundEnqueued, n);
        span.finish_ok();
    } else {
        // A failed request keeps a terminal state distinct from complete.
        span.fail();
    }
    result
}

/// Audit-aware wrapper for
/// [`super::nav_features::handle_hover`].
///
/// Routes through [`crate::audit_harness::run_with_audit`] so the
/// per-method timeout budget, cancellation marker, and records-store
/// publication all happen on the production code path. When
/// `audit_enabled = false`, this short-circuits to the original
/// handler with no observability cost.
pub(super) async fn handle_hover_with_audit(
    server: &VerterLanguageServer,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let host = server.documents.host_arc();
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .clone();
    let position = params.text_document_position_params.position;
    let target_identity = crate::audit_harness::target_identity_for_uri(&server.documents, &uri);
    let span = server.interaction_trace.begin("textDocument/hover", || {
        server.documents.get(&uri).map(|doc| doc.version as u64)
    });
    with_protocol_trace(
        span,
        crate::audit_harness::run_with_audit(
            &host,
            verter_audit::payloads::tags::LspMethodTag::Hover,
            target_identity,
            Some(position),
            async move { handle_hover(server, params).await },
            |payload, value| {
                payload.response_size_bytes = hover_response_size(value.as_ref());
            },
        ),
        |value| hover_response_size(value.as_ref()),
    )
    .await
}

/// Audit-aware wrapper for
/// [`super::nav_features::handle_completion`].
pub(super) async fn handle_completion_with_audit(
    server: &VerterLanguageServer,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let host = server.documents.host_arc();
    let uri = params.text_document_position.text_document.uri.clone();
    let position = params.text_document_position.position;
    let target_identity = crate::audit_harness::target_identity_for_uri(&server.documents, &uri);
    let span = server
        .interaction_trace
        .begin("textDocument/completion", || {
            server.documents.get(&uri).map(|doc| doc.version as u64)
        });
    with_protocol_trace(
        span,
        crate::audit_harness::run_with_audit(
            &host,
            verter_audit::payloads::tags::LspMethodTag::Completion,
            target_identity,
            Some(position),
            async move { handle_completion(server, params).await },
            |payload, value| {
                let count = match value {
                    Some(CompletionResponse::Array(items)) => items.len(),
                    Some(CompletionResponse::List(list)) => list.items.len(),
                    None => 0,
                };
                payload.num_completion_items = Some(u32::try_from(count).unwrap_or(u32::MAX));
                payload.response_size_bytes =
                    u32::try_from(count.saturating_mul(64)).unwrap_or(u32::MAX);
            },
        ),
        |value| match value {
            Some(CompletionResponse::Array(items)) => {
                u32::try_from(items.len().saturating_mul(64)).unwrap_or(u32::MAX)
            }
            Some(CompletionResponse::List(list)) => {
                u32::try_from(list.items.len().saturating_mul(64)).unwrap_or(u32::MAX)
            }
            None => 0,
        },
    )
    .await
}

/// Audit-aware wrapper for
/// [`super::nav_features_navigation::handle_goto_definition`].
pub(super) async fn handle_goto_definition_with_audit(
    server: &VerterLanguageServer,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let host = server.documents.host_arc();
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .clone();
    let position = params.text_document_position_params.position;
    let target_identity = crate::audit_harness::target_identity_for_uri(&server.documents, &uri);
    let span = server
        .interaction_trace
        .begin("textDocument/definition", || {
            server.documents.get(&uri).map(|doc| doc.version as u64)
        });
    with_protocol_trace(
        span,
        crate::audit_harness::run_with_audit(
            &host,
            verter_audit::payloads::tags::LspMethodTag::GotoDefinition,
            target_identity,
            Some(position),
            async move { handle_goto_definition(server, params).await },
            |payload, value| {
                let count = match value {
                    Some(GotoDefinitionResponse::Scalar(_)) => 1,
                    Some(GotoDefinitionResponse::Array(v)) => v.len(),
                    Some(GotoDefinitionResponse::Link(v)) => v.len(),
                    None => 0,
                };
                payload.response_size_bytes =
                    u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX);
            },
        ),
        |value| {
            let count = match value {
                Some(GotoDefinitionResponse::Scalar(_)) => 1,
                Some(GotoDefinitionResponse::Array(v)) => v.len(),
                Some(GotoDefinitionResponse::Link(v)) => v.len(),
                None => 0,
            };
            u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX)
        },
    )
    .await
}

/// Audit-aware wrapper for
/// [`super::nav_features_navigation::handle_references`].
pub(super) async fn handle_references_with_audit(
    server: &VerterLanguageServer,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let host = server.documents.host_arc();
    let uri = params.text_document_position.text_document.uri.clone();
    let position = params.text_document_position.position;
    let target_identity = crate::audit_harness::target_identity_for_uri(&server.documents, &uri);
    let span = server
        .interaction_trace
        .begin("textDocument/references", || {
            server.documents.get(&uri).map(|doc| doc.version as u64)
        });
    with_protocol_trace(
        span,
        crate::audit_harness::run_with_audit(
            &host,
            verter_audit::payloads::tags::LspMethodTag::References,
            target_identity,
            Some(position),
            async move { handle_references(server, params).await },
            |payload, value| {
                let count = value.as_ref().map(Vec::len).unwrap_or(0);
                payload.num_references = Some(u32::try_from(count).unwrap_or(u32::MAX));
                payload.response_size_bytes =
                    u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX);
            },
        ),
        |value| {
            let count = value.as_ref().map(Vec::len).unwrap_or(0);
            u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX)
        },
    )
    .await
}

/// Audit-aware wrapper for
/// [`super::nav_features_navigation::handle_rename`].
pub(super) async fn handle_rename_with_audit(
    server: &VerterLanguageServer,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let host = server.documents.host_arc();
    let uri = params.text_document_position.text_document.uri.clone();
    let position = params.text_document_position.position;
    let target_identity = crate::audit_harness::target_identity_for_uri(&server.documents, &uri);
    let span = server.interaction_trace.begin("textDocument/rename", || {
        server.documents.get(&uri).map(|doc| doc.version as u64)
    });
    with_protocol_trace(
        span,
        crate::audit_harness::run_with_audit(
            &host,
            verter_audit::payloads::tags::LspMethodTag::Rename,
            target_identity,
            Some(position),
            async move { handle_rename(server, params).await },
            |payload, value| {
                let edit_count = value
                    .as_ref()
                    .and_then(|w| w.changes.as_ref())
                    .map(|m| m.values().map(Vec::len).sum::<usize>())
                    .unwrap_or(0);
                payload.response_size_bytes =
                    u32::try_from(edit_count.saturating_mul(96)).unwrap_or(u32::MAX);
            },
        ),
        |value| {
            let edit_count = value
                .as_ref()
                .and_then(|w| w.changes.as_ref())
                .map(|m| m.values().map(Vec::len).sum::<usize>())
                .unwrap_or(0);
            u32::try_from(edit_count.saturating_mul(96)).unwrap_or(u32::MAX)
        },
    )
    .await
}

fn hover_response_size(hover: Option<&Hover>) -> u32 {
    let Some(h) = hover else {
        return 0;
    };
    let total: usize = match &h.contents {
        HoverContents::Scalar(s) => match s {
            MarkedString::String(s) => s.len(),
            MarkedString::LanguageString(ls) => ls.value.len() + ls.language.len(),
        },
        HoverContents::Array(items) => items
            .iter()
            .map(|m| match m {
                MarkedString::String(s) => s.len(),
                MarkedString::LanguageString(ls) => ls.value.len() + ls.language.len(),
            })
            .sum(),
        HoverContents::Markup(m) => m.value.len(),
    };
    u32::try_from(total).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::Arc;

    use verter_session::{HostConfig, VerterHost};

    use crate::interaction_trace::TraceStatus;
    use crate::{LspConfig, ProjectSyncMode};

    fn make_server() -> (
        tower_lsp_server::LspService<VerterLanguageServer>,
        tower_lsp_server::ClientSocket,
    ) {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let host_for_server = Arc::clone(&host);
        tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host_for_server),
                    type_provider: None,
                    project_sync_mode: ProjectSyncMode::FullProject,
                    type_provider_kind: crate::TypeProviderKind::EditorTsserver,
                    type_provider_topology: crate::TypeProviderTopology::EditorTsserver,
                    mcp_port: None,
                    type_provider_reason: Some("trace deferral test".into()),
                    type_provider_advisory: None,
                    suppress_imported_carrier_prewarm: true,
                },
            )
        })
    }

    #[tokio::test]
    async fn disabled_trace_runs_handler_without_epoch_or_byte_callbacks() {
        let (service, _socket) = make_server();
        let server = service.inner();
        assert!(!server.interaction_trace.is_enabled());

        let epoch_called = Cell::new(false);
        let byte_called = Cell::new(false);
        let handler_ran = Cell::new(false);
        let span = server.interaction_trace.begin("textDocument/hover", || {
            epoch_called.set(true);
            Some(3)
        });
        let result: Result<Option<u32>> = with_protocol_trace(
            span,
            async {
                handler_ran.set(true);
                Ok(None)
            },
            |_| {
                byte_called.set(true);
                0
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(
            handler_ran.get(),
            "the audited handler still runs with tracing disabled"
        );
        assert!(
            !epoch_called.get(),
            "source_epoch lookup must be deferred until tracing is enabled"
        );
        assert!(
            !byte_called.get(),
            "byte_len must only run while tracing is active"
        );
        assert!(server.interaction_trace.snapshot().traces.is_empty());
    }

    #[tokio::test]
    async fn enabled_trace_evaluates_epoch_and_byte_callbacks() {
        let (service, _socket) = make_server();
        let server = service.inner();
        server.interaction_trace.set_enabled(true);

        let span = server
            .interaction_trace
            .begin("textDocument/hover", || Some(7));
        let result: Result<Option<u32>> =
            with_protocol_trace(span, async { Ok(Some(42u32)) }, |value| {
                value.map_or(0, |v| v + 1)
            })
            .await;

        assert_eq!(result.unwrap(), Some(42));
        let snap = server.interaction_trace.snapshot();
        let trace = &snap.traces[0];
        assert_eq!(trace.source_epoch, Some(7));
        assert_eq!(trace.status, TraceStatus::Complete);
        let enqueued = trace
            .stamps
            .iter()
            .find(|stamp| stamp.stage == ProtocolStage::OutboundEnqueued)
            .expect("outbound enqueued stamp");
        assert_eq!(enqueued.byte_length, Some(43));
    }
}
