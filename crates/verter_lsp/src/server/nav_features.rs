//! LSP navigation feature method bodies (phase 11e.9).
//!
//! Free functions hosting the bodies of `impl LanguageServer for
//! VerterLanguageServer` navigation feature methods (hover,
//! completion, completion_resolve, goto_definition,
//! goto_type_definition, references, prepare_rename, rename) plus
//! the relocated `enrich_hover_with_provenance` inherent helper and
//! the `append_markdown` private helper used by hover provenance
//! enrichment.
//!
//! The trait impl block stays in `mod.rs`; each trait method is a
//! 1-line stub that delegates to the corresponding `handle_<method>`
//! free function here.

use std::sync::Arc;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::features::completion::completions_at_position;
use crate::features::cursor_context::{
    classify_cursor_context, classify_expression_context_with_trigger, CursorContext,
    ExpressionContext, TemplateCursorContext,
};
use crate::features::definition::definition_at_position;
use crate::features::hover;
use crate::features::hover::hover_at_position;
use crate::features::references::references_at_position;
use crate::features::rename::{prepare_rename, rename_at_position};
use crate::tsgo::merge;

use super::handler_guard::HandlerGuard;
use super::server_utils::*;
use super::VerterLanguageServer;

pub(super) async fn handle_hover(
    server: &VerterLanguageServer,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let _hg = HandlerGuard::new("hover");
    let uri = &params.text_document_position_params.text_document.uri;
    let position = &params.text_document_position_params.position;
    tracing::info!(
        "hover ENTER {} at {}:{}",
        uri.as_str(),
        position.line,
        position.character
    );
    let _timer = server
        .statistics
        .timer("hover", Some(uri.as_str().to_string()));

    // Virtual file: route directly through TSGO (position is already in TSX coordinates)
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(Some(info)) = tp.get_hover(&tsx_path, offset).await {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: info.contents,
                        }),
                        range: None,
                    }));
                }
            }
            return Ok(None);
        }
    }

    let ssr_context = {
        let canonical_id = server.documents.get_canonical_id(uri);
        canonical_id
            .as_deref()
            .map(|cid| server.is_ssr_context(cid))
            .unwrap_or(false)
    };

    let verter_full = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        hover_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
            ssr_context,
        )
    })();
    let vue_kind_label = verter_full.as_ref().and_then(|r| r.vue_kind_label.clone());
    let verter_result = verter_full.map(|r| r.hover);

    let child_hover_target = (|| {
        let analysis = server.documents.get_analysis(uri)?;
        let doc = server.documents.get(uri)?;
        let vue_offset = doc.line_index.position_to_offset(position)?;
        hover::child_hover_target_at_offset(vue_offset, &doc.source, &analysis)
    })();
    if let Some(target) = child_hover_target.as_ref() {
        if let Some(child_hover) = server.child_hover_for_target(uri, target) {
            return Ok(Some(child_hover));
        }
    }

    // Slot syntax: verter provides rich hover; type provider returns unhelpful
    // generic types (`() any`, `string`). Skip type provider merge entirely.
    if verter_result.is_some() {
        if let Some(analysis) = server.documents.get_analysis(uri) {
            if let Some(doc) = server.documents.get(uri) {
                if let Some(vue_offset) = doc.line_index.position_to_offset(position) {
                    if hover::is_on_slot_syntax(vue_offset, &analysis) {
                        return Ok(verter_result);
                    }
                }
            }
        }
    }

    // Enhance with TypeProvider if available.
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        if let Some(ctx) = server.type_provider_context(uri) {
            // Use validated mapping to avoid querying TSGO at synthetic TSX
            // positions (e.g., <div> → generated JSX) which can crash it.
            let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            );

            let type_hover = if let Some(tsx_offset) = tsx_offset {
                // Log TSX context snippet around the hover offset for debugging
                if let Some((before, after)) = debug_snippet(&ctx.tsx_content, tsx_offset as usize)
                {
                    tracing::info!(
                        "hover TSX context at offset {}: «{}⸽{}»",
                        tsx_offset,
                        before.replace('\n', "↵"),
                        after.replace('\n', "↵"),
                    );
                }
                match tp.get_hover(&ctx.tsx_path, tsx_offset).await {
                    Ok(hover) => {
                        tracing::info!(
                            "hover type provider result: {}",
                            if hover.is_some() {
                                hover
                                    .as_ref()
                                    .map(|h| h.contents.as_str())
                                    .unwrap_or("Some(empty)")
                            } else {
                                "None"
                            }
                        );
                        hover
                    }
                    Err(e) => {
                        tracing::warn!("hover type provider error: {}", e);

                        None
                    }
                }
            } else {
                tracing::info!(
                    "hover: vue_to_tsx validation failed for {}:{} — position is in synthetic TSX region",
                    position.line,
                    position.character
                );
                None
            };

            // If TSGO returned a result, merge and return.
            if type_hover.is_some() {
                return Ok(merge::merge_hover(
                    verter_result,
                    type_hover,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                    &ctx.vue_line_index,
                    vue_kind_label.as_deref(),
                ));
            }

            // Redirect: when TSGO returned nothing and the cursor is on a static
            // `class`/`style` attribute that was merged with a dynamic binding,
            // the static attribute's source position maps to removed TSX content.
            // Retry at the dynamic directive's position instead.
            if let Some(analysis) = server.documents.get_analysis(uri) {
                let vue_offset = ctx.vue_line_index.position_to_offset(position);
                if let Some(vue_offset) = vue_offset {
                    if let Some(redirect_offset) =
                        hover::merged_attribute_redirect_offset(vue_offset, &analysis)
                    {
                        // Convert the redirect SFC offset to a Vue line:col position
                        if let Some(redirect_pos) =
                            ctx.vue_line_index.offset_to_position(redirect_offset)
                        {
                            if let Some(redirect_tsx) = merge::vue_position_to_tsx_offset_validated(
                                &redirect_pos,
                                &ctx.vue_line_index,
                                &ctx.mapper,
                                &ctx.tsx_line_index,
                            ) {
                                tracing::info!(
                                    "hover: redirecting merged class/style from vue offset {} to {} (tsx offset {})",
                                    vue_offset, redirect_offset, redirect_tsx
                                );
                                if let Ok(redirect_hover) =
                                    tp.get_hover(&ctx.tsx_path, redirect_tsx).await
                                {
                                    return Ok(merge::merge_hover(
                                        verter_result,
                                        redirect_hover,
                                        &ctx.mapper,
                                        &ctx.tsx_line_index,
                                        &ctx.vue_line_index,
                                        vue_kind_label.as_deref(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            return Ok(merge::merge_hover(
                verter_result,
                None,
                &ctx.mapper,
                &ctx.tsx_line_index,
                &ctx.vue_line_index,
                vue_kind_label.as_deref(),
            ));
        } else {
            tracing::info!("hover: no ide_context for {}", uri.as_str());
        }
    } else {
        tracing::info!("hover: no type_provider");
    }

    // Plan §3 Commit 9 — provenance enrichment on the primary
    // verter-only return path. Early returns (virtual file,
    // child-hover, type-provider merge) intentionally skip
    // enrichment for now; the opt-in feature targets the
    // common "verter-only hover on a Vue binding" case.
    Ok(enrich_hover_with_provenance(
        server,
        uri,
        position,
        verter_result,
    ))
}

pub(super) async fn handle_completion(
    server: &VerterLanguageServer,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let _hg = HandlerGuard::new("completion");
    let uri = &params.text_document_position.text_document.uri;
    let _timer = server
        .statistics
        .timer("completion", Some(uri.as_str().to_string()));
    // Increment the generation counter so stale requests can detect they've been superseded.
    let completion_gen = server
        .completion_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let position = &params.text_document_position.position;
    let trigger_character = params
        .context
        .as_ref()
        .and_then(|ctx| ctx.trigger_character.as_deref());
    tracing::info!(
        "completion ENTER {} at {}:{} (trigger={:?})",
        uri.as_str(),
        position.line,
        position.character,
        trigger_character
    );

    // Check coalescing — skip stale requests superseded by newer keystrokes.
    if server
        .completion_generation
        .load(std::sync::atomic::Ordering::Relaxed)
        != completion_gen + 1
    {
        tracing::debug!(
            "completion: skipping stale request (gen {})",
            completion_gen
        );
        return Ok(None);
    }

    // NOTE: We do NOT call ensure_provider_synced here.  The debounced sync in
    // did_change sends the update to TSGO within 50ms of the last keystroke.
    // Flushing inline would serialize: sync → TSGO re-analysis → get_completions,
    // which takes 2-3s on large files and blocks the entire completion pipeline.
    // Instead we let TSGO answer with whatever version it has; if it's stale the
    // response arrives fast and VS Code re-requests after the debounce fires.

    // Virtual file: route directly through TSGO
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(result) = tp
                    .get_completions(&tsx_path, offset, trigger_character)
                    .await
                {
                    let items: Vec<CompletionItem> = result
                        .items
                        .into_iter()
                        .filter(|c| {
                            !c.label.starts_with("___VERTER___") && !c.label.starts_with("$V_")
                        })
                        .map(|c| CompletionItem {
                            label: c.label,
                            detail: c.detail,
                            documentation: c.documentation.map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d,
                                })
                            }),
                            sort_text: c.sort_text,
                            ..Default::default()
                        })
                        .collect();
                    return Ok(if items.is_empty() {
                        None
                    } else {
                        Some(CompletionResponse::List(CompletionList {
                            is_incomplete: result.is_incomplete,
                            items,
                        }))
                    });
                }
            }
            return Ok(None);
        }
    }

    let completion_ssr_context = {
        let canonical_id = server.documents.get_canonical_id(uri);
        canonical_id
            .as_deref()
            .map(|cid| server.is_ssr_context(cid))
            .unwrap_or(false)
    };

    let verter_result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let canonical_id = crate::documents::uri_to_canonical_id(uri);
        let resolve_component = |import_source: &str,
                                 component_name: Option<&str>|
         -> Option<verter_session::FileAnalysisSnapshot> {
            let try_follow_reexport = |resolved: &str,
                                       comp_name: Option<&str>|
             -> Option<verter_session::FileAnalysisSnapshot> {
                if resolved.ends_with(".vue") {
                    return server.documents.host().get_analysis(resolved);
                }
                // Ensure the barrel file is loaded so we can inspect its exports
                if server.documents.host().get_analysis(resolved).is_none() {
                    server.documents.host().ensure_loaded(resolved);
                }
                // For non-.vue files (barrel/index), follow re-export chains if we know the component name
                if let Some(name) = comp_name {
                    if let Some((terminal_id, _, _)) = server
                        .documents
                        .host()
                        .get_export_span_follow_reexports(resolved, name)
                    {
                        if terminal_id.ends_with(".vue") {
                            // Ensure the terminal .vue file is compiled
                            if server.documents.host().get_analysis(&terminal_id).is_none() {
                                server.documents.host().ensure_loaded(&terminal_id);
                            }
                            return server.documents.host().get_analysis(&terminal_id);
                        }
                    }
                }
                server.documents.host().get_analysis(resolved)
            };

            // Try 1: Use resolve_import_specifier (handles relative, alias, index files)
            if let Some(resolved) = server.resolve_import_specifier(&canonical_id, import_source) {
                if let Some(a) = try_follow_reexport(&resolved, component_name) {
                    return Some(a);
                }
            }

            // Try 2: Manual relative resolution (fallback for host-cached files not on disk)
            if import_source.starts_with('.') {
                let parts: Vec<&str> = canonical_id.split('/').collect();
                let dir = parts[..parts.len().saturating_sub(1)].join("/");
                let resolved = if let Some(stripped) = import_source.strip_prefix("./") {
                    format!("{}/{}", dir, stripped)
                } else if import_source.starts_with("../") {
                    let mut dir_parts: Vec<&str> = dir.split('/').collect();
                    let mut rel = import_source;
                    while let Some(rest) = rel.strip_prefix("../") {
                        dir_parts.pop();
                        rel = rest;
                    }
                    format!(
                        "{}/{}",
                        dir_parts.join("/"),
                        rel.strip_prefix("./").unwrap_or(rel)
                    )
                } else {
                    format!("{}/{}", dir, import_source)
                };
                if let Some(a) = try_follow_reexport(&resolved, component_name) {
                    return Some(a);
                }
            }

            // Try 3: VFS resolution (path aliases, tsconfig paths, disk probing)
            if let Some(resolved_path) =
                server.resolve_import_specifier(&canonical_id, import_source)
            {
                if let Some(a) = try_follow_reexport(&resolved_path, component_name) {
                    return Some(a);
                }
            }

            // Try 4: Direct lookup (bare specifiers, already-resolved)
            try_follow_reexport(import_source, component_name)
        };
        // Build workspace component list for auto-import
        let ws_components = build_workspace_components(&server.documents.host, &canonical_id);
        completions_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
            Some(&resolve_component),
            if ws_components.is_empty() {
                None
            } else {
                Some(&ws_components)
            },
            Some(uri.as_str()),
            completion_ssr_context,
        )
    })();

    let verter_is_incomplete = verter_result
        .as_ref()
        .map(|r| r.is_incomplete)
        .unwrap_or(false);
    let verter_items = verter_result.map(|r| r.items);

    // Compute cursor context once — derive attribute vs expression context
    let (is_template_attr_context, in_expression_context) = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let offset = doc.line_index.position_to_offset(position)?;
        let context = classify_cursor_context(offset, &doc.source, &blocks, analysis.as_ref());
        Some(match &context {
            CursorContext::Template(TemplateCursorContext::AttributeName { .. }) => (true, false),
            CursorContext::Template(
                TemplateCursorContext::Expression { .. } | TemplateCursorContext::Interpolation,
            ) => (false, true),
            CursorContext::Template(_) => (false, false),
            _ => (false, false),
        })
    })()
    .unwrap_or((false, false));

    // Enhance with TypeProvider if available.
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        if matches!(server.type_provider_kind, crate::TypeProviderKind::Tsserver)
            && server.current_file_needs_inline_type_provider_sync(uri)
        {
            tracing::debug!(
                "completion: repairing current-file tsserver sync for {}",
                uri.as_str()
            );
            server.ensure_current_file_synced(uri).await;
        }
        server.ensure_imported_vue_apis_synced(uri).await;
        let ctx = server.type_provider_context(uri);
        if ctx.is_none() {
            tracing::debug!("completion: no ide_context for {}", uri.as_str());
        }
        if let Some(ctx) = ctx {
            // TSX is always fresh in the type provider — synced eagerly in did_change.
            // Only DTS sync and diagnostics publishing are debounced (300ms via SyncCoordinator).

            let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            );
            if tsx_offset.is_none() {
                tracing::debug!(
                    "completion: position mapping failed for {}:{},{}",
                    uri.as_str(),
                    position.line,
                    position.character,
                );
            }
            // Template completion has two complementary flags based on expression context:
            //
            // 1. `suppress_verter`: In MemberAccess/Literal/Type/PropertyKey contexts,
            //    verter's identifier-level completions are irrelevant — only the TypeProvider
            //    knows the object's members. So we suppress verter items.
            //
            // 2. `skip_type_provider`: In IdentifierExpected context, the TypeProvider
            //    returns ALL globals in scope (AbortController, HTMLElement, Array, etc.)
            //    which are NOT accessible in Vue template expressions (templates use a
            //    render proxy that only exposes script setup bindings). Verter's
            //    template_completions() already provides exactly the right set.
            //
            // | ExpressionContext    | suppress_verter | skip_type_provider |
            // |----------------------|-----------------|--------------------|
            // | IdentifierExpected   | false           | true               |
            // | MemberAccess         | true            | false              |
            // | Literal/Type/PropKey | true            | false              |
            // | Unknown              | false           | false (filtered)   |
            let expr_context = if in_expression_context {
                tsx_offset.map(|off| {
                    classify_expression_context_with_trigger(
                        &ctx.tsx_content,
                        off as usize,
                        trigger_character,
                    )
                })
            } else {
                None
            };

            let suppress_verter = expr_context
                .as_ref()
                .map(|ec| {
                    matches!(
                        ec,
                        ExpressionContext::MemberAccess
                            | ExpressionContext::Literal
                            | ExpressionContext::TypePosition
                            | ExpressionContext::PropertyKey
                    )
                })
                .unwrap_or(false);

            if let Some(tsx_offset) = tsx_offset {
                let identifier_prefix = expr_context.as_ref().and_then(|ec| {
                    matches!(
                        ec,
                        ExpressionContext::IdentifierExpected | ExpressionContext::Unknown
                    )
                    .then(|| identifier_prefix_before_offset(&ctx.tsx_content, tsx_offset as usize))
                    .flatten()
                    .map(str::to_string)
                });

                let skip_type_provider = expr_context
                    .as_ref()
                    .map(|ec| {
                        matches!(ec, ExpressionContext::IdentifierExpected)
                            && identifier_prefix.is_none()
                    })
                    .unwrap_or(false);

                if skip_type_provider {
                    tracing::debug!(
                        "completion: skipping type provider for IdentifierExpected context"
                    );
                    return Ok(verter_items.map(|items| {
                        CompletionResponse::List(CompletionList {
                            is_incomplete: verter_is_incomplete,
                            items,
                        })
                    }));
                }
                // Only forward trigger characters that tsserver/TSGO recognize.
                // Vue-specific triggers (":", "@", " ") are handled by Verter's
                // native completions and cause tsserver errors if forwarded.
                let tp_trigger = trigger_character
                    .filter(|t| matches!(*t, "." | "\"" | "'" | "`" | "/" | "<"))
                    .or_else(|| {
                        (matches!(expr_context, Some(ExpressionContext::MemberAccess))
                            && is_immediately_after_member_access_dot(
                                &ctx.tsx_content,
                                tsx_offset as usize,
                            ))
                        .then_some(".")
                    });
                let mut type_completion_result = tp
                    .get_completions(&ctx.tsx_path, tsx_offset, tp_trigger)
                    .await;
                if matches!(server.type_provider_kind, crate::TypeProviderKind::Tsserver) {
                    for retry_delay_ms in [50u64, 150, 300] {
                        let needs_retry = matches!(
                            type_completion_result,
                            Err(ref error) if error.message.contains("No content available")
                        );
                        if !needs_retry {
                            break;
                        }
                        tracing::debug!(
                            "completion: retrying tsserver completion after no-content error for {} (delay={}ms)",
                            ctx.tsx_path,
                            retry_delay_ms
                        );
                        server.force_reopen_current_file_in_type_provider(uri).await;
                        server.sync_api_to_provider(uri).await;
                        server.ensure_imported_vue_apis_synced(uri).await;
                        tokio::time::sleep(std::time::Duration::from_millis(retry_delay_ms)).await;
                        type_completion_result = tp
                            .get_completions(&ctx.tsx_path, tsx_offset, tp_trigger)
                            .await;
                    }
                }
                match type_completion_result {
                    Ok(mut type_result) => {
                        tracing::debug!(
                            "completion: type provider returned {} items (incomplete={})",
                            type_result.items.len(),
                            type_result.is_incomplete
                        );

                        filter_type_provider_completion_result(
                            &mut type_result,
                            expr_context.as_ref(),
                            identifier_prefix.as_deref(),
                            verter_items.as_ref(),
                        );

                        if matches!(expr_context, Some(ExpressionContext::MemberAccess))
                            && tp_trigger == Some(".")
                            && type_result.items.is_empty()
                        {
                            tracing::debug!(
                                "completion: retrying member access without dot trigger after empty backend result"
                            );
                            if let Ok(mut retry_result) =
                                tp.get_completions(&ctx.tsx_path, tsx_offset, None).await
                            {
                                filter_type_provider_completion_result(
                                    &mut retry_result,
                                    expr_context.as_ref(),
                                    identifier_prefix.as_deref(),
                                    verter_items.as_ref(),
                                );
                                if !retry_result.items.is_empty() {
                                    type_result = retry_result;
                                }
                            }
                        }

                        let (merged, is_incomplete) = merge::merge_completions(
                            if suppress_verter {
                                Vec::new()
                            } else {
                                verter_items.unwrap_or_default()
                            },
                            type_result,
                            &ctx.mapper,
                            &ctx.tsx_line_index,
                            &ctx.vue_line_index,
                            Some(&ctx.tsx_path),
                            is_template_attr_context,
                        );
                        return Ok(if merged.is_empty() {
                            None
                        } else {
                            Some(CompletionResponse::List(CompletionList {
                                is_incomplete: is_incomplete || verter_is_incomplete,
                                items: merged,
                            }))
                        });
                    }
                    Err(e) => {
                        tracing::warn!("completion: type provider error: {e}");
                    }
                }
            }
        }
    } else {
        tracing::debug!("completion: no type provider available");
    }

    Ok(verter_items.map(|items| {
        CompletionResponse::List(CompletionList {
            is_incomplete: verter_is_incomplete,
            items,
        })
    }))
}

