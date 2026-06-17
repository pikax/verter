//! Completion-resolve auto-import edit translation for LSP.
//!
//! Hosts `resolve_tsgo_auto_import_edits` — which maps a TSGO
//! completion-resolve's generated-TSX `additionalTextEdits` back into
//! Vue-source [`TextEdit`]s — and `completion_resolve_error`, the
//! structured JSON-RPC error it reports those failures through.
//!
//! Extracted from `nav_features` to keep that source under the
//! file-size guard (`no_oversize_files`); `handle_completion_resolve`
//! calls both functions through this sibling module.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::tsgo::auto_import::{
    resolve_script_import_anchor, translate_completion_import_edits, ProviderImportEdit,
};

use super::VerterLanguageServer;

/// Translate a TSGO completion-resolve's `additionalTextEdits` (generated-TSX byte offsets) into
/// Vue-source [`TextEdit`]s, or fail with a human-readable reason.
///
/// Called only once the provider has returned a NON-EMPTY auto-import edit set, so every failure
/// here is a hard rejection: missing IDE context / Vue URI / open document, or an edit that cannot
/// be placed (see [`translate_completion_import_edits`]). The caller turns the reason into a
/// structured resolve error rather than returning a successful item with the edits dropped.
pub(super) fn resolve_tsgo_auto_import_edits(
    server: &VerterLanguageServer,
    tsx_path: &str,
    provider_edits: &[ProviderImportEdit],
) -> std::result::Result<Vec<TextEdit>, String> {
    let (_, tsx_content, mapper) = server
        .ide_context_by_path(tsx_path)
        .ok_or_else(|| format!("no IDE context for {tsx_path}"))?;
    let vue_uri = server
        .vue_uri_from_ide_path(tsx_path)
        .ok_or_else(|| format!("no Vue URI for {tsx_path}"))?;
    let doc = server
        .documents
        .get(&vue_uri)
        .ok_or_else(|| format!("no open document for {}", vue_uri.as_str()))?;

    let tsx_li = LineIndex::new(&tsx_content, server.documents.encoding());
    // `AnalyzedImport.span` is SFC-absolute; pass the spans straight through. The anchor authority
    // consumes them in that coordinate space and filters to the selected `<script setup>` block.
    let user_import_spans: Vec<(u32, u32)> = server
        .documents
        .get_analysis(&vue_uri)
        .map(|a| {
            a.imports
                .iter()
                .map(|imp| (imp.span.start, imp.span.end))
                .collect()
        })
        .unwrap_or_default();
    let anchor = resolve_script_import_anchor(&doc.source, &user_import_spans);

    let edits = translate_completion_import_edits(
        provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &doc.line_index,
    )
    .map_err(|e| e.to_string())?;

    if edits.is_empty() {
        return Err("translation produced no edits".to_string());
    }
    Ok(edits)
}

/// Build a structured JSON-RPC error for a failed completion resolve.
pub(super) fn completion_resolve_error(reason: &str) -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
        message: std::borrow::Cow::Owned(format!("completion resolve: {reason}")),
        data: None,
    }
}
