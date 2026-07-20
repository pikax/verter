//! LSP navigation feature method bodies — hover and completion.
//!
//! Free functions hosting the bodies of `impl LanguageServer for
//! VerterLanguageServer` hover/completion methods (hover, completion,
//! completion_resolve). The definition/type-definition/references/rename
//! bodies live in the `nav_features_navigation` sibling module; the
//! hover-provenance enrichment helpers live in the
//! `nav_features_hover_provenance` sibling module.
//!
//! The trait impl block stays in `mod.rs`; each trait method is a
//! 1-line stub that delegates to the corresponding `handle_<method>`
//! free function here.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::features::completion::completions_at_position;
use crate::features::cursor_context::{
    classify_cursor_context_for_language, classify_expression_context_with_trigger,
    CarrierTemplateLanguage, CursorContext, ExpressionContext, TemplateCursorContext,
};
use crate::features::hover;
use crate::features::hover::hover_at_position;
use crate::type_provider::auto_import::ProviderImportEdit;
use crate::type_provider::merge;
use crate::type_provider::protocol::CompletionResolveData;

use super::handler_guard::HandlerGuard;
use super::nav_features_completion_resolve::{
    completion_resolve_error, merge_resolved_label_details, resolve_provider_auto_import_edits,
};
use super::nav_features_hover_provenance::enrich_hover_with_provenance;
use super::server_utils::*;
use super::VerterLanguageServer;

/// Whether the completion position sits inside a style `v-bind(|)` context.
fn is_style_v_bind_context(server: &VerterLanguageServer, uri: &Uri, position: &Position) -> bool {
    (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri);
        let blocks = scan_sfc_blocks(&doc.source);
        let offset = doc.line_index.position_to_offset(position)?;
        Some(matches!(
            classify_cursor_context_for_language(
                offset,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                CarrierTemplateLanguage::from_uri(uri.as_str()),
            ),
            CursorContext::Style(crate::features::cursor_context::StyleCursorContext::VBind)
        ))
    })()
    .unwrap_or(false)
}