pub(super) async fn handle_completion_resolve(
    server: &VerterLanguageServer,
    mut item: CompletionItem,
) -> Result<CompletionItem> {
    let _hg = HandlerGuard::new("completion_resolve");
    // Check if this item requires auto-import (verter workspace components)
    if let Some(ref data) = item.data {
        if data.get("auto_import").and_then(|v| v.as_bool()) == Some(true) {
            if let (Some(import_path), Some(component_name), Some(doc_uri)) = (
                data.get("import_path").and_then(|v| v.as_str()),
                data.get("component_name").and_then(|v| v.as_str()),
                data.get("uri").and_then(|v| v.as_str()),
            ) {
                if let Some(edit) =
                    server.build_auto_import_edit(doc_uri, component_name, import_path)
                {
                    item.additional_text_edits = Some(vec![edit]);
                }
            }
        }

        // Check if this item is from TSGO and needs resolve for auto-import
        if data.get("tsgo").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(tp) = &server.type_provider {
                if let (Some(tsx_path), Some(original_data)) = (
                    data.get("tsx_path").and_then(|v| v.as_str()),
                    data.get("original_data"),
                ) {
                    // Only call resolve if original_data is not null
                    if !original_data.is_null() {
                        if let Ok(Some(resolve_result)) =
                            tp.resolve_completion(tsx_path, original_data.clone()).await
                        {
                            if !resolve_result.additional_text_edits.is_empty() {
                                // Map TSX positions to Vue positions
                                if let Some((_, tsx_content, mapper)) =
                                    server.ide_context_by_path(tsx_path)
                                {
                                    let tsx_li =
                                        LineIndex::new(&tsx_content, server.documents.encoding());
                                    // Find the Vue URI from tsx_path
                                    if let Some(vue_uri) = server.vue_uri_from_ide_path(tsx_path) {
                                        if let Some(doc) = server.documents.get(&vue_uri) {
                                            let edits: Vec<TextEdit> = resolve_result
                                                .additional_text_edits
                                                .iter()
                                                .filter_map(|e| {
                                                    let range = merge::tsx_range_to_vue_range(
                                                        e.start,
                                                        e.end,
                                                        &tsx_li,
                                                        &mapper,
                                                        &doc.line_index,
                                                    )?;
                                                    Some(TextEdit {
                                                        range,
                                                        new_text: e.new_text.clone(),
                                                    })
                                                })
                                                .collect();
                                            if !edits.is_empty() {
                                                item.additional_text_edits = Some(edits);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(item)
}

pub(super) async fn handle_goto_definition(
    server: &VerterLanguageServer,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let _hg = HandlerGuard::new("goto_definition");
    let uri = &params.text_document_position_params.text_document.uri;
    let _timer = server
        .statistics
        .timer("definition", Some(uri.as_str().to_string()));
    let position = &params.text_document_position_params.position;
    tracing::debug!(
        "definition: {} at {}:{}",
        uri.as_str(),
        position.line,
        position.character
    );

    server.ensure_provider_synced(uri).await;

    // Virtual file: route directly through TSGO (position is already in TSX coordinates)
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_defs) = tp.get_definition(&tsx_path, offset).await {
                    let locations: Vec<Location> = type_defs
                        .into_iter()
                        .filter_map(|d| {
                            // Strip virtual suffixes so user navigates to .vue
                            let vue_source_exists =
                                |p: &str| server.documents.host().get_source(p).is_some();
                            let target_path =
                                merge::normalize_vue_path_owned(&d.path, &vue_source_exists);
                            let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                            // Convert byte offsets to positions using vf LineIndex for
                            // same-file refs; for external files, fall back to 0:0
                            let range = if d.path == tsx_path {
                                Range {
                                    start: vf_li.offset_to_position(d.start).unwrap_or_default(),
                                    end: vf_li.offset_to_position(d.end).unwrap_or_default(),
                                }
                            } else {
                                Range::default()
                            };
                            Some(Location {
                                uri: target_uri,
                                range,
                            })
                        })
                        .collect();
                    if !locations.is_empty() {
                        return Ok(Some(GotoDefinitionResponse::Array(locations)));
                    }
                }
            }
            return Ok(None);
        }
    }

    let verter_result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let canonical_id = uri_to_canonical_id(uri);
        let resolve_path = {
            let canonical_id = canonical_id.clone();
            let host = &server.documents.host;
            move |specifier: &str| -> Option<String> {
                host.resolve_import_via_workspace(&canonical_id, specifier)
            }
        };
        #[allow(clippy::type_complexity)]
        let resolve_fn: Option<&dyn Fn(&str) -> Option<String>> =
            Some(&resolve_path as &dyn Fn(&str) -> Option<String>);

        let encoding = server.position_encoding.read().clone();
        let host = &server.documents.host;
        let resolve_export = |target_canonical_id: &str, binding_name: &str| -> Option<Location> {
            // Follow re-exports (cycle-detected) to find the actual definition
            let (resolved_id, start, end) = host
                .get_export_span_follow_reexports(target_canonical_id, binding_name)
                .or_else(|| {
                    // Fallback to non-following version for backwards compat
                    let (s, e) = host.get_export_span(target_canonical_id, binding_name)?;
                    Some((target_canonical_id.to_string(), s, e))
                })?;
            let target_source = host.get_source(&resolved_id)?;
            let target_li = LineIndex::new(&target_source, encoding.clone());
            let start_pos = target_li.offset_to_position(start)?;
            let end_pos = target_li.offset_to_position(end)?;
            let normalized = resolved_id.replace('\\', "/");
            let uri_str = if normalized.starts_with('/') {
                format!("file://{normalized}")
            } else if normalized.chars().nth(1) == Some(':') {
                format!("file:///{normalized}")
            } else {
                return None;
            };
            let target_uri: Uri = uri_str.parse().ok()?;
            Some(Location {
                uri: target_uri,
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
            })
        };
        #[allow(clippy::type_complexity)]
        let resolve_export_fn = Some(&resolve_export as &dyn Fn(&str, &str) -> Option<Location>);

        // Unified component contract resolution runs FIRST: props, events,
        // v-model, slots. Returns early if any contract surface was hit.
        if let Some(contract_def) = server.try_component_contract_definition(uri, position) {
            return Some(contract_def);
        }

        // Barrel-file export symbol click: if the cursor is on an export
        // signature in a re-export statement, follow the chain to the terminal.
        if let Some(barrel_def) = server.try_barrel_export_definition(uri, position) {
            return Some(barrel_def);
        }

        let mut def = definition_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
            resolve_fn,
            resolve_export_fn,
        )?;

        // Fix up sentinel URIs: if the definition is in the same file, use the document URI
        if let GotoDefinitionResponse::Scalar(ref mut loc) = def {
            if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI_STR {
                loc.uri = uri.clone();
            }
        }

        Some(def)
    })();

    tracing::debug!("definition: verter found={}", verter_result.is_some());

    // If verter already resolved a cross-file definition, return it directly.
    // Querying TSGO with a synthetic TSX position often crashes it.
    if let Some(GotoDefinitionResponse::Scalar(ref loc)) = verter_result {
        if loc.uri.as_str() != uri.as_str() {
            tracing::debug!("definition: verter resolved cross-file, skipping type provider");
            return Ok(verter_result);
        }
    }

    // Component contract resolution (props, events, v-model, slots) now runs
    // BEFORE definition_at_position inside the closure above via
    // try_component_contract_definition. The old separate resolve_component_event_definition
    // and resolve_component_prop_definition calls are subsumed by it.

    // Enhance with TypeProvider for cross-file definitions.
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        if let Some(ctx) = server.type_provider_context(uri) {
            // Use validated mapping to avoid querying TSGO at synthetic TSX
            // positions (e.g., <div> → generated JSX) which can crash it.
            if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            ) {
                tracing::debug!(
                    "definition: querying type provider at tsx offset {}",
                    tsx_offset
                );
                match tp.get_definition(&ctx.tsx_path, tsx_offset).await {
                    Ok(type_defs) => {
                        tracing::debug!(
                            "definition: type provider returned {} locations",
                            type_defs.len()
                        );
                        let vue_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let barrel_resolver =
                            |path: &str, start: u32, end: u32| -> Option<Location> {
                                server.resolve_barrel_type_provider_location(path, start, end)
                            };
                        let merged = merge::merge_definitions_with_barrel_resolver(
                            verter_result,
                            type_defs,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            uri,
                            &vue_source_exists,
                            Some(&barrel_resolver),
                        );
                        // Post-process: if type provider resolved to a barrel file,
                        // follow re-exports to the terminal declaration.
                        return Ok(server.resolve_barrel_locations(merged));
                    }
                    Err(e) => {
                        tracing::warn!("definition: type provider error: {e}");
                    }
                }
            } else {
                tracing::debug!(
                    "definition: position mapping failed for {}:{}:{}",
                    uri.as_str(),
                    position.line,
                    position.character
                );
            }
        }
    }

    Ok(verter_result)
}

pub(super) async fn handle_goto_type_definition(
    server: &VerterLanguageServer,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let _hg = HandlerGuard::new("goto_type_definition");
    let uri = &params.text_document_position_params.text_document.uri;
    let _timer = server
        .statistics
        .timer("type_definition", Some(uri.as_str().to_string()));
    let position = &params.text_document_position_params.position;
    tracing::debug!(
        "type_definition: {} at {}:{}",
        uri.as_str(),
        position.line,
        position.character
    );

    server.ensure_provider_synced(uri).await;

    // Virtual file: route directly through type provider (position is already in TSX coordinates)
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_defs) = tp.get_type_definition(&tsx_path, offset).await {
                    let locations: Vec<Location> = type_defs
                        .into_iter()
                        .filter_map(|d| {
                            let vue_source_exists =
                                |p: &str| server.documents.host().get_source(p).is_some();
                            if let Some(location) = server
                                .resolve_barrel_type_provider_location(&d.path, d.start, d.end)
                            {
                                return Some(location);
                            }
                            let target_path =
                                merge::normalize_vue_path_owned(&d.path, &vue_source_exists);
                            let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                            let range = if d.path == tsx_path {
                                Range {
                                    start: vf_li.offset_to_position(d.start).unwrap_or_default(),
                                    end: vf_li.offset_to_position(d.end).unwrap_or_default(),
                                }
                            } else {
                                Range::default()
                            };
                            Some(Location {
                                uri: target_uri,
                                range,
                            })
                        })
                        .collect();
                    if !locations.is_empty() {
                        return Ok(Some(GotoDefinitionResponse::Array(locations)));
                    }
                }
            }
            return Ok(None);
        }
    }

    // Type definition is purely a type provider operation — no verter analysis phase.
    if let Some(tp) = &server.type_provider {
        if let Some(ctx) = server.type_provider_context(uri) {
            if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            ) {
                tracing::debug!(
                    "type_definition: querying type provider at tsx offset {}",
                    tsx_offset
                );
                match tp.get_type_definition(&ctx.tsx_path, tsx_offset).await {
                    Ok(type_defs) => {
                        tracing::debug!(
                            "type_definition: type provider returned {} locations",
                            type_defs.len()
                        );
                        let vue_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let barrel_resolver =
                            |path: &str, start: u32, end: u32| -> Option<Location> {
                                server.resolve_barrel_type_provider_location(path, start, end)
                            };
                        return Ok(merge::merge_definitions_with_barrel_resolver(
                            None,
                            type_defs,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            uri,
                            &vue_source_exists,
                            Some(&barrel_resolver),
                        ));
                    }
                    Err(e) => {
                        tracing::warn!("type_definition: type provider error: {e}");
                    }
                }
            } else {
                tracing::debug!(
                    "type_definition: position mapping failed for {}:{}:{}",
                    uri.as_str(),
                    position.line,
                    position.character
                );
            }
        }
    }

    Ok(None)
}

