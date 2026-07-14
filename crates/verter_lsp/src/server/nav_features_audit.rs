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
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.hover;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::Hover,
        canonical_id,
        Some(position),
        budget,
        async move { handle_hover(server, params).await },
        |payload, value| {
            payload.response_size_bytes = hover_response_size(value.as_ref());
        },
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
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.completion;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::Completion,
        canonical_id,
        Some(position),
        budget,
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
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.goto_definition;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::GotoDefinition,
        canonical_id,
        Some(position),
        budget,
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
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.references;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::References,
        canonical_id,
        Some(position),
        budget,
        async move { handle_references(server, params).await },
        |payload, value| {
            let count = value.as_ref().map(Vec::len).unwrap_or(0);
            payload.num_references = Some(u32::try_from(count).unwrap_or(u32::MAX));
            payload.response_size_bytes =
                u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX);
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
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.rename;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::Rename,
        canonical_id,
        Some(position),
        budget,
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