/// Attach provider-typed `detail` to `v-bind(|)` completion items: for each
/// offered binding (bounded), a quickinfo at its DECLARATION position supplies
/// the type line. Items whose declaration cannot be mapped keep their native
/// kind detail (fail closed — never fabricated).
async fn enrich_v_bind_completion_details(
    server: &VerterLanguageServer,
    uri: &Uri,
    mut items: Vec<CompletionItem>,
) -> Vec<CompletionItem> {
    const MAX_TYPED_ITEMS: usize = 12;
    let Some(tp) = &server.type_provider else {
        return items;
    };
    let Some(ctx) = server.type_provider_context(uri) else {
        return items;
    };
    // Declaration position per offered binding name (sync snapshot reads).
    let decl_positions: Vec<(usize, Position)> = {
        let Some(doc) = server.documents.get(uri) else {
            return items;
        };
        let Some(analysis) = server.documents.get_analysis(uri) else {
            return items;
        };
        items
            .iter()
            .enumerate()
            .take(MAX_TYPED_ITEMS)
            .filter_map(|(idx, item)| {
                let binding = analysis.bindings.iter().find(|b| b.name == item.label)?;
                if binding.span.start == 0 && binding.span.end == 0 {
                    return None;
                }
                Some((idx, doc.line_index.offset_to_position(binding.span.start)?))
            })
            .collect()
    };
    for (idx, decl_pos) in decl_positions {
        let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
            &decl_pos,
            &ctx.carrier_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        ) else {
            continue;
        };
        if let Ok(Some(info)) = tp.get_hover(&ctx.tsx_path, tsx_offset).await {
            // Post-await validation (fail closed): stop enriching against a
            // superseded surface; already-set details came from a live one.
            if !server.provider_context_still_valid(uri, &ctx) {
                break;
            }
            // First informative line of the quickinfo (skip code fences).
            if let Some(line) = info
                .contents
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("```"))
            {
                if let Some(item) = items.get_mut(idx) {
                    item.detail = Some(line.to_string());
                }
            }
        }
    }
    items
}

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

    if server.editor_owns_carrier_source_features() {
        // CSS-native results have no TS correlate — the editor's TS plugin can
        // never own them, so the server still serves EXACTLY the css leg.
        let css_hover = (|| {
            let doc = server.documents.get(uri)?;
            let analysis = server.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            hover::css_only_hover_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )
        })();
        return Ok(css_hover);
    }

    // Virtual file: route directly through TSGO (position is already in TSX coordinates)
    if let Some(tp) = &server.type_provider {
        if let Some(vf_ctx) = server.virtual_file_context(uri) {
            if let Some(offset) = vf_ctx.line_index.position_to_offset(position) {
                if let Ok(Some(info)) = tp.get_hover(&vf_ctx.tsx_path, offset).await {
                    // Post-await validation (fail closed): a hover produced
                    // against a superseded surface must be dropped.
                    if !server.virtual_request_surface_still_valid(uri, &vf_ctx) {
                        return Ok(None);
                    }
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
        let native = hover_at_position(
            position,
            &doc.source,
            &blocks,
            analysis.as_ref(),
            &doc.line_index,
            ssr_context,
        );
        // D6 Svelte: directive-KEYWORD doc hovers are verter-owned — the
        // provider can never describe the `use:`/`transition:` keyword through
        // the mapped projection (its local name stays provider-answered).
        native.or_else(|| {
            let canonical_id = server.documents.get_canonical_id(uri)?;
            if !crate::server::carrier_language_for(&canonical_id)
                .is_some_and(|language| language.is_svelte())
            {
                return None;
            }
            let offset = doc.line_index.position_to_offset(position)?;
            crate::features::hover_directive_names::svelte_directive_keyword_hover(
                offset,
                analysis.as_ref()?,
            )
        })
    })();
    let vue_kind_label = verter_full.as_ref().and_then(|r| r.vue_kind_label.clone());
    let source_token = verter_full.as_ref().and_then(|r| r.source_token.clone());
    let verter_result = verter_full.map(|r| r.hover);

    let child_hover_target = (|| {
        let analysis = server.documents.get_analysis(uri)?;
        let doc = server.documents.get(uri)?;
        let carrier_offset = doc.line_index.position_to_offset(position)?;
        hover::child_hover_target_at_offset(carrier_offset, &doc.source, &analysis)
    })();
    if let Some(target) = child_hover_target.as_ref() {
        if let Some(child_hover) = server.child_hover_for_target(uri, target) {
            return Ok(Some(child_hover));
        }
        // D3 fail-closed: an identified slot-name token on a resolved child
        // whose slots surface declares no such slot is SILENT — never the
        // untyped static fallback, never a fabricated signature.
        if matches!(target, hover::ChildHoverTarget::SlotAttribute(_)) {
            return Ok(None);
        }
    }

    // B4: typed `v-bind()` hover — the style token has no TSX projection
    // (style blocks are removed from the generated surface), so the provider
    // is queried at the root binding's DECLARATION position and the result is
    // presented on the v-bind token. Fail-closed to the native v-bind hover.
    let vbind_target = (|| {
        let doc = server.documents.get(uri)?;
        let analysis = server.documents.get_analysis(uri)?;
        let offset = doc.line_index.position_to_offset(position)?;
        let (expr, decl_span) = crate::css::v_bind_decl_target_at(offset, &analysis)?;
        let decl_pos = doc.line_index.offset_to_position(decl_span.start)?;
        Some((expr, decl_pos))
    })();
    if let Some((expr, decl_pos)) = vbind_target {
        if let Some(tp) = &server.type_provider {
            if let Some(ctx) = server.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                    &decl_pos,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    if let Ok(Some(info)) = tp.get_hover(&ctx.tsx_path, tsx_offset).await {
                        // Post-await validation (fail closed): drop a provider
                        // result produced against a superseded surface.
                        if server.provider_context_still_valid(uri, &ctx) {
                            return Ok(Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: format!("**v-bind({expr})**\n\n{}", info.contents),
                                }),
                                range: None,
                            }));
                        }
                    }
                }
            }
        }
        // No provider / unmappable declaration — native v-bind hover only.
        return Ok(verter_result);
    }

    // Slot syntax: verter provides rich hover; type provider returns unhelpful
    // generic types (`() any`, `string`). Skip type provider merge entirely.
    if verter_result.is_some() {
        if let Some(analysis) = server.documents.get_analysis(uri) {
            if let Some(doc) = server.documents.get(uri) {
                if let Some(carrier_offset) = doc.line_index.position_to_offset(position) {
                    if hover::is_on_slot_syntax(carrier_offset, &analysis) {
                        return Ok(verter_result);
                    }
                }
            }
        }
    }

    // Enhance with TypeProvider if available.
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = &server.type_provider {
        let repaired_current_file = server.current_file_needs_inline_type_provider_sync(uri);
        if repaired_current_file {
            tracing::debug!(
                "hover: repairing current-file provider sync for {}",
                uri.as_str()
            );
            server.ensure_current_file_synced(uri).await;
        }
        if let Some(captured_ctx) = server.type_provider_context(uri) {
            // Use validated mapping to avoid querying TSGO at synthetic TSX
            // positions (e.g., <div> → generated JSX) which can crash it.
            let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
                position,
                &captured_ctx.carrier_line_index,
                &captured_ctx.mapper,
                &captured_ctx.tsx_line_index,
            );

            // `(provider hover, surface that produced it)`. The surface is
            // carried alongside so a post-error resync+retry validates and
            // merges against the RETRY surface, never the superseded one.
            let (type_hover, ctx) = if let Some(tsx_offset) = tsx_offset {
                // Log TSX context snippet around the hover offset for debugging
                if let Some((before, after)) =
                    debug_snippet(&captured_ctx.tsx_content, tsx_offset as usize)
                {
                    tracing::info!(
                        "hover TSX context at offset {}: «{}⸽{}»",
                        tsx_offset,
                        before.replace('\n', "↵"),
                        after.replace('\n', "↵"),
                    );
                }
                // The store-backed tsserver plugin applies a publication-token
                // refresh on the next Node event-loop turn to avoid re-entrant
                // configured-project mutation inside `configurePlugin`. When this
                // request just repaired a stale current-file surface, the first
                // ordered quickinfo response is the synchronization probe that lets
                // that turn run; discard it and issue the user-visible query against
                // the refreshed ScriptInfo. Warm hovers and tsgo pay no duplicate.
                if repaired_current_file
                    && matches!(server.type_provider_kind, crate::TypeProviderKind::Tsserver)
                {
                    let _ = tp.get_hover(&captured_ctx.tsx_path, tsx_offset).await;
                }
                match tp.get_hover(&captured_ctx.tsx_path, tsx_offset).await {
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
                        (hover, captured_ctx)
                    }
                    Err(e) => {
                        // No-silent-empty (D7): a FAILED provider hover must
                        // never surface as a vanishing tooltip. Resync the
                        // current file and retry exactly once against the
                        // freshly captured surface; a second failure fails
                        // closed. Provider-neutral — this sits above the
                        // per-route provider trait.
                        //
                        // Fail-closed-on-persistent is the INTENDED semantics:
                        // after the bounded retry the handler returns `None`
                        // (no tooltip), never a fabrication and never a spin.
                        // A persistently failing provider is a sync/health
                        // concern (the B10/B12 family), not something hover
                        // may paper over with invented content.
                        tracing::warn!(
                            "hover type provider error: {} — resyncing and retrying once",
                            e
                        );
                        server.ensure_current_file_synced(uri).await;
                        match server.type_provider_context(uri) {
                            Some(retry_ctx) => {
                                let retry_offset = merge::carrier_position_to_tsx_offset_validated(
                                    position,
                                    &retry_ctx.carrier_line_index,
                                    &retry_ctx.mapper,
                                    &retry_ctx.tsx_line_index,
                                );
                                match retry_offset {
                                    Some(retry_offset) => {
                                        match tp.get_hover(&retry_ctx.tsx_path, retry_offset).await
                                        {
                                            Ok(hover) => (hover, retry_ctx),
                                            Err(e2) => {
                                                tracing::warn!(
                                                    "hover type provider retry failed: {}",
                                                    e2
                                                );
                                                (None, retry_ctx)
                                            }
                                        }
                                    }
                                    None => (None, retry_ctx),
                                }
                            }
                            None => (None, captured_ctx),
                        }
                    }
                }
            } else {
                tracing::info!(
                    "hover: carrier_to_tsx validation failed for {}:{} — position is in synthetic TSX region",
                    position.line,
                    position.character
                );
                (None, captured_ctx)
            };

            // Post-await validation: a hover produced against a surface that no
            // longer matches must be DROPPED (fail closed), never mapped through
            // a superseded context.
            let type_hover = type_hover.filter(|_| server.provider_context_still_valid(uri, &ctx));

            // If TSGO returned a result, merge and return.
            if type_hover.is_some() {
                return Ok(merge::merge_hover(
                    verter_result,
                    type_hover,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                    &ctx.carrier_line_index,
                    vue_kind_label.as_deref(),
                    source_token.as_ref(),
                ));
            }

            // Redirect: when TSGO returned nothing and the cursor is on a static
            // `class`/`style` attribute that was merged with a dynamic binding,
            // the static attribute's source position maps to removed TSX content.
            // Retry at the dynamic directive's position instead.
            if let Some(analysis) = server.documents.get_analysis(uri) {
                let carrier_offset = ctx.carrier_line_index.position_to_offset(position);
                if let Some(carrier_offset) = carrier_offset {
                    if let Some(redirect_offset) =
                        hover::merged_attribute_redirect_offset(carrier_offset, &analysis)
                    {
                        // Convert the redirect SFC offset to a Vue line:col position
                        if let Some(redirect_pos) =
                            ctx.carrier_line_index.offset_to_position(redirect_offset)
                        {
                            if let Some(redirect_tsx) =
                                merge::carrier_position_to_tsx_offset_validated(
                                    &redirect_pos,
                                    &ctx.carrier_line_index,
                                    &ctx.mapper,
                                    &ctx.tsx_line_index,
                                )
                            {
                                tracing::info!(
                                    "hover: redirecting merged class/style from vue offset {} to {} (tsx offset {})",
                                    carrier_offset, redirect_offset, redirect_tsx
                                );
                                if let Ok(redirect_hover) =
                                    tp.get_hover(&ctx.tsx_path, redirect_tsx).await
                                {
                                    // Post-await validation (fail closed): drop the
                                    // provider hover on a superseded surface.
                                    let redirect_hover = redirect_hover
                                        .filter(|_| server.provider_context_still_valid(uri, &ctx));
                                    return Ok(merge::merge_hover(
                                        verter_result,
                                        redirect_hover,
                                        &ctx.mapper,
                                        &ctx.tsx_line_index,
                                        &ctx.carrier_line_index,
                                        vue_kind_label.as_deref(),
                                        source_token.as_ref(),
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
                &ctx.carrier_line_index,
                vue_kind_label.as_deref(),
                source_token.as_ref(),
            ));
        } else {
            tracing::info!("hover: no ide_context for {}", uri.as_str());
        }
    } else {
        tracing::info!("hover: no type_provider");
    }

    // Provenance enrichment on the primary verter-only return path.
    // Early returns (virtual file, child-hover, type-provider merge)
    // intentionally skip enrichment for now; the opt-in feature
    // targets the common "verter-only hover on a Vue binding" case.
    Ok(enrich_hover_with_provenance(
        server,
        uri,
        position,
        verter_result,
    ))
}

/// Where the completion cursor sits in the SFC, as a first-class context.
///
/// Replaces the historical `(is_template_attr_context, in_expression_context)`
/// boolean pair so that `<script setup>` positions are classified explicitly.
/// Both `TemplateExpression` and `Script` compute an [`ExpressionContext`] (so
/// member-access completion works in scripts), but the template-only
/// `IdentifierExpected` TypeProvider suppression stays fenced to
/// `TemplateExpression` — ordinary script identifier completions (TS globals,
/// imports) must NOT be suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionSourceContext {
    /// Template attribute-name position (`<div cla|`).
    TemplateAttr,
    /// Template expression / interpolation (`:prop="x|"`, `{{ x| }}`).
    TemplateExpression,
    /// Inside `<script>` / `<script setup>`.
    Script,
    /// Anywhere else (tag name, text, style, root level, …).
    Other,
}

impl CompletionSourceContext {
    /// Whether this context should compute an [`ExpressionContext`] from the
    /// mapped TSX (member access, literal, type position, …).
    fn computes_expression_context(self) -> bool {
        matches!(self, Self::TemplateExpression | Self::Script)
    }
}

#[derive(Clone)]
struct CompletionDocumentIdentity {
    version: i32,
    source: std::sync::Arc<str>,
}

fn completion_document_identity(
    server: &VerterLanguageServer,
    uri: &Uri,
) -> Option<CompletionDocumentIdentity> {
    server
        .documents
        .get(uri)
        .map(|document| CompletionDocumentIdentity {
            version: document.version,
            source: std::sync::Arc::clone(&document.source),
        })
}

fn completion_document_identity_matches(
    before: Option<&CompletionDocumentIdentity>,
    after: Option<&CompletionDocumentIdentity>,
) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            before.version == after.version && std::sync::Arc::ptr_eq(&before.source, &after.source)
        }
        (None, None) => true,
        _ => false,
    }
}