pub(super) async fn handle_references(
    server: &VerterLanguageServer,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let _hg = HandlerGuard::new("references");
    let uri = &params.text_document_position.text_document.uri;
    let _timer = server
        .statistics
        .timer("references", Some(uri.as_str().to_string()));
    let position = &params.text_document_position.position;
    let include_declaration = params.context.include_declaration;
    tracing::debug!(
        "references: {} at {}:{} (include_decl={})",
        uri.as_str(),
        position.line,
        position.character,
        include_declaration
    );

    // Virtual file: route directly through TSGO
    if let Some(tp) = &server.type_provider {
        if let Some((tsx_path, vf_li)) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_refs) = tp.get_references(&tsx_path, offset).await {
                    let locations: Vec<Location> = type_refs
                        .into_iter()
                        .filter_map(|r| {
                            let vue_source_exists =
                                |p: &str| server.documents.host().get_source(p).is_some();
                            let target_path =
                                merge::normalize_vue_path_owned(&r.path, &vue_source_exists);
                            let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                            let range = if r.path == tsx_path {
                                Range {
                                    start: vf_li.offset_to_position(r.start).unwrap_or_default(),
                                    end: vf_li.offset_to_position(r.end).unwrap_or_default(),
                                }
                            } else {
                                Range::default()
                            };
                            Some(Location {
                                uri: target_uri,
                                range,
                            })
                        })
                        .collect();
                    return Ok(if locations.is_empty() {
                        None
                    } else {
                        Some(locations)
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
        let mut locations = references_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
            include_declaration,
        )?;

        // Fix up sentinel URIs
        for loc in &mut locations {
            if loc.uri.as_str() == crate::features::references::SAME_FILE_URI_STR {
                loc.uri = uri.clone();
            }
        }

        Some(locations)
    })();

    tracing::debug!(
        "references: verter found {}",
        verter_result.as_ref().map_or(0, |v| v.len())
    );

    // Enhance with TypeProvider if available.
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        if let Some(ctx) = server.type_provider_context(uri) {
            if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            ) {
                tracing::debug!(
                    "references: querying type provider at tsx offset {}",
                    tsx_offset
                );
                match tp.get_references(&ctx.tsx_path, tsx_offset).await {
                    Ok(type_refs) => {
                        tracing::debug!(
                            "references: type provider returned {} locations",
                            type_refs.len()
                        );
                        let vue_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        return Ok(merge::merge_references(
                            verter_result,
                            type_refs,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            &vue_source_exists,
                        ));
                    }
                    Err(e) => {
                        tracing::warn!("references: type provider error: {e}");
                    }
                }
            } else {
                tracing::debug!(
                    "references: position mapping failed for {}:{}:{}",
                    uri.as_str(),
                    position.line,
                    position.character
                );
            }
        }
    }

    Ok(verter_result)
}

