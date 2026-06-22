//! LSP auxiliary feature method bodies.
//!
//! Free functions hosting the bodies of `impl LanguageServer for
//! VerterLanguageServer` auxiliary feature methods (document_symbol,
//! folding_range, selection_range, document_highlight, signature_help,
//! code_action, semantic_tokens_full, code_lens, inlay_hint,
//! linked_editing_range, document_link, document_color,
//! color_presentation, formatting, on_type_formatting, symbol,
//! prepare_call_hierarchy, incoming_calls, outgoing_calls).
//!
//! The trait impl block stays in `mod.rs`; each trait method is a
//! 1-line stub that delegates to the corresponding `handle_<method>`
//! free function here.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::features::action_utils::fix_placeholder_uris;
use crate::features::call_hierarchy;
use crate::features::code_lens::code_lenses;
use crate::features::color_info;
use crate::features::document_highlight::highlights_at_position;
use crate::features::document_link::build_document_links;
use crate::features::document_symbol::build_document_symbols;
use crate::features::folding_range::build_folding_ranges;
use crate::features::formatting::format_document;
use crate::features::linked_editing::linked_editing_ranges;
use crate::features::organize_imports::organize_imports_actions;
use crate::features::workspace_symbol::workspace_symbols;
use crate::type_provider::merge;
use crate::type_provider::protocol::ProviderDiagnosticContext;

use super::handler_guard::{block_in_place_if_available, HandlerGuard};
use super::server_utils::*;
use super::VerterLanguageServer;

pub(super) async fn handle_document_symbol(
    server: &VerterLanguageServer,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let _hg = HandlerGuard::new("document_symbol");
    let uri = &params.text_document.uri;

    let symbols = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let symbols = build_document_symbols(&blocks, analysis.as_ref(), &doc.line_index);
        if symbols.is_empty() {
            None
        } else {
            Some(symbols)
        }
    })();

    Ok(symbols.map(DocumentSymbolResponse::Nested))
}

/// Audit-aware wrapper for [`handle_document_symbol`].
pub(super) async fn handle_document_symbol_with_audit(
    server: &VerterLanguageServer,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let host = server.documents.host_arc();
    let uri = params.text_document.uri.clone();
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.document_symbols;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::DocumentSymbols,
        canonical_id,
        None,
        budget,
        async move { handle_document_symbol(server, params).await },
        |payload, value| {
            let count = match value {
                Some(DocumentSymbolResponse::Flat(v)) => v.len(),
                Some(DocumentSymbolResponse::Nested(v)) => v.len(),
                None => 0,
            };
            payload.num_symbols = Some(u32::try_from(count).unwrap_or(u32::MAX));
            payload.response_size_bytes =
                u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX);
        },
    )
    .await
}

pub(super) async fn handle_folding_range(
    server: &VerterLanguageServer,
    params: FoldingRangeParams,
) -> Result<Option<Vec<FoldingRange>>> {
    let _hg = HandlerGuard::new("folding_range");
    let uri = &params.text_document.uri;

    let ranges = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let ranges = build_folding_ranges(&blocks, analysis.as_ref(), &doc.line_index);
        if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        }
    })();

    Ok(ranges)
}

pub(super) async fn handle_selection_range(
    server: &VerterLanguageServer,
    params: SelectionRangeParams,
) -> Result<Option<Vec<SelectionRange>>> {
    let _hg = HandlerGuard::new("selection_range");
    let uri = &params.text_document.uri;

    let result = (|| {
        let doc = server.documents.get(uri)?;
        let blocks = scan_sfc_blocks(&doc.source);
        let line_index = &doc.line_index;
        let source_len = doc.source.len() as u32;

        let file_range = Range {
            start: line_index.offset_to_position(0).unwrap_or_default(),
            end: line_index
                .offset_to_position(source_len)
                .unwrap_or_default(),
        };

        let ranges: Vec<_> = params
            .positions
            .iter()
            .map(|pos| {
                let offset = line_index.position_to_offset(pos).unwrap_or(0) as usize;

                // Find the containing block
                let block = blocks.iter().find(|b| {
                    let (cs, ce) = b.content_range();
                    offset >= cs as usize && offset <= ce as usize
                });

                if let Some(block) = block {
                    let (cs, ce) = block.content_range();
                    let content_range = Range {
                        start: line_index.offset_to_position(cs).unwrap_or_default(),
                        end: line_index.offset_to_position(ce).unwrap_or_default(),
                    };
                    let block_range = Range {
                        start: line_index
                            .offset_to_position(block.open_tag_start)
                            .unwrap_or_default(),
                        end: line_index
                            .offset_to_position(block.close_tag_end)
                            .unwrap_or_default(),
                    };

                    SelectionRange {
                        range: content_range,
                        parent: Some(Box::new(SelectionRange {
                            range: block_range,
                            parent: Some(Box::new(SelectionRange {
                                range: file_range,
                                parent: None,
                            })),
                        })),
                    }
                } else {
                    SelectionRange {
                        range: file_range,
                        parent: None,
                    }
                }
            })
            .collect();

        Some(ranges)
    })();

    Ok(result)
}

