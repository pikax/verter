//! Completion-resolve auto-import edit translation for LSP.
//!
//! Hosts `resolve_provider_auto_import_edits` — which maps ANY type provider's
//! completion-resolve's generated-TSX `additionalTextEdits` back into
//! carrier-source [`TextEdit`]s — and `completion_resolve_error`, the
//! structured JSON-RPC error it reports those failures through.
//!
//! The translation is provider-neutral: the generated-TSX → `.vue` carrier
//! re-anchor is identical whether TSGO, tsserver, or the extension produced the
//! edits.
//!
//! `handle_completion_resolve` calls both functions through this sibling
//! module; it lives apart from `nav_features` so each stays within the
//! file-size guard (`no_oversize_files`).

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::tsgo::auto_import::{
    resolve_script_import_anchor, translate_completion_import_edits, ProviderImportEdit,
};

use super::VerterLanguageServer;

/// Translate a type provider's completion-resolve `additionalTextEdits` (generated-TSX byte
/// offsets) into carrier-source [`TextEdit`]s.
///
/// The auto-import re-anchor maps provider edits back through a Vue `<script setup>` carrier — it
/// reverse-maps a carrier IDE TSX path to its `.vue` source and re-anchors the import at the SFC's
/// `<script setup>` insertion site. It is meaningful ONLY for a carrier-IDE projection.
///
/// Three outcomes:
/// * `Ok(Some(edits))` — the provider path is a Vue carrier and the edits were placed in its
///   carrier source. Called only once the provider has returned a NON-EMPTY edit set, so a carrier
///   that cannot place those edits is a hard rejection (`Err`), never a successful item with the
///   import silently dropped.
/// * `Ok(None)` — the provider path is NOT a resolvable Vue carrier (a self-file rune module such
///   as a Svelte `.svelte.ts` / `.svelte.js`, or any non-carrier projection). The carrier
///   re-anchor does not apply; fail closed by leaving the item unchanged rather than synthesizing
///   a Vue `<script setup>` block into a non-Vue source. (Self-file auto-import edit placement is a
///   separate capability, not handled by this carrier re-anchor.) `Ok(None)` is reserved for THIS
///   no-carrier / self-file case — a real carrier that cannot place the edits is an `Err`.
/// * `Err(reason)` — a genuine carrier-resolve failure for a path that DID reverse-map to a Vue
///   carrier: missing IDE context / open document, or an edit that cannot be placed. The caller
///   turns the reason into a structured resolve error rather than dropping the import edits.
pub(super) fn resolve_provider_auto_import_edits(
    server: &VerterLanguageServer,
    tsx_path: &str,
    provider_edits: &[ProviderImportEdit],
) -> std::result::Result<Option<Vec<TextEdit>>, String> {
    // Reverse-map the provider path to its owning source URI. A self-file rune module maps to
    // ITSELF; a Vue carrier IDE path maps to its `.vue` source. A path that maps to neither is not
    // a resolvable carrier — fail closed.
    let Some(carrier_uri) = server.carrier_uri_from_ide_path(tsx_path) else {
        return Ok(None);
    };
    // The carrier re-anchor is meaningful only for a carrier-IDE projection. A self-file projection
    // (a Svelte `.svelte.ts` / `.svelte.js` rune module, whose path reverse-maps to itself) has no
    // `<script setup>` carrier to re-anchor into — fail closed rather than reach the Vue-specific
    // `resolve_script_import_anchor` and synthesize a bogus block.
    if server.is_self_file_projection(&carrier_uri) {
        return Ok(None);
    }
    // The path reverse-mapped to a REAL `.vue` carrier (not self-file), so a
    // missing IDE context is a genuine carrier-resolve FAILURE, not a no-carrier
    // case. Fail with a structured `Err` so the caller reports it — never an
    // `Ok(None)` success, which would silently drop the provider's non-empty
    // auto-import edits ("accepted completion but no import").
    let Some((_, tsx_content, mapper)) = server.ide_context_by_path(tsx_path) else {
        return Err(format!("no IDE context for {tsx_path}"));
    };
    let doc = server
        .documents
        .get(&carrier_uri)
        .ok_or_else(|| format!("no open document for {}", carrier_uri.as_str()))?;

    let tsx_li = LineIndex::new(&tsx_content, server.documents.encoding());
    // `AnalyzedImport.span` is SFC-absolute; pass the spans straight through. The anchor authority
    // consumes them in that coordinate space and filters to the selected `<script setup>` block.
    let user_import_spans: Vec<(u32, u32)> = server
        .documents
        .get_analysis(&carrier_uri)
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
    Ok(Some(edits))
}

/// Build a structured JSON-RPC error for a failed completion resolve.
pub(super) fn completion_resolve_error(reason: &str) -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
        message: std::borrow::Cow::Owned(format!("completion resolve: {reason}")),
        data: None,
    }
}