pub(super) async fn handle_prepare_rename(
    server: &VerterLanguageServer,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let _hg = HandlerGuard::new("prepare_rename");
    let uri = &params.text_document.uri;
    let position = &params.position;

    // Virtual file: not supported (no Verter rename context for generated code)
    if server.documents.get_virtual_source_uri(uri).is_some() {
        return Ok(None);
    }

    let result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let range = prepare_rename(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
        )?;
        Some(PrepareRenameResponse::Range(range))
    })();

    Ok(result)
}

pub(super) async fn handle_rename(
    server: &VerterLanguageServer,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let _hg = HandlerGuard::new("rename");
    let uri = &params.text_document_position.text_document.uri;
    let position = &params.text_document_position.position;
    let new_name = &params.new_name;

    // Virtual file: not supported (renaming in generated code isn't meaningful)
    if server.documents.get_virtual_source_uri(uri).is_some() {
        return Ok(None);
    }

    let verter_result = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let mut edit = rename_at_position(
            position,
            new_name,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
        )?;

        // Fix up sentinel URIs in workspace edit
        if let Some(ref mut changes) = edit.changes {
            let sentinel = crate::features::rename::SAME_FILE_URI.clone();
            if let Some(edits) = changes.remove(&sentinel) {
                changes.insert(uri.clone(), edits);
            }
        }

        Some(edit)
    })();

    // Enhance with TypeProvider for cross-file renames.
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        if let Some(ctx) = server.type_provider_context(uri) {
            if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            ) {
                if let Ok(type_locs) = tp.get_rename_locations(&ctx.tsx_path, tsx_offset).await {
                    let vue_source_exists =
                        |p: &str| server.documents.host().get_source(p).is_some();
                    return Ok(merge::merge_rename_locations(
                        verter_result,
                        type_locs,
                        new_name,
                        &ctx.tsx_line_index,
                        &ctx.mapper,
                        &ctx.vue_line_index,
                        Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                        &vue_source_exists,
                    ));
                }
            }
        }
    }

    Ok(verter_result)
}