pub(super) async fn handle_document_highlight(
    server: &VerterLanguageServer,
    params: DocumentHighlightParams,
) -> Result<Option<Vec<DocumentHighlight>>> {
    let _hg = HandlerGuard::new("document_highlight");
    let uri = &params.text_document_position_params.text_document.uri;
    let position = &params.text_document_position_params.position;

    // Virtual file: route directly through TSGO
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_highlights) = tp.get_document_highlights(&tsx_path, offset).await {
                    let highlights: Vec<DocumentHighlight> = type_highlights
                        .into_iter()
                        .filter_map(|h| {
                            Some(DocumentHighlight {
                                range: Range {
                                    start: vf_li.offset_to_position(h.start)?,
                                    end: vf_li.offset_to_position(h.end)?,
                                },
                                kind: Some(match h.kind {
                                    crate::type_provider::protocol::TypeDocumentHighlightKind::Read => {
                                        DocumentHighlightKind::READ
                                    }
                                    crate::type_provider::protocol::TypeDocumentHighlightKind::Write => {
                                        DocumentHighlightKind::WRITE
                                    }
                                    _ => DocumentHighlightKind::TEXT,
                                }),
                            })
                        })
                        .collect();
                    return Ok(if highlights.is_empty() {
                        None
                    } else {
                        Some(highlights)
                    });
                }
            }
            return Ok(None);
        }
    }

    let verter_result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        highlights_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
        )
    })();

    // Enhance with TypeProvider if available
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, tsx_content, mapper)) = server.ide_context(uri) {
            let tsx_li = LineIndex::new(&tsx_content, server.documents.encoding());
            if let Some(doc) = server.documents.get(uri) {
                if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                    position,
                    &doc.line_index,
                    &mapper,
                    &tsx_li,
                ) {
                    if let Ok(type_highlights) =
                        tp.get_document_highlights(&tsx_path, tsx_offset).await
                    {
                        return Ok(merge::merge_document_highlights(
                            verter_result,
                            type_highlights,
                            &tsx_li,
                            &mapper,
                            &doc.line_index,
                        ));
                    }
                }
            }
        }
    }

    Ok(verter_result)
}

pub(super) async fn handle_signature_help(
    server: &VerterLanguageServer,
    params: SignatureHelpParams,
) -> Result<Option<SignatureHelp>> {
    let _hg = HandlerGuard::new("signature_help");
    let uri = &params.text_document_position_params.text_document.uri;
    let position = &params.text_document_position_params.position;

    // Virtual file: route directly through TSGO
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_sig) = tp.get_signature_help(&tsx_path, offset).await {
                    return Ok(merge::merge_signature_help(type_sig));
                }
            }
            return Ok(None);
        }
    }

    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        if let Some(ctx) = server.type_provider_context(uri) {
            if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                position,
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            ) {
                if let Ok(type_sig) = tp.get_signature_help(&ctx.tsx_path, tsx_offset).await {
                    return Ok(merge::merge_signature_help(type_sig));
                }
            }
        }
    }

    Ok(None)
}

