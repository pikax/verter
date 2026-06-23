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
use crate::type_provider::auto_import::{
    resolve_script_import_anchor, translate_completion_import_edits, ProviderImportEdit,
};

use super::VerterLanguageServer;

/// Translate a type provider's completion-resolve `additionalTextEdits` (generated-TSX byte
/// offsets) into carrier-source [`TextEdit`]s.
///
/// The auto-import re-anchor maps provider edits back through a Vue `<script setup>` carrier — it
/// reverse-maps a carrier IDE TSX path to its `.vue` source and re-anchors the import at the SFC's
/// `<script setup>` insertion site. It is meaningful ONLY for a Vue carrier-IDE projection: the
/// re-anchor decision is routed through the typed carrier-kind authority
/// ([`carrier_kind_for_language`](crate::features::auto_close_tag::carrier_kind_for_language)) so
/// only a `CarrierKind::Vue` carrier continues.
///
/// Three outcomes:
/// * `Ok(Some(edits))` — the provider path is a Vue carrier and the edits were placed in its
///   carrier source. Called only once the provider has returned a NON-EMPTY edit set, so a carrier
///   that cannot place those edits is a hard rejection (`Err`), never a successful item with the
///   import silently dropped.
/// * `Ok(None)` — the carrier re-anchor does not apply. This covers (a) a path that reverse-maps to
///   neither a carrier nor itself (no resolvable source); (b) a self-file rune module (a Svelte
///   `.svelte.ts` / `.svelte.js`, whose path reverse-maps to itself); and (c) a real, non-self-file
///   carrier of a NON-Vue framework (a Svelte `.svelte` component) — gated out by the typed
///   carrier-kind authority, because the `<script setup>` re-anchor is Vue-SFC-specific. In every
///   case fail closed by leaving the item unchanged rather than synthesizing a Vue `<script setup>`
///   block into a non-Vue source. (Self-file auto-import edit placement is a separate capability,
///   not handled by this carrier re-anchor.) A Vue carrier that cannot place the edits is an `Err`,
///   not `Ok(None)`.
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
    // The carrier `<script setup>` import re-anchor is a Vue-SFC-specific construct. A real,
    // non-self-file carrier of another framework (a Svelte `.svelte` component) reverse-maps to its
    // carrier source and is NOT self-file, so it slips past the self-file gate above; driving the
    // Vue-specific `resolve_script_import_anchor` over it would synthesize a bogus Vue
    // `<script setup>` block into the non-Vue source. Route the re-anchor decision through the SAME
    // typed carrier-kind authority the code-action preamble path uses
    // (`resolve_carrier_preamble_import_anchor`): classify the carrier URI to a `FileLanguage` via
    // the shared carrier classifier, then map it to a `CarrierKind` through the fail-closed,
    // descriptor-identity `carrier_kind_for_language`. ONLY a `Some(CarrierKind::Vue)` continues; a
    // Svelte / non-carrier classification — and any future markup carrier without its own arm —
    // fails closed here (`Ok(None)`), never falling through into Vue `<script setup>` synthesis.
    let carrier_continues =
        crate::server::carrier_language_for(carrier_uri.as_str()).is_some_and(|language| {
            matches!(
                crate::features::auto_close_tag::carrier_kind_for_language(&language),
                Some(crate::features::auto_close_tag::CarrierKind::Vue)
            )
        });
    if !carrier_continues {
        return Ok(None);
    }
    // The path reverse-mapped to a REAL Vue `.vue` carrier (not self-file), so a
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

/// Merge a resolve-supplied [`CompletionLabelDetails`](crate::type_provider::protocol::CompletionLabelDetails)
/// onto the item's list-time label details, SUB-FIELD by sub-field.
///
/// `completionItem/resolve` enrichment is additive: a resolve may refine ONE
/// label-detail sub-field (e.g. fill in the right-aligned import `description`)
/// while the completion LIST already carried the other (e.g. the inline signature
/// `detail`). Overwriting the whole `CompletionItemLabelDetails` would drop the
/// list-time sub-field the resolve did not re-send. So per sub-field: take the
/// resolve's value when it is `Some`, otherwise KEEP the existing list-time value.
///
/// `existing` is the item's current label details (`None` when the list carried
/// none — then the merged result is just the resolve's sub-fields).
pub(super) fn merge_resolved_label_details(
    existing: Option<CompletionItemLabelDetails>,
    resolved: crate::type_provider::protocol::CompletionLabelDetails,
) -> CompletionItemLabelDetails {
    let (existing_detail, existing_description) = match existing {
        Some(ld) => (ld.detail, ld.description),
        None => (None, None),
    };
    CompletionItemLabelDetails {
        // Resolve-supplied sub-field wins; an absent resolve sub-field preserves
        // the list-time value.
        detail: resolved.detail.or(existing_detail),
        description: resolved.description.or(existing_description),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_provider::protocol;

    /// A resolve that supplies ONLY `description` must merge onto a list-time
    /// label-details carrying ONLY `detail` — the result keeps BOTH. Discriminating
    /// against the pre-fix overwrite (which replaced the whole struct and dropped
    /// the list-time `detail`).
    #[test]
    fn merge_resolved_label_details_preserves_list_time_subfield() {
        let existing = Some(CompletionItemLabelDetails {
            detail: Some("(app: App)".to_string()),
            description: None,
        });
        let resolved = protocol::CompletionLabelDetails {
            detail: None,
            description: Some("vue".to_string()),
        };

        let merged = merge_resolved_label_details(existing, resolved);
        assert_eq!(
            merged.detail.as_deref(),
            Some("(app: App)"),
            "the list-time `detail` must survive a resolve that only refined `description`"
        );
        assert_eq!(
            merged.description.as_deref(),
            Some("vue"),
            "the resolve-supplied `description` must be folded in"
        );
    }

    /// A resolve sub-field wins over the list-time value when BOTH are present.
    #[test]
    fn merge_resolved_label_details_resolve_subfield_wins() {
        let existing = Some(CompletionItemLabelDetails {
            detail: Some("(stale)".to_string()),
            description: Some("stale-src".to_string()),
        });
        let resolved = protocol::CompletionLabelDetails {
            detail: Some("(refined)".to_string()),
            description: None,
        };

        let merged = merge_resolved_label_details(existing, resolved);
        assert_eq!(
            merged.detail.as_deref(),
            Some("(refined)"),
            "the resolve-supplied `detail` wins over the list-time one"
        );
        assert_eq!(
            merged.description.as_deref(),
            Some("stale-src"),
            "the un-refined `description` keeps the list-time value"
        );
    }

    /// When the item had NO list-time label details, the merge is just the
    /// resolve's sub-fields.
    #[test]
    fn merge_resolved_label_details_sets_when_item_had_none() {
        let resolved = protocol::CompletionLabelDetails {
            detail: Some("(d)".to_string()),
            description: Some("src".to_string()),
        };
        let merged = merge_resolved_label_details(None, resolved);
        assert_eq!(merged.detail.as_deref(), Some("(d)"));
        assert_eq!(merged.description.as_deref(), Some("src"));
    }
}