/// Post-process a hover response with provenance enrichment
/// (plan §3 Commit 9):
/// - If `hover.provenance` is disabled → return hover unchanged.
/// - If the enrichment cache has a payload for `(canonical_id,
///   position)` → append it to the hover body.
/// - On cache miss → spawn a background blocking task to compute
///   the payload via `VerterHost::get_component_meta_with_resolution`
///   and insert it into the cache. The current hover call returns
///   the legacy payload immediately.
///
/// The host must have `audit_enabled + footprint_capture` set for
/// the background task to produce a useful payload; otherwise the
/// task returns without populating the cache (graceful degradation).
pub(super) fn enrich_hover_with_provenance(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
    hover: Option<Hover>,
) -> Option<Hover> {
    use std::sync::atomic::Ordering;
    if !server.hover_provenance_enabled.load(Ordering::Relaxed) {
        return hover;
    }
    let Some(canonical_id) = server.documents.get_canonical_id(uri) else {
        return hover;
    };
    let key =
        crate::features::hover_provenance::HoverProvenanceKey::new(canonical_id.clone(), *position);

    if let Some(payload) = server.hover_provenance_cache.get(&key) {
        return Some(append_markdown(hover, &payload.markdown));
    }

    // Cache miss — check whether the host can actually produce a
    // useful payload BEFORE spawning. If `audit_enabled` or
    // `footprint_capture` are off, `get_component_meta_with_resolution`
    // would run to completion but `take_audit_record` would
    // return None and the cache would stay empty. That means
    // every subsequent hover would spawn another futile task.
    // Short-circuit here so the user sees the legacy hover and
    // no blocking-pool slots are burned on a capture-disabled
    // host. Plan §3 Commit 9.
    let host = server.documents.host_arc();
    if !host.config().audit_enabled || !host.config().footprint_capture {
        return hover;
    }

    // Cache miss + capture enabled — spawn a background task.
    // Plan §3 Commit 9 "legacy payload immediately; background
    // AuditedRequest".
    let cache = Arc::clone(&server.hover_provenance_cache);
    let canonical_for_task = canonical_id;
    tokio::task::spawn_blocking(move || {
        let Some((_analysis, resolution)) =
            host.get_component_meta_with_resolution(&canonical_for_task)
        else {
            return;
        };
        let Some(record) = host.take_audit_record(resolution.request_id) else {
            return;
        };
        let markdown = crate::features::hover_provenance::render_provenance_markdown(&record);
        cache.insert(
            key,
            crate::features::hover_provenance::HoverProvenancePayload { markdown },
        );
    });

    hover
}