pub(super) async fn handle_code_action(
    server: &VerterLanguageServer,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let _hg = HandlerGuard::new("code_action");
    let uri = &params.text_document.uri;
    let range = &params.range;

    let only = params.context.only.as_deref();

    let mut all_actions: Vec<CodeActionOrCommand> = Vec::new();

    // Verter's own code actions (organize imports)
    if let Some(doc) = server.documents.get(uri) {
        let analysis = server.documents.get_analysis(uri);

        if wants_code_action_kind(only, "source.organizeImports") {
            let mut verter_actions =
                organize_imports_actions(&doc.source, analysis.as_ref(), &doc.line_index);
            fix_placeholder_uris(&mut verter_actions, uri);
            all_actions.extend(verter_actions);
        }

        // Extract component refactoring
        if wants_code_action_kind(only, "refactor.extract") {
            let blocks = scan_sfc_blocks(&doc.source);
            if let Some(extract_action) =
                crate::features::extract_component::extract_component_action(
                    &doc.source,
                    range,
                    &blocks,
                    &doc.line_index,
                    uri,
                )
            {
                all_actions.push(extract_action);
            }
        }

        if wants_code_action_kind(only, "quickfix") {
            let blocks = scan_sfc_blocks(&doc.source);

            // Macro code actions (defineSlots, defineEmits generation/augmentation)
            let cursor_offset = doc.line_index.position_to_offset(&range.start);
            let mut macro_actions = crate::features::macro_actions::macro_code_actions(
                &doc.source,
                analysis.as_ref(),
                &blocks,
                &doc.line_index,
                cursor_offset,
            );
            fix_placeholder_uris(&mut macro_actions, uri);
            all_actions.extend(macro_actions);

            // Component code actions (add unknown props/v-models to child)
            if let Some(ref analysis) = analysis {
                let comp_actions = crate::features::component_actions::component_code_actions(
                    analysis,
                    &|import_source| server.resolve_component_context(uri, import_source, None),
                );
                all_actions.extend(comp_actions);

                // Suggest matching props from parent bindings to child component tags
                let suggest_actions = crate::features::component_actions::suggest_matching_props(
                    analysis,
                    &doc.source,
                    &doc.line_index,
                    uri,
                    &|import_source| server.resolve_component_context(uri, import_source, None),
                );
                all_actions.extend(suggest_actions);

                // Event handler type hint actions
                let mut event_actions = crate::features::event_type_hints::event_type_hint_actions(
                    analysis,
                    &doc.source,
                    &doc.line_index,
                );
                fix_placeholder_uris(&mut event_actions, uri);
                all_actions.extend(event_actions);
            }
        }

        let wants_quickfix = wants_code_action_kind(only, "quickfix");
        let wants_refactor = wants_code_action_kind(only, "refactor");

        // Action engine quick fixes and refactorings.
        if wants_quickfix || wants_refactor {
            if let Some(ref analysis) = analysis {
                let canonical_id = uri_to_canonical_id(uri);
                let linter = server.linter_for_file(&canonical_id);
                if wants_quickfix {
                    all_actions.extend(crate::features::diagnostics_bridge::action_engine_fixes(
                        &server.action_engine,
                        analysis,
                        &doc.source,
                        &doc.line_index,
                        &linter,
                        &params.context.diagnostics,
                        uri,
                    ));
                }
                if wants_refactor {
                    if let Some(offset) = doc.line_index.position_to_offset(&range.start) {
                        all_actions.extend(
                            crate::features::diagnostics_bridge::action_engine_refactorings(
                                &server.action_engine,
                                analysis,
                                &doc.source,
                                &doc.line_index,
                                &linter,
                                offset,
                                uri,
                            ),
                        );
                    }
                }
            }
        }
    }

    // TypeProvider code actions (TSGO quick fixes, refactorings).
    // Skip during typing cooldown to keep TSGO pipeline clear for interactive requests.
    // Extract all context synchronously — no DashMap guard held across await.
    //
    // GATED OFF for a SELF-FILE rune-module own buffer: a code-action's
    // workspace-EDIT positions are not yet mapped through the self-file mapper,
    // so an applied edit could land off by the prelude offset (or inside the
    // prelude) and CORRUPT the module. Code actions stay DEFERRED for the
    // self-file projection — a clean no-op, never a wrong/unmapped edit.
    // (Carrier code actions unchanged.)
    // The provider's `get_code_actions` issues `getCodeFixes` only — it returns
    // QUICKFIX-kind actions (e.g. the TS6133 remove-unused-declaration fix and its
    // delete-all-unused companion), never refactors and never source actions. So
    // gating it on a non-`quickfix` kind would return quickfixes the client
    // explicitly did NOT ask for (an LSP `context.only` violation). Gate on
    // `quickfix` / `None` only; the implicit `None` (all-kinds) case still fires
    // because `wants_code_action_kind(None, _)` is true. The `source.removeUnused`
    // SOURCE action (fixAll-on-save / "Source Action…" removing all unused without a
    // cursor-on-diagnostic) is a separate surface DEFERRED to the `source.*` backlog
    // — it is intentionally NOT wired into this gate.
    if !server.is_self_file_projection(uri)
        && !server.is_typing_cooldown()
        && wants_code_action_kind(only, "quickfix")
    {
        if let Some(tp) = &server.type_provider {
            if let Some(ctx) = server.type_provider_context(uri) {
                let start_offset = merge::carrier_position_to_tsx_offset_validated(
                    &range.start,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );
                let end_offset = merge::carrier_position_to_tsx_offset_validated(
                    &range.end,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );
                // Resolve the editor's diagnostics (codes + ranges) into the
                // provider-facing context: parse each `code` to an integer and map
                // each range to TSX byte offsets, fail-closed (an unparseable code
                // or an unmappable range drops that diagnostic). The provider feeds
                // these into `getCodeFixes` / `context.diagnostics`.
                let diag_ctx = build_provider_diagnostic_contexts(
                    &params.context.diagnostics,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );
                if let (Some(so), Some(eo)) = (start_offset, end_offset) {
                    if let Ok(type_actions) =
                        tp.get_code_actions(&ctx.tsx_path, so, eo, &diag_ctx).await
                    {
                        let carrier_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let negotiated_encoding = server.position_encoding.read().clone();
                        // For the add-import prelude re-anchor: a provider `addMissingImport`
                        // quickfix inserts a brand-new import at the synthetic TSX helper-preamble,
                        // which the strict mapper drops; `merge_code_actions` re-anchors that
                        // CURRENT-file insertion at the SFC's `<script setup>` import site through
                        // the shared completion re-anchor. It needs the carrier source and the
                        // SFC-absolute top-level import spans (exactly the completion-resolve
                        // inputs). Empty/absent when the carrier document is not open — then a
                        // preamble insertion has no anchor and stays dropped (fail-closed).
                        let carrier_source: String = server
                            .documents
                            .get(uri)
                            .map(|doc| doc.source.to_string())
                            .unwrap_or_default();
                        let user_import_spans: Vec<(u32, u32)> = server
                            .documents
                            .get_analysis(uri)
                            .map(|a| {
                                a.imports
                                    .iter()
                                    .map(|imp| (imp.span.start, imp.span.end))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let actions = merge::merge_code_actions(
                            type_actions,
                            &ctx.tsx_path,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            &carrier_source_exists,
                            negotiated_encoding,
                            &|p: &str| {
                                block_in_place_if_available(|| {
                                    server.documents.host().workspace_read().read_file(p)
                                })
                            },
                            &carrier_source,
                            &user_import_spans,
                        );
                        all_actions.extend(actions);
                    }
                }
            }
        }
    }

    Ok(if all_actions.is_empty() {
        None
    } else {
        Some(all_actions)
    })
}

/// Parse an LSP diagnostic `code` into the integer TypeScript error code the
/// provider code-fix path keys on.
///
/// - `Number(n)` → included when it fits `u32`.
/// - `String(s)` → parsed as a decimal `u32` (Verter publishes TS codes as
///   strings, e.g. `"6133"`).
/// - a non-numeric string or a missing code → `None` (skipped, fail-closed).
fn parse_diagnostic_code(code: Option<&NumberOrString>) -> Option<u32> {
    match code? {
        NumberOrString::Number(n) => u32::try_from(*n).ok(),
        NumberOrString::String(s) => s.parse::<u32>().ok(),
    }
}

/// Resolve the editor's `params.context.diagnostics` into the provider-facing
/// [`ProviderDiagnosticContext`] list: parse each `code` to an integer and map
/// each range to TSX byte offsets in the queried generated file.
///
/// Fail-closed: a diagnostic whose code is non-numeric, or whose range does not
/// map cleanly into the TSX, is dropped rather than forwarded with a guessed code
/// or an off-by-prelude range.
fn build_provider_diagnostic_contexts(
    diagnostics: &[Diagnostic],
    carrier_line_index: &LineIndex,
    mapper: &crate::documents::provider_projection::ProviderPositionMapper,
    tsx_line_index: &LineIndex,
) -> Vec<ProviderDiagnosticContext> {
    diagnostics
        .iter()
        .filter_map(|diag| {
            let code = parse_diagnostic_code(diag.code.as_ref())?;
            let start = merge::carrier_position_to_tsx_offset_validated(
                &diag.range.start,
                carrier_line_index,
                mapper,
                tsx_line_index,
            )?;
            let end = merge::carrier_position_to_tsx_offset_validated(
                &diag.range.end,
                carrier_line_index,
                mapper,
                tsx_line_index,
            )?;
            Some(ProviderDiagnosticContext { code, start, end })
        })
        .collect()
}

/// Audit-aware wrapper for [`handle_code_action`].
pub(super) async fn handle_code_action_with_audit(
    server: &VerterLanguageServer,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let host = server.documents.host_arc();
    let uri = params.text_document.uri.clone();
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.code_action;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::CodeAction,
        canonical_id,
        None,
        budget,
        async move { handle_code_action(server, params).await },
        |payload, value| {
            let count = value.as_ref().map(Vec::len).unwrap_or(0);
            payload.response_size_bytes =
                u32::try_from(count.saturating_mul(96)).unwrap_or(u32::MAX);
        },
    )
    .await
}

pub(super) async fn handle_semantic_tokens_full(
    server: &VerterLanguageServer,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokensResult>> {
    let _hg = HandlerGuard::new("semantic_tokens");
    let uri = &params.text_document.uri;

    // Skip TSGO while typing — serial TSGO pipeline must stay clear
    // for interactive requests. VS Code re-requests after the typing pause.
    // Extract all context synchronously — no DashMap guard held across await.
    if !server.is_typing_cooldown() {
        if let Some(tp) = &server.type_provider {
            if let Some(ctx) = server.type_provider_context(uri) {
                if let Ok(type_tokens) = tp.get_semantic_tokens(&ctx.tsx_path).await {
                    let tokens = merge::merge_semantic_tokens(
                        type_tokens,
                        &ctx.tsx_line_index,
                        &ctx.mapper,
                        &ctx.carrier_line_index,
                    );
                    if !tokens.is_empty() {
                        return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                            result_id: None,
                            data: tokens,
                        })));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Audit-aware wrapper for [`handle_semantic_tokens_full`].
pub(super) async fn handle_semantic_tokens_full_with_audit(
    server: &VerterLanguageServer,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokensResult>> {
    let host = server.documents.host_arc();
    let uri = params.text_document.uri.clone();
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.semantic_tokens;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::SemanticTokens,
        canonical_id,
        None,
        budget,
        async move { handle_semantic_tokens_full(server, params).await },
        |payload, value| {
            let count = match value {
                Some(SemanticTokensResult::Tokens(t)) => t.data.len(),
                Some(SemanticTokensResult::Partial(p)) => p.data.len(),
                None => 0,
            };
            payload.response_size_bytes =
                u32::try_from(count.saturating_mul(20)).unwrap_or(u32::MAX);
        },
    )
    .await
}

pub(super) async fn handle_code_lens(
    server: &VerterLanguageServer,
    params: CodeLensParams,
) -> Result<Option<Vec<CodeLens>>> {
    let _hg = HandlerGuard::new("code_lens");
    let uri = &params.text_document.uri;

    let lenses = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        Some(code_lenses(&blocks, analysis.as_ref(), &doc.line_index))
    })();

    match lenses {
        Some(v) if !v.is_empty() => Ok(Some(v)),
        _ => Ok(None),
    }
}

pub(super) async fn handle_inlay_hint(
    server: &VerterLanguageServer,
    params: InlayHintParams,
) -> Result<Option<Vec<InlayHint>>> {
    let _hg = HandlerGuard::new("inlay_hint");
    let uri = &params.text_document.uri;
    let range = &params.range;

    // Skip TSGO while typing — serial TSGO pipeline must stay clear
    // for interactive requests.
    let typing = server.is_typing_cooldown();

    let inlay_enabled = server
        .inlay_hints_enabled
        .load(std::sync::atomic::Ordering::Relaxed);

    // Virtual file: route directly through type provider (positions already in TSX coordinates)
    if !typing && inlay_enabled {
        if let Some(tp) = &server.type_provider {
            if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
                let start = vf_li.position_to_offset(&range.start);
                let end = vf_li.position_to_offset(&range.end);
                if let (Some(so), Some(eo)) = (start, end) {
                    if let Ok(type_hints) = tp.get_inlay_hints(&tsx_path, so, eo).await {
                        let hints: Vec<InlayHint> = type_hints
                            .into_iter()
                            .filter_map(|h| {
                                let pos = vf_li.offset_to_position(h.position)?;
                                let kind = h.kind.map(|k| match k {
                                    crate::type_provider::protocol::InlayHintKind::Type => {
                                        InlayHintKind::TYPE
                                    }
                                    crate::type_provider::protocol::InlayHintKind::Parameter => {
                                        InlayHintKind::PARAMETER
                                    }
                                });
                                Some(InlayHint {
                                    position: pos,
                                    label: InlayHintLabel::String(h.label),
                                    kind,
                                    text_edits: None,
                                    tooltip: None,
                                    padding_left: h.padding_left,
                                    padding_right: h.padding_right,
                                    data: None,
                                })
                            })
                            .collect();
                        return Ok(if hints.is_empty() { None } else { Some(hints) });
                    }
                }
                return Ok(None);
            }
        }
    }

    // Collect Verter-specific hints (DOM queries, useTemplateRef)
    let mut hints: Vec<InlayHint> = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri)?;
        let blocks = scan_sfc_blocks(&doc.source);
        Some(crate::features::inlay_hints::verter_inlay_hints(
            &doc.source,
            &blocks,
            &analysis,
            &doc.line_index,
        ))
    })()
    .unwrap_or_default();

    // Standard .vue file: merge with type provider hints when available.
    // Extract all context synchronously — no DashMap guard held across await.
    if !typing && inlay_enabled {
        if let Some(tp) = &server.type_provider {
            if let Some(ctx) = server.type_provider_context(uri) {
                let start_offset = merge::carrier_position_to_tsx_offset_validated(
                    &range.start,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );
                // Tolerant end mapping: fall back to unvalidated, then TSX EOF.
                // The visible range end often lands in synthetic JSX (generated for
                // HTML elements), which fails validation. Inlay hints tolerate an
                // approximate end bound — only the start must be precise.
                let end_offset = merge::carrier_position_to_tsx_offset_validated(
                    &range.end,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                )
                .or_else(|| {
                    merge::carrier_position_to_tsx_offset(
                        &range.end,
                        &ctx.carrier_line_index,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                    )
                })
                .or_else(|| Some(ctx.tsx_line_index.source_len()));
                if let (Some(so), Some(eo)) = (start_offset, end_offset) {
                    match tp.get_inlay_hints(&ctx.tsx_path, so, eo).await {
                        Ok(type_hints) => {
                            tracing::debug!(
                                "inlay_hint: type provider returned {} hints for {}",
                                type_hints.len(),
                                uri.as_str()
                            );
                            let mut tsgo_hints = merge::merge_inlay_hints(
                                type_hints,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.carrier_line_index,
                            );
                            tracing::debug!(
                                "inlay_hint: {} hints after merge mapping",
                                tsgo_hints.len()
                            );
                            hints.append(&mut tsgo_hints);
                        }
                        Err(e) => {
                            tracing::debug!(
                                "inlay_hint: type provider error for {}: {}",
                                uri.as_str(),
                                e
                            );
                        }
                    }
                } else {
                    tracing::debug!(
                        "inlay_hint: start position mapping failed for {}",
                        uri.as_str()
                    );
                }
            } else {
                tracing::debug!("inlay_hint: no type_provider_context for {}", uri.as_str());
            }
        }
    } else {
        tracing::debug!("inlay_hint: skipped type provider (typing cooldown or disabled)");
    }

    // Deduplicate hints at the same position (prefer type provider hints over Verter placeholders)
    hints.sort_by_key(|h| (h.position.line, h.position.character));
    hints.dedup_by(|a, b| a.position == b.position && a.kind == b.kind);

    Ok(if hints.is_empty() { None } else { Some(hints) })
}