pub(super) async fn handle_completion(
    server: &VerterLanguageServer,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = params.text_document_position.text_document.uri.clone();
    if uri.as_str().starts_with("verter-virtual://") {
        return handle_completion_attempt(server, &params, false).await;
    }

    // Provider work can suspend while a newer document instance or edit commits.
    // Match both version and immutable source identity: a close/reopen may reuse
    // the same LSP version and must still invalidate the suspended response. The
    // final native-only attempt keeps the commit fence through native calculation,
    // so it returns the coherent post-fence snapshot even if the pre-wait identity
    // sampled here was older.
    for _attempt in 0..2 {
        let identity_before = completion_document_identity(server, &uri);
        let response = handle_completion_attempt(server, &params, false).await?;
        let identity_after = completion_document_identity(server, &uri);
        if completion_document_identity_matches(identity_before.as_ref(), identity_after.as_ref()) {
            return Ok(response);
        }
        tracing::debug!(
            "completion: retrying {} after document identity advanced {:?} -> {:?}",
            uri.as_str(),
            identity_before.as_ref().map(|identity| identity.version),
            identity_after.as_ref().map(|identity| identity.version)
        );
    }
    #[cfg(test)]
    server.maybe_pause_completion_before_final_native().await;
    handle_completion_attempt(server, &params, true).await
}