/// Append a markdown suffix to a hover body. Plan §3 Commit 9 —
/// used by the hover provenance enrichment to tack on the
/// "Provenance" section below the legacy hover content. Returns a
/// constructed hover even if the input was `None` (the enrichment
/// alone counts as useful output).
fn append_markdown(hover: Option<Hover>, suffix: &str) -> Hover {
    match hover {
        Some(mut h) => {
            let combined = match h.contents {
                HoverContents::Markup(existing) => {
                    let mut value = existing.value;
                    value.push_str(suffix);
                    HoverContents::Markup(MarkupContent {
                        kind: existing.kind,
                        value,
                    })
                }
                HoverContents::Scalar(marked) => {
                    let value = match marked {
                        MarkedString::String(s) => s,
                        MarkedString::LanguageString(ls) => ls.value,
                    };
                    HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("{value}{suffix}"),
                    })
                }
                HoverContents::Array(items) => {
                    let mut combined = String::new();
                    for item in items {
                        let part = match item {
                            MarkedString::String(s) => s,
                            MarkedString::LanguageString(ls) => ls.value,
                        };
                        if !combined.is_empty() {
                            combined.push_str("\n\n");
                        }
                        combined.push_str(&part);
                    }
                    combined.push_str(suffix);
                    HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: combined,
                    })
                }
            };
            h.contents = combined;
            h
        }
        None => Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: suffix.to_string(),
            }),
            range: None,
        },
    }
}