/// Audit-aware wrapper for [`handle_inlay_hint`].
pub(super) async fn handle_inlay_hint_with_audit(
    server: &VerterLanguageServer,
    params: InlayHintParams,
) -> Result<Option<Vec<InlayHint>>> {
    let host = server.documents.host_arc();
    let uri = params.text_document.uri.clone();
    let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), &uri);
    let budget = host.config().lsp_method_timeouts.inlay_hints;
    crate::audit_harness::run_with_audit(
        &host,
        verter_audit::payloads::tags::LspMethodTag::InlayHints,
        canonical_id,
        None,
        budget,
        async move { handle_inlay_hint(server, params).await },
        |payload, value| {
            let count = value.as_ref().map(Vec::len).unwrap_or(0);
            payload.response_size_bytes =
                u32::try_from(count.saturating_mul(64)).unwrap_or(u32::MAX);
        },
    )
    .await
}

pub(super) async fn handle_linked_editing_range(
    server: &VerterLanguageServer,
    params: LinkedEditingRangeParams,
) -> Result<Option<LinkedEditingRanges>> {
    let _hg = HandlerGuard::new("linked_editing");
    let uri = &params.text_document_position_params.text_document.uri;
    let position = &params.text_document_position_params.position;

    let result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        linked_editing_ranges(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
        )
    })();

    Ok(result)
}