async fn handle_completion_attempt(
    server: &VerterLanguageServer,
    params: &CompletionParams,
    native_only: bool,
) -> Result<Option<CompletionResponse>> {
    let _hg = HandlerGuard::new("completion");
    let uri = &params.text_document_position.text_document.uri;
    let _timer = server
        .statistics
        .timer("completion", Some(uri.as_str().to_string()));
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
    let provider_only = server.provider_only_completions();

    // Capture completion source only after an already-running `did_change` has
    // committed its registry and host updates. Keep this document-version
    // fence through every synchronous native snapshot read below; otherwise a
    // new edit can land between releasing the mutex and reading source/analysis.
    // The fence is released before any provider await.
    let mut edit_fence = server.did_change_mutex.lock().await;
    // NOTE: We do NOT call ensure_provider_synced here.  The debounced sync in
    // did_change sends the update to TSGO within 50ms of the last keystroke.
    // Flushing inline would serialize: sync → TSGO re-analysis → get_completions,
    // which takes 2-3s on large files and blocks the entire completion pipeline.
    // Instead we let TSGO answer with whatever version it has; if it's stale the
    // response arrives fast and VS Code re-requests after the debounce fires.

    // Virtual file: route directly through TSGO
    if let Some(tp) = &server.type_provider {
        if let Some(vf_ctx) = server.virtual_file_context(uri) {
            drop(edit_fence);
            let tsx_path = vf_ctx.tsx_path.clone();
            if let Some(offset) = vf_ctx.line_index.position_to_offset(position) {
                if let Ok(result) = tp
                    .get_completions(&tsx_path, offset, trigger_character)
                    .await
                {
                    // Post-await validation (fail closed): completions produced
                    // against a superseded surface must be dropped.
                    if !server.virtual_request_surface_still_valid(uri, &vf_ctx) {
                        return Ok(None);
                    }
                    // Route through the SAME provider→LSP envelope mapper as the
                    // normal completion path so a provider auto-import returned on
                    // the virtual-file path preserves its actionable
                    // `verter_resolve` handle and can resolve into an import edit.
                    // (Previously this branch stripped `Completion.data`, so the
                    // likely real `.vue` completion path could never auto-import —
                    // review finding F1.) Virtual-file positions are already in the
                    // generated-TSX coordinates the editor shows, so no carrier
                    // text-edit re-anchor applies (`text_edit = None`); the resolve
                    // re-issues against this same `tsx_path`.
                    let provider_id = tp.provider_id();
                    let items: Vec<CompletionItem> = result
                        .items
                        .into_iter()
                        .filter(|c| {
                            !c.label.starts_with("___VERTER___") && !c.label.starts_with("$V_")
                        })
                        .map(|c| {
                            let label = c.label.clone();
                            merge::provider_completion_to_lsp_item(
                                c,
                                label,
                                None,
                                provider_id,
                                Some(&tsx_path),
                            )
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

    // D1 (open+edit+completion race): tower-lsp runs the did_open notification and
    // a completion request concurrently, so a completion can arrive BEFORE the
    // document is registered in `documents`. Returning `Ok(None)` here is what the
    // editor renders as a document-text (word) fallback — the exact D1 defect.
    // Hold briefly for the in-flight open to land (bounded; a real editor only
    // sends completion for a document it is opening) instead of falling open into
    // silence. Once the document registers, the normal path answers typed.
    if server.documents.get(uri).is_none() {
        drop(edit_fence);
        // ~300ms total. Each request remains independent: unrelated concurrent
        // completions cannot cancel a valid request into an empty response.
        for wait_ms in [20u64, 20, 40, 60, 80, 80] {
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            if server.documents.get(uri).is_some() {
                break;
            }
        }
        edit_fence = server.did_change_mutex.lock().await;
    }

    let completion_ssr_context = {
        let canonical_id = server.documents.get_canonical_id(uri);
        canonical_id
            .as_deref()
            .map(|cid| server.is_ssr_context(cid))
            .unwrap_or(false)
    };

    struct NativeCompletionSnapshot {
        source: std::sync::Arc<str>,
        line_index: crate::documents::line_index::LineIndex,
        analysis: Option<verter_session::FileAnalysisSnapshot>,
        blocks: Vec<crate::documents::sfc_scanner::SfcBlock>,
        canonical_id: String,
    }
    let native_snapshot = (|| {
        let doc = server.documents.get(uri)?;
        Some(NativeCompletionSnapshot {
            source: doc.source.clone(),
            line_index: doc.line_index.clone(),
            analysis: server.documents.get_analysis(uri),
            blocks: scan_sfc_blocks(&doc.source),
            canonical_id: crate::documents::uri_to_canonical_id(uri),
        })
    })();
    // Normal attempts release the typing fence before cold child/meta work and
    // validate identity after provider awaits. The bounded final native-only
    // attempt deliberately retains the fence through the synchronous native
    // calculation: it has no provider await and therefore returns one coherent,
    // current snapshot instead of panicking or failing open under sustained churn.
    let native_edit_fence = if native_only {
        Some(edit_fence)
    } else {
        drop(edit_fence);
        #[cfg(test)]
        server.maybe_pause_completion_after_snapshot().await;
        None
    };
    #[cfg(test)]
    if native_only {
        server.maybe_pause_final_completion_after_snapshot().await;
    }

    let verter_result = native_snapshot.as_ref().and_then(|native| {
        let canonical_id = &native.canonical_id;
        let resolve_component = |import_source: &str,
                                 component_name: Option<&str>|
         -> Option<verter_session::FileAnalysisSnapshot> {
            let get_component_analysis =
                |resolved: &str| -> Option<verter_session::FileAnalysisSnapshot> {
                    if server.documents.host().get_analysis(resolved).is_none() {
                        server.documents.host().ensure_loaded(resolved);
                    }
                    let mut analysis = server.documents.host().get_analysis(resolved)?;
                    if carrier_language_for(resolved).is_some_and(|language| language.is_svelte())
                        && analysis
                            .template
                            .as_deref()
                            .is_none_or(|template| template.prop_definitions.is_empty())
                    {
                        // The cache-owned component-meta entry point is the
                        // cold-building semantic authority for public Svelte
                        // keys. It preserves aliases, string keys, rest-covered
                        // members, named interfaces, and whole-object `$props()`
                        // declarations that local bindings cannot.
                        let host = server.documents.host();
                        let mut semantic_props = host
                            .get_component_meta_with_resolution(resolved)
                            .map(|(component_meta, _resolution)| {
                                component_meta
                                    .props
                                    .into_iter()
                                    .map(|prop| {
                                        let is_boolean =
                                            prop.raw_type.as_deref() == Some("boolean");
                                        verter_semantic::analysis::AnalyzedPropDefinition {
                                            name: prop.name,
                                            type_annotation: prop.raw_type,
                                            has_default: prop.has_default,
                                            is_required: prop.required,
                                            is_boolean,
                                            used_in_template: false,
                                            used_in_script: false,
                                            span: verter_span::Span::new(0, 0),
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        // Svelte's component-meta request currently cold-builds
                        // the cache entry while its `analysis.props` lane can be
                        // empty. The cache-owned public contract is the structured
                        // Svelte projection of that same entry; consult it only
                        // after the cold-building request, never by parsing source.
                        if semantic_props.is_empty() {
                            semantic_props = host
                                .get_public_api_projection(resolved)
                                .and_then(|projection| projection.contract)
                                .map(|contract| {
                                    contract
                                        .props
                                        .into_iter()
                                        .map(|prop| {
                                            let is_boolean =
                                                prop.type_annotation.as_deref() == Some("boolean");
                                            verter_semantic::analysis::AnalyzedPropDefinition {
                                                name: prop.name,
                                                type_annotation: prop.type_annotation,
                                                has_default: prop.has_default,
                                                is_required: !prop.optional,
                                                is_boolean,
                                                used_in_template: false,
                                                used_in_script: false,
                                                span: verter_span::Span::new(0, 0),
                                            }
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                        }
                        if !semantic_props.is_empty() {
                            let mut template =
                                analysis.template.as_deref().cloned().unwrap_or_default();
                            template.prop_definitions = semantic_props;
                            analysis.template = Some(std::sync::Arc::new(template));
                        }
                    }
                    Some(analysis)
                };
            let try_follow_reexport = |resolved: &str,
                                       comp_name: Option<&str>|
             -> Option<verter_session::FileAnalysisSnapshot> {
                if crate::server::is_default_export_component_carrier(resolved) {
                    // A direct `.vue` import resolves the component file itself. Ensure
                    // it is loaded/compiled so its prop/event/slot analysis is
                    // available — the cold-open race where the child is not yet in the
                    // host cache would otherwise leave component-prop completions
                    // empty (D1: the editor then falls back to word suggestions).
                    return get_component_analysis(resolved);
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
                        if crate::server::is_default_export_component_carrier(&terminal_id) {
                            // Ensure the terminal .vue file is compiled
                            return get_component_analysis(&terminal_id);
                        }
                    }
                }
                get_component_analysis(resolved)
            };

            // Try 1: Use resolve_import_specifier (handles relative, alias, index files)
            if let Some(resolved) = server.resolve_import_specifier(canonical_id, import_source) {
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
                server.resolve_import_specifier(canonical_id, import_source)
            {
                if let Some(a) = try_follow_reexport(&resolved_path, component_name) {
                    return Some(a);
                }
            }

            // Try 4: Direct lookup (bare specifiers, already-resolved)
            try_follow_reexport(import_source, component_name)
        };
        // Build workspace component list for auto-import
        let ws_components = build_workspace_components(&server.documents.host, canonical_id);
        completions_at_position(
            position,
            &native.source,
            &native.blocks,
            native.analysis.as_ref(),
            &native.line_index,
            Some(&resolve_component),
            if ws_components.is_empty() {
                None
            } else {
                Some(&ws_components)
            },
            Some(uri.as_str()),
            completion_ssr_context,
        )
    });

    // Provider-attribution E2E still computes Verter's template-visible NAME set,
    // but only as a subtractive scope boundary. No Verter completion item is ever
    // emitted in this mode: every surviving item, kind, detail, and resolve handle
    // is owned by the selected TypeScript engine. Keeping the visibility boundary
    // prevents carrier implementation scope (DOM globals and generated helpers)
    // from masquerading as expressions users can actually name in a Vue template.
    let provider_only_template_scope = provider_only.then(|| {
        let mut scope = verter_result
            .as_ref()
            .map(|result| {
                result
                    .items
                    .iter()
                    .map(|item| item.label.clone())
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        if let Some(native) = native_snapshot.as_ref() {
            if let Some(analysis) = native.analysis.as_ref() {
                if let (Some(cursor_offset), Some(template)) = (
                    native.line_index.position_to_offset(position),
                    analysis.template.as_deref(),
                ) {
                    scope.extend(template_lexical_scope_names(template, cursor_offset));
                }
            }
        }
        scope
    });
    let (verter_is_incomplete, verter_items) = if provider_only {
        (false, None)
    } else {
        verter_result
            .map(|result| (result.is_incomplete, Some(result.items)))
            .unwrap_or((false, None))
    };
    // B4: typed detail for `v-bind(|)` completions — the style position has
    // no TSX projection, so each offered binding's type comes from a provider
    // quickinfo at its DECLARATION position (bounded; fail-closed to the
    // native kind detail when the mapping or provider is unavailable).
    let verter_items = match verter_items {
        Some(items) if is_style_v_bind_context(server, uri, position) => {
            Some(enrich_v_bind_completion_details(server, uri, items).await)
        }
        other => other,
    };

    if native_only {
        drop(native_edit_fence);
        return Ok(verter_items.map(|items| {
            CompletionResponse::List(CompletionList {
                is_incomplete: verter_is_incomplete,
                items,
            })
        }));
    }

    // Compute the cursor's source context once — template attribute, template
    // expression, script, or other.
    let (source_ctx, source_expr_context) = (|| {
        let native = native_snapshot.as_ref()?;
        let offset = native.line_index.position_to_offset(position)?;
        let context = classify_cursor_context_for_language(
            offset,
            &native.source,
            &native.blocks,
            native.analysis.as_ref(),
            CarrierTemplateLanguage::from_uri(uri.as_str()),
        );
        let source_ctx = match &context {
            CursorContext::Template(TemplateCursorContext::AttributeName { .. }) => {
                CompletionSourceContext::TemplateAttr
            }
            CursorContext::Template(
                TemplateCursorContext::Expression { .. } | TemplateCursorContext::Interpolation,
            ) => CompletionSourceContext::TemplateExpression,
            CursorContext::Script => CompletionSourceContext::Script,
            _ => CompletionSourceContext::Other,
        };
        let expression_context = source_ctx.computes_expression_context().then(|| {
            classify_expression_context_with_trigger(
                &native.source,
                offset as usize,
                trigger_character,
            )
        });
        Some((source_ctx, expression_context))
    })()
    .unwrap_or((CompletionSourceContext::Other, None));
    let carrier_source_snapshot = native_snapshot.as_ref().map(|native| native.source.clone());
    let is_template_attr_context = matches!(source_ctx, CompletionSourceContext::TemplateAttr);

    // The attested editor tsserver plugin is the typed owner for all script
    // completions and for template member lists. Script blocks must retain the
    // complete TypeScript experience (locals, globals, and actionable
    // auto-imports), while Verter owns bare template render-proxy scope.
    // VS Code merges completion providers, so returning Verter's enclosing
    // template scope for `obj.|` pollutes the plugin's precise properties with
    // unrelated locals.
    if matches!(
        server.type_provider_kind,
        crate::TypeProviderKind::EditorTsserver
    ) && (matches!(source_ctx, CompletionSourceContext::Script)
        || matches!(source_expr_context, Some(ExpressionContext::MemberAccess)))
    {
        return Ok(None);
    }

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
        let _ = server.ensure_imported_carrier_apis_synced(uri).await;
        let ctx = server.type_provider_context(uri);
        if ctx.is_none() {
            tracing::debug!("completion: no ide_context for {}", uri.as_str());
        }
        if let Some(ctx) = ctx {
            // TSX is always fresh in the type provider — synced eagerly in did_change.
            // Only DTS sync and diagnostics publishing are debounced (300ms via SyncCoordinator).

            let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
                position,
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            )
            // Completion-only fallback: the strict mapper legitimately returns None for a
            // zero-width member-access boundary (the cursor right after `obj.` sits OUTSIDE
            // any mapped run). The completion-only helper anchors on a mapped run whose
            // source extent ends exactly at the cursor or exactly before the operator, and
            // accepts ONLY when the generated TSX carries the matching `.`/`?.` operator at
            // that run's generated endpoint. It is consulted ONLY on strict None, ONLY here
            // in completion — no other feature path uses it.
            .or_else(|| {
                let carrier_source = carrier_source_snapshot.as_ref()?;
                merge::carrier_completion_member_boundary_offset(
                    position,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                    &ctx.tsx_content,
                    carrier_source,
                )
            });
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
            // Compute the expression sub-context for BOTH template expressions
            // and `<script setup>` positions, so member-access completion works
            // in scripts (`a.` → MemberAccess → dot-trigger + member filtering).
            let expr_context = if source_ctx.computes_expression_context() {
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

                // FENCED to template expressions only: the template render proxy
                // exposes only script bindings, so verter's own completions are
                // the correct set and the TypeProvider's globals are noise. In
                // SCRIPT, by contrast, TS globals and imports ARE valid — never
                // suppress the TypeProvider for a bare script identifier position.
                let skip_type_provider = !provider_only
                    && matches!(source_ctx, CompletionSourceContext::TemplateExpression)
                    && expr_context
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
                // Recover a "No content available" completion: the carrier surface
                // the provider needs is not currently materialised. The recovery
                // mechanism is engine-specific:
                //   * tsserver — the carrier is served from the publish store via
                //     the plugin (NOT an open buffer), so re-PUBLISH the carrier
                //     companions (the change notification fires inside
                //     `publish_carrier`) to refresh the store + evict the stale
                //     resolution; the carrier-companion open verbs are no-ops here.
                //   * tsgo — the carrier is an open buffer, so reopen it (close +
                //     open) and re-sync the API to re-establish the lost content.
                if matches!(
                    server.type_provider_kind,
                    crate::TypeProviderKind::Tsserver | crate::TypeProviderKind::Tsgo
                ) {
                    for retry_delay_ms in [50u64, 150, 300] {
                        let needs_retry = matches!(
                            type_completion_result,
                            Err(ref error) if error.message.contains("No content available")
                        );
                        if !needs_retry {
                            break;
                        }
                        tracing::debug!(
                            "completion: retrying completion after no-content error for {} (delay={}ms)",
                            ctx.tsx_path,
                            retry_delay_ms
                        );
                        if matches!(server.type_provider_kind, crate::TypeProviderKind::Tsserver) {
                            if let Some(canonical_id) = server.documents.get_canonical_id(uri) {
                                server.publish_carrier_to_external_ts(&canonical_id).await;
                            }
                        } else {
                            server.force_reopen_current_file_in_type_provider(uri).await;
                            server.sync_api_to_provider(uri).await;
                        }
                        let _ = server.ensure_imported_carrier_apis_synced(uri).await;
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
                            !provider_only,
                            if matches!(source_ctx, CompletionSourceContext::TemplateExpression) {
                                provider_only_template_scope.as_ref()
                            } else {
                                None
                            },
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
                                    !provider_only,
                                    if matches!(
                                        source_ctx,
                                        CompletionSourceContext::TemplateExpression
                                    ) {
                                        provider_only_template_scope.as_ref()
                                    } else {
                                        None
                                    },
                                );
                                if !retry_result.items.is_empty() {
                                    type_result = retry_result;
                                }
                            }
                        }

                        // Post-await validation: completion items produced against a
                        // surface that no longer matches must be DROPPED (fail
                        // closed) — VS Code re-requests after the debounced sync
                        // lands. The verter-only items are still served.
                        if !server.provider_context_still_valid(uri, &ctx) {
                            tracing::debug!(
                                "completion: dropping provider items — captured surface \
                                 no longer valid"
                            );
                            return Ok(verter_items.map(|items| {
                                CompletionResponse::List(CompletionList {
                                    is_incomplete: verter_is_incomplete,
                                    items,
                                })
                            }));
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
                            &ctx.carrier_line_index,
                            Some(&ctx.tsx_path),
                            tp.provider_id(),
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

        // Check if this item carries a provider-neutral resolve envelope and
        // needs a `completionItem/resolve` round-trip for auto-import. The
        // envelope is provider-agnostic: any registered provider that minted it
        // (TSGO, tsserver, extension) resolves through the SAME path here.
        if let Some(envelope) = data.get("verter_resolve") {
            if envelope.get("kind").and_then(|v| v.as_str()) == Some("type_provider") {
                if let Some(tp) = &server.type_provider {
                    if let (Some(envelope_provider_id), Some(provider_path), Some(provider_data)) = (
                        envelope.get("provider_id").and_then(|v| v.as_str()),
                        envelope.get("provider_path").and_then(|v| v.as_str()),
                        envelope.get("provider_data"),
                    ) {
                        // Fail CLOSED on a provider mismatch: an item minted by a
                        // provider that is no longer active (mid-session swap)
                        // must never be resolved against a different backend.
                        if envelope_provider_id != tp.provider_id() {
                            tracing::debug!(
                                "completion_resolve: envelope provider '{}' != active '{}', \
                                 skipping resolve",
                                envelope_provider_id,
                                tp.provider_id()
                            );
                            return Ok(item);
                        }

                        // The provider-pure resolve key is typed; deserialize it
                        // back into `CompletionResolveData`. A malformed/foreign
                        // key cannot be resolved — fail closed (leave unchanged).
                        let Ok(resolve_data) =
                            serde_json::from_value::<CompletionResolveData>(provider_data.clone())
                        else {
                            return Ok(item);
                        };

                        if let Ok(Some(resolve_result)) =
                            tp.resolve_completion(provider_path, resolve_data).await
                        {
                            // Lazy `completionItem/resolve` enrichment: fold the
                            // provider's resolved detail (signature) and
                            // documentation onto the item when it returned them.
                            // `None` leaves the list-time value untouched. The
                            // CompletionResolveResult contract carries these, so
                            // dropping them here would make the contract dishonest
                            // (review finding F4).
                            if let Some(detail) = resolve_result.detail.clone() {
                                item.detail = Some(detail);
                            }
                            if let Some(documentation) = resolve_result.documentation.clone() {
                                item.documentation =
                                    Some(Documentation::MarkupContent(MarkupContent {
                                        kind: MarkupKind::Markdown,
                                        value: documentation,
                                    }));
                            }
                            // Lazy label-detail / command enrichment: fold the
                            // provider's resolved label details and post-accept
                            // command onto the item when present. `None` leaves
                            // the list-time value untouched. The
                            // CompletionResolveResult contract carries these, so
                            // dropping them would make the contract dishonest.
                            //
                            // MERGE sub-field by sub-field rather than overwriting
                            // the whole `CompletionItemLabelDetails`: a resolve may
                            // refine ONE sub-field (e.g. the import `description`)
                            // while the LIST already carried the other (the inline
                            // signature `detail`). A whole-struct overwrite would
                            // drop the list-time sub-field the resolve didn't re-send.
                            if let Some(ld) = resolve_result.label_details.clone() {
                                item.label_details =
                                    Some(merge_resolved_label_details(item.label_details, ld));
                            }
                            if let Some(cmd) = resolve_result.command.clone() {
                                item.command = Some(Command {
                                    title: cmd.title,
                                    command: cmd.command,
                                    arguments: cmd.arguments,
                                });
                            }
                            if !resolve_result.additional_text_edits.is_empty() {
                                // The provider returned auto-import edits that MUST be placed. From
                                // here on, missing IDE context / carrier URI / document OR an
                                // unplaceable edit returns a STRUCTURED resolve error — never an
                                // apparently-successful item with the import edits silently dropped
                                // (which recreates "accepted completion but no import"). Map
                                // completely or reject.
                                let provider_edits: Vec<ProviderImportEdit> = resolve_result
                                    .additional_text_edits
                                    .iter()
                                    .map(|e| ProviderImportEdit {
                                        start: e.start,
                                        end: e.end,
                                        new_text: e.new_text.clone(),
                                    })
                                    .collect();
                                let resolved = resolve_provider_auto_import_edits(
                                    server,
                                    provider_path,
                                    &provider_edits,
                                )
                                .map_err(|reason| {
                                    tracing::warn!(
                                        "completion_resolve: rejecting auto-import for \
                                                 {provider_path}: {reason}"
                                    );
                                    completion_resolve_error(&reason)
                                })?;
                                // `None` ⇒ the provider path is not a resolvable Vue carrier (a
                                // self-file rune module such as a Svelte `.svelte.ts`); the carrier
                                // re-anchor does not apply. Fail closed: leave the item unchanged
                                // rather than error or synthesize a Vue block into a non-Vue source.
                                if let Some(edits) = resolved {
                                    item.additional_text_edits = Some(edits);
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