pub(super) async fn handle_document_link(
    server: &VerterLanguageServer,
    params: DocumentLinkParams,
) -> Result<Option<Vec<DocumentLink>>> {
    let _hg = HandlerGuard::new("document_link");
    let uri = &params.text_document.uri;

    let links = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let links = build_document_links(&doc.source, &blocks, analysis.as_ref(), &doc.line_index);
        if links.is_empty() {
            None
        } else {
            Some(links)
        }
    })();

    Ok(links)
}

pub(super) async fn handle_document_color(
    server: &VerterLanguageServer,
    params: DocumentColorParams,
) -> Result<Vec<ColorInformation>> {
    let _hg = HandlerGuard::new("document_color");
    let uri = &params.text_document.uri;

    let colors = (|| {
        let doc = server.documents.get(uri)?;
        let blocks = scan_sfc_blocks(&doc.source);
        Some(color_info::document_colors(
            &doc.source,
            &blocks,
            &doc.line_index,
        ))
    })();

    Ok(colors.unwrap_or_default())
}

pub(super) async fn handle_color_presentation(
    _server: &VerterLanguageServer,
    params: ColorPresentationParams,
) -> Result<Vec<ColorPresentation>> {
    let _hg = HandlerGuard::new("color_presentation");
    Ok(color_info::color_presentations(&params.color))
}

pub(super) async fn handle_formatting(
    server: &VerterLanguageServer,
    params: DocumentFormattingParams,
) -> Result<Option<Vec<TextEdit>>> {
    let _hg = HandlerGuard::new("formatting");
    let uri = &params.text_document.uri;

    let edits = (|| {
        let doc = server.documents.get(uri)?;
        let blocks = scan_sfc_blocks(&doc.source);
        let edits = format_document(&doc.source, &blocks, &doc.line_index, &params.options);
        if edits.is_empty() {
            None
        } else {
            Some(edits)
        }
    })();

    Ok(edits)
}

/// Resolve the markup [`CarrierKind`](crate::features::auto_close_tag::CarrierKind)
/// for an on-type-formatting document, or `None` when the document is NOT a Vue
/// or Svelte carrier (a plain script, a virtual file, an unsupported carrier, …).
///
/// The editor `language_id` is authoritative for a framework carrier (an
/// in-memory carrier document may not carry a `.vue` / `.svelte` path), so it is
/// consulted first via the registry; any other document classifies by canonical
/// path through the host's static classifier — the same resolution
/// `DocumentRegistry::document_file_language` performs. Only the built-in Vue /
/// Svelte CARRIER rows map to a `CarrierKind`; everything else returns `None` and
/// the on-type handler emits no edit.
fn carrier_kind_for_on_type(
    language_id: &str,
    canonical_id: &str,
) -> Option<crate::features::auto_close_tag::CarrierKind> {
    use crate::features::auto_close_tag::CarrierKind;
    let language = verter_session::LanguageRegistry::global()
        .carrier_for_editor_language_id(language_id)
        .unwrap_or_else(|| {
            verter_session::LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution()
        });
    // Carrier-generic routing (no Vue-only `.is_vue()` predicate — the carrier
    // routing guard bans it). A non-carrier document (plain script / template
    // row / unknown) has no markup region. Of the built-in CARRIERS, Svelte is
    // resolved via the allowlisted carrier check; the only OTHER built-in markup
    // carrier today is the Vue SFC, so a framework carrier that is not Svelte is
    // the Vue carrier. A third markup carrier would need its own explicit arm
    // here (and its own `CarrierKind`), not a silent fall-through.
    if !language.is_framework_carrier() {
        None
    } else if language.is_svelte() {
        Some(CarrierKind::Svelte)
    } else {
        Some(CarrierKind::Vue)
    }
}

pub(super) async fn handle_on_type_formatting(
    server: &VerterLanguageServer,
    params: DocumentOnTypeFormattingParams,
) -> Result<Option<Vec<TextEdit>>> {
    let _hg = HandlerGuard::new("on_type_formatting");
    let uri = &params.text_document_position.text_document.uri;
    let position = &params.text_document_position.position;

    let edits = (|| {
        let doc = server.documents.get(uri)?;

        // Proactive tag auto-close is a MARKUP feature. It must fire only for a
        // framework CARRIER document (Vue / Svelte) and only inside that
        // carrier's template/markup region — never in a plain `.ts` / `.js`
        // document (where `formatOnType` may be globally enabled and a `>` is a
        // TS-generic close, not a tag), and never inside the carrier's
        // `<script>` / `<style>` blocks. The carrier kind is resolved from the
        // document's authoritative editor `language_id`; the region gate lives
        // in `auto_close_tag_in_carrier`.
        let carrier = carrier_kind_for_on_type(&doc.language_id, &doc.canonical_id)?;

        let offset = doc.line_index.position_to_offset(position)? as usize;
        let snippet = crate::features::auto_close_tag::auto_close_tag_in_carrier(
            &doc.source,
            offset,
            carrier,
        )?;

        // Insert the closing tag text right at the cursor position (after the `>`)
        // The `$0` cursor marker is for snippet-capable clients; for the TextEdit
        // we just strip it and insert plain text.
        let plain_text = snippet.replace("$0", "");
        Some(vec![TextEdit {
            range: Range::new(*position, *position),
            new_text: plain_text,
        }])
    })();

    Ok(edits)
}

pub(super) async fn handle_symbol(
    server: &VerterLanguageServer,
    params: WorkspaceSymbolParams,
) -> Result<Option<WorkspaceSymbolResponse>> {
    let _hg = HandlerGuard::new("workspace_symbol");
    let symbols = workspace_symbols(&server.documents.host, &params.query);
    Ok(if symbols.is_empty() {
        None
    } else {
        Some(symbols.into())
    })
}

pub(super) async fn handle_prepare_call_hierarchy(
    server: &VerterLanguageServer,
    params: CallHierarchyPrepareParams,
) -> Result<Option<Vec<CallHierarchyItem>>> {
    let _hg = HandlerGuard::new("prepare_call_hierarchy");
    let uri = &params.text_document_position_params.text_document.uri;
    let position = &params.text_document_position_params.position;

    let result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        call_hierarchy::prepare_call_hierarchy(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
            uri,
        )
    })();

    Ok(result)
}

pub(super) async fn handle_incoming_calls(
    server: &VerterLanguageServer,
    params: CallHierarchyIncomingCallsParams,
) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
    let _hg = HandlerGuard::new("incoming_calls");
    let uri = &params.item.uri;

    let calls = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        Some(call_hierarchy::incoming_calls(
            &params.item,
            &doc.source,
            analysis.as_ref(),
            &doc.line_index,
            uri,
        ))
    })();

    match calls {
        Some(v) if !v.is_empty() => Ok(Some(v)),
        _ => Ok(None),
    }
}

pub(super) async fn handle_outgoing_calls(
    server: &VerterLanguageServer,
    params: CallHierarchyOutgoingCallsParams,
) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
    let _hg = HandlerGuard::new("outgoing_calls");
    let uri = &params.item.uri;

    let calls = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        Some(call_hierarchy::outgoing_calls(
            &params.item,
            analysis.as_ref(),
            &doc.line_index,
            uri,
        ))
    })();

    match calls {
        Some(v) if !v.is_empty() => Ok(Some(v)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod on_type_gate_tests {
    use super::carrier_kind_for_on_type;
    use crate::features::auto_close_tag::CarrierKind;

    /// BLOCKER 1: the on-type auto-close gate must resolve a carrier ONLY for a
    /// framework carrier document. A plain `.ts` / `.js` / `.tsx` document (where
    /// `editor.formatOnType` may be globally enabled) resolves to `None`, so the
    /// handler emits no edit and a TS-generic `>` is never turned into `</...>`.
    #[test]
    fn non_carrier_languages_resolve_to_no_carrier() {
        for (lang, path) in [
            ("typescript", "file:///proj/src/util.ts"),
            ("javascript", "file:///proj/src/util.js"),
            ("typescriptreact", "file:///proj/src/App.tsx"),
            ("javascriptreact", "file:///proj/src/App.jsx"),
            ("plaintext", "file:///proj/notes.txt"),
        ] {
            assert_eq!(
                carrier_kind_for_on_type(lang, path),
                None,
                "`{lang}` is not a markup carrier — auto-close must not engage",
            );
        }
    }

    /// The Vue carrier `language_id` resolves to `CarrierKind::Vue`, authoritative
    /// even for an in-memory carrier whose canonical id is not a `.vue` path.
    #[test]
    fn vue_language_id_resolves_to_vue_carrier() {
        assert_eq!(
            carrier_kind_for_on_type("vue", "file:///proj/src/App.vue"),
            Some(CarrierKind::Vue),
        );
        // language_id is authoritative even without a `.vue` extension.
        assert_eq!(
            carrier_kind_for_on_type("vue", "untitled:Untitled-1"),
            Some(CarrierKind::Vue),
        );
    }

    /// The Svelte carrier `language_id` resolves to `CarrierKind::Svelte`.
    #[test]
    fn svelte_language_id_resolves_to_svelte_carrier() {
        assert_eq!(
            carrier_kind_for_on_type("svelte", "file:///proj/src/App.svelte"),
            Some(CarrierKind::Svelte),
        );
    }

    /// A `.vue` / `.svelte` path still classifies as its carrier when the editor
    /// `language_id` is unhelpful (the canonical-path fallback), so an upgrade
    /// path that loses the carrier `language_id` does not silently disable the
    /// markup gate.
    #[test]
    fn carrier_path_fallback_classifies_when_language_id_is_generic() {
        assert_eq!(
            carrier_kind_for_on_type("plaintext", "file:///proj/src/App.vue"),
            Some(CarrierKind::Vue),
        );
        assert_eq!(
            carrier_kind_for_on_type("plaintext", "file:///proj/src/App.svelte"),
            Some(CarrierKind::Svelte),
        );
    }
}

#[cfg(test)]
mod code_action_diag_ctx_tests {
    use super::{build_provider_diagnostic_contexts, parse_diagnostic_code};
    use crate::documents::line_index::LineIndex;
    use crate::documents::position_map::PositionMapper;
    use crate::documents::provider_projection::ProviderPositionMapper;
    use tower_lsp_server::ls_types::{Diagnostic, NumberOrString, Position, Range};

    // ── parse_diagnostic_code: Number / String / non-numeric / missing ──────

    /// Verter publishes TS codes as strings (`code: String("6133")`); the handler
    /// must parse that decimal string to the integer 6133 the code-fix path needs.
    #[test]
    fn parses_string_code_to_integer() {
        assert_eq!(
            parse_diagnostic_code(Some(&NumberOrString::String("6133".to_string()))),
            Some(6133),
        );
    }

    /// A numeric `code` is taken directly when it fits u32.
    #[test]
    fn parses_number_code_directly() {
        assert_eq!(
            parse_diagnostic_code(Some(&NumberOrString::Number(6133))),
            Some(6133),
        );
    }

    /// A non-numeric string code is dropped (fail-closed) — never forwarded as a
    /// guessed/0 code.
    #[test]
    fn drops_non_numeric_string_code() {
        assert_eq!(
            parse_diagnostic_code(Some(&NumberOrString::String("notanumber".to_string()))),
            None,
        );
        // An empty string is likewise non-numeric.
        assert_eq!(
            parse_diagnostic_code(Some(&NumberOrString::String(String::new()))),
            None,
        );
    }

    /// A missing code is dropped.
    #[test]
    fn drops_missing_code() {
        assert_eq!(parse_diagnostic_code(None), None);
    }

    /// A negative numeric code does not fit u32 and is dropped (no wraparound).
    #[test]
    fn drops_negative_number_code() {
        assert_eq!(
            parse_diagnostic_code(Some(&NumberOrString::Number(-1))),
            None,
        );
    }

    // ── build_provider_diagnostic_contexts: parse + map + fail-closed ────────

    /// An identity-mapped carrier/TSX (same text, 1:1 source map) lets us assert
    /// the full pipeline: a `String("6133")`-coded diagnostic over a mapped range
    /// yields a `ProviderDiagnosticContext { code: 6133, .. }` with a real TSX
    /// span; a non-numeric-coded diagnostic in the same batch is dropped.
    fn identity_mapping() -> (ProviderPositionMapper, LineIndex, LineIndex) {
        let src = "const foo = 1;\nconst bar = 2;\n";
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let source_id = builder.set_source_and_content("App.vue", src);
        // 1:1 tokens for both lines so carrier positions map straight through.
        builder.add_token(0, 0, 0, 0, Some(source_id), None);
        builder.add_token(1, 0, 1, 0, Some(source_id), None);
        let json = builder.into_sourcemap().to_json_string();
        let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
        let li = LineIndex::new_utf16(src);
        let tsx_li = LineIndex::new_utf16(src);
        (mapper, li, tsx_li)
    }

    fn diag_at(line: u32, start_char: u32, end_char: u32, code: NumberOrString) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: start_char,
                },
                end: Position {
                    line,
                    character: end_char,
                },
            },
            code: Some(code),
            ..Default::default()
        }
    }

    #[test]
    fn threads_6133_and_drops_non_numeric() {
        let (mapper, carrier_li, tsx_li) = identity_mapping();
        let diags = vec![
            // `const foo` on line 0, cols 0..9 — a TS6133 published as a string.
            diag_at(0, 0, 9, NumberOrString::String("6133".to_string())),
            // A non-numeric code in the same batch must be dropped.
            diag_at(1, 0, 9, NumberOrString::String("oops".to_string())),
        ];
        let ctxs = build_provider_diagnostic_contexts(&diags, &carrier_li, &mapper, &tsx_li);
        assert_eq!(
            ctxs.len(),
            1,
            "only the numeric-coded diagnostic should be threaded; got {ctxs:?}"
        );
        assert_eq!(
            ctxs[0].code, 6133,
            "the parsed code must be the integer 6133"
        );
        // The mapped TSX span covers `const foo` (byte 0..9 on the identity map).
        assert_eq!(ctxs[0].start, 0);
        assert_eq!(ctxs[0].end, 9);
    }

    /// An empty diagnostics list yields an empty context list (the provider call
    /// then short-circuits).
    #[test]
    fn empty_diagnostics_yield_empty_contexts() {
        let (mapper, carrier_li, tsx_li) = identity_mapping();
        let ctxs = build_provider_diagnostic_contexts(&[], &carrier_li, &mapper, &tsx_li);
        assert!(ctxs.is_empty());
    }

    /// Architect ruling (UPHELD): the forwarding is GENERIC over numeric codes,
    /// NOT hardcoded to 6133. A non-6133 numeric code (e.g. TS2304
    /// "cannot find name") over a mappable range IS forwarded to the provider,
    /// carrying that exact integer code — while a native Verter string code
    /// (`"verter/..."`) is filtered out by the numeric parse.
    ///
    /// Discriminating two ways: a regression that hardcoded `code == 6133` would
    /// DROP the 2304 context (so `ctxs` would be empty / the code assertion
    /// fails); a regression that forwarded non-numeric Verter codes would let the
    /// `"verter/..."` diagnostic through (so the length would be 2). Both the
    /// `Number(2304)` and the `String("2304")` spellings are exercised.
    #[test]
    fn forwards_arbitrary_numeric_code_and_drops_verter_string_code() {
        let (mapper, carrier_li, tsx_li) = identity_mapping();
        let diags = vec![
            // A non-6133 numeric code published as a decimal STRING (the editor form).
            diag_at(0, 0, 9, NumberOrString::String("2304".to_string())),
            // The same generic path published as a raw NUMBER on line 1.
            diag_at(1, 0, 9, NumberOrString::Number(2304)),
            // A native Verter rule code (string, non-numeric) must NOT be forwarded.
            diag_at(
                0,
                0,
                9,
                NumberOrString::String("verter/some-rule".to_string()),
            ),
        ];
        let ctxs = build_provider_diagnostic_contexts(&diags, &carrier_li, &mapper, &tsx_li);
        assert_eq!(
            ctxs.len(),
            2,
            "both numeric-coded diagnostics (string + number spelling) are forwarded; \
             the verter/ string code is dropped — got {ctxs:?}"
        );
        assert!(
            ctxs.iter().all(|c| c.code == 2304),
            "the path forwards the ACTUAL integer code (2304), proving it is generic, \
             not 6133-locked — got {ctxs:?}"
        );
        // No forwarded context may carry a sentinel/zero code from a non-numeric source.
        assert!(
            !ctxs.iter().any(|c| c.code == 0),
            "a non-numeric verter/ code must never be forwarded as a 0 sentinel — got {ctxs:?}"
        );
    }
}
