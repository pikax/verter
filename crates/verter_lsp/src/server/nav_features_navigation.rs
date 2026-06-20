//! LSP navigation feature method bodies — definition, type-definition,
//! references, and rename.
//!
//! Free functions hosting the bodies of the `impl LanguageServer for
//! VerterLanguageServer` navigation methods that map source-position requests
//! onto the generated artifact and back (`goto_definition`,
//! `goto_type_definition`, `references`, `prepare_rename`, `rename`). The
//! hover/completion/completion-resolve bodies stay in the `nav_features`
//! sibling module; the trait impl block stays in `mod.rs`, where each method is
//! a 1-line stub delegating to the corresponding `handle_<method>` here.

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::features::definition::definition_at_position;
use crate::features::references::references_at_position;
use crate::features::rename::{prepare_rename, rename_at_position};
use crate::type_provider::merge;

use super::handler_guard::{block_in_place_if_available, HandlerGuard};
use super::VerterLanguageServer;

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
                    let encoding = server.position_encoding.read().clone();
                    let locations: Vec<Location> = type_defs
                        .into_iter()
                        .filter_map(|d| {
                            // Strip virtual suffixes so user navigates to .vue
                            let carrier_source_exists =
                                |p: &str| server.documents.host().get_source(p).is_some();
                            let target_path = merge::normalize_carrier_path_owned(
                                &d.path,
                                &carrier_source_exists,
                            );
                            let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                            // Same-file refs use the virtual-file LineIndex. When path
                            // normalization is a no-op the emitted URI IS the file the
                            // provider's byte offsets index, so read it back and convert.
                            // Fail closed otherwise — never manufacture a line-0 range.
                            let range = if d.path == tsx_path {
                                Range {
                                    start: vf_li.offset_to_position(d.start)?,
                                    end: vf_li.offset_to_position(d.end)?,
                                }
                            } else if target_path == d.path {
                                merge::resolve_external_target_range(
                                    &d.path,
                                    d.start,
                                    d.end,
                                    encoding.clone(),
                                    &|p: &str| {
                                        block_in_place_if_available(|| {
                                            server.documents.host().workspace_read().read_file(p)
                                        })
                                    },
                                )?
                            } else {
                                return None;
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
            if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                position,
                &ctx.carrier_line_index,
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
                        let carrier_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let barrel_resolver =
                            |path: &str, start: u32, end: u32| -> Option<Location> {
                                server.resolve_barrel_type_provider_location(path, start, end)
                            };
                        let negotiated_encoding = server.position_encoding.read().clone();
                        let merged = merge::merge_definitions_with_barrel_resolver(
                            verter_result,
                            type_defs,
                            &ctx.tsx_path,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            uri,
                            &carrier_source_exists,
                            Some(&barrel_resolver),
                            negotiated_encoding,
                            &|p: &str| {
                                block_in_place_if_available(|| {
                                    server.documents.host().workspace_read().read_file(p)
                                })
                            },
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
                    let encoding = server.position_encoding.read().clone();
                    let locations: Vec<Location> = type_defs
                        .into_iter()
                        .filter_map(|d| {
                            let carrier_source_exists =
                                |p: &str| server.documents.host().get_source(p).is_some();
                            if let Some(location) = server
                                .resolve_barrel_type_provider_location(&d.path, d.start, d.end)
                            {
                                return Some(location);
                            }
                            let target_path = merge::normalize_carrier_path_owned(
                                &d.path,
                                &carrier_source_exists,
                            );
                            let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                            // Same-file refs use the virtual-file LineIndex. When path
                            // normalization is a no-op the emitted URI IS the file the
                            // provider's byte offsets index, so read it back and convert.
                            // Fail closed otherwise — never manufacture a line-0 range.
                            let range = if d.path == tsx_path {
                                Range {
                                    start: vf_li.offset_to_position(d.start)?,
                                    end: vf_li.offset_to_position(d.end)?,
                                }
                            } else if target_path == d.path {
                                merge::resolve_external_target_range(
                                    &d.path,
                                    d.start,
                                    d.end,
                                    encoding.clone(),
                                    &|p: &str| {
                                        block_in_place_if_available(|| {
                                            server.documents.host().workspace_read().read_file(p)
                                        })
                                    },
                                )?
                            } else {
                                return None;
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
            if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                position,
                &ctx.carrier_line_index,
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
                        let carrier_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let barrel_resolver =
                            |path: &str, start: u32, end: u32| -> Option<Location> {
                                server.resolve_barrel_type_provider_location(path, start, end)
                            };
                        let negotiated_encoding = server.position_encoding.read().clone();
                        return Ok(merge::merge_definitions_with_barrel_resolver(
                            None,
                            type_defs,
                            &ctx.tsx_path,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            uri,
                            &carrier_source_exists,
                            Some(&barrel_resolver),
                            negotiated_encoding,
                            &|p: &str| {
                                block_in_place_if_available(|| {
                                    server.documents.host().workspace_read().read_file(p)
                                })
                            },
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
                    let encoding = server.position_encoding.read().clone();
                    let locations: Vec<Location> = type_refs
                        .into_iter()
                        .filter_map(|r| {
                            let carrier_source_exists =
                                |p: &str| server.documents.host().get_source(p).is_some();
                            let target_path = merge::normalize_carrier_path_owned(
                                &r.path,
                                &carrier_source_exists,
                            );
                            let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                            // Same-file refs use the virtual-file LineIndex. When path
                            // normalization is a no-op the emitted URI IS the file the provider's
                            // byte offsets index, so read it back and convert in the negotiated
                            // encoding. Fail closed otherwise — never manufacture a line-0 range.
                            let range = if r.path == tsx_path {
                                Range {
                                    start: vf_li.offset_to_position(r.start)?,
                                    end: vf_li.offset_to_position(r.end)?,
                                }
                            } else if target_path == r.path {
                                merge::resolve_external_target_range(
                                    &r.path,
                                    r.start,
                                    r.end,
                                    encoding.clone(),
                                    &|p: &str| {
                                        block_in_place_if_available(|| {
                                            server.documents.host().workspace_read().read_file(p)
                                        })
                                    },
                                )?
                            } else {
                                return None;
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
            if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                position,
                &ctx.carrier_line_index,
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
                        let carrier_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let negotiated_encoding = server.position_encoding.read().clone();
                        return Ok(merge::merge_references(
                            verter_result,
                            type_refs,
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

    // PRODUCTION sync-before-query: the cross-file rename declaration surfaces via
    // the imported component's `{carrier}.ts` PUBLIC-API surface, which the
    // provider must already hold before the query. Peer navigation handlers
    // (`handle_goto_definition`) sync first; rename MUST too, or a closed child
    // carrier's API surface is never live and the rename drops the child edit.
    // Run BEFORE the fence so the sync's own provider commands are written, then
    // pin the resulting generations under the fence.
    if !server.is_self_file_projection(uri) {
        server.ensure_provider_synced(uri).await;
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
    //
    // GATED OFF for a SELF-FILE rune-module own buffer: its workspace-EDIT
    // positions are not yet mapped through the self-file mapper, so a returned
    // edit could land off by the prelude offset (or inside the prelude) and
    // CORRUPT the module. Rename stays DEFERRED for the self-file projection —
    // a clean no-op, never a wrong/unmapped edit. (Carrier rename unchanged.)
    if !server.is_self_file_projection(uri) {
        if let Some(tp) = &server.type_provider {
            if let Some(ctx) = server.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                    position,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    // FENCE: hold the provider fence across snapshot-capture →
                    // query → response so the captured carrier-API generations are
                    // the generations the provider's offsets are interpreted
                    // against, with no concurrent rename transaction interleaving
                    // its own surface mutations mid-capture.
                    let _fence = server.rename_provider_fence.lock().await;

                    // Capture the immutable carrier-API snapshot set BEFORE the
                    // query. A returned `{carrier}.ts` location maps ONLY against
                    // the snapshot captured here for that exact path + generation;
                    // a path absent from the set, or whose generation was later
                    // superseded by a racing background sync, fails closed (drop).
                    let query_snapshot = server
                        .documents
                        .provider_surfaces()
                        .capture_current_carrier_api_set();

                    if let Ok(type_locs) = tp.get_rename_locations(&ctx.tsx_path, tsx_offset).await
                    {
                        let carrier_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let negotiated_encoding = server.position_encoding.read().clone();
                        let api_resolver = |api_path: &str| {
                            // 3-state classification — the fail-closed distinction a bare `Option`
                            // could not make — routed through the single store-owned policy
                            // `classify_captured_api_surface`, which reads ONLY the captured snapshot
                            // pinned above (ZERO live-store reads between capture and merge):
                            //   • captured Current (CarrierApi at capture) + context builds → Vouched
                            //     (map onto the `.vue` through THAT captured generation's source map).
                            //   • captured Current but no source map → VirtualDrop (fail closed).
                            //   • captured KnownNonMappable (Closing at capture, or a non-CarrierApi
                            //     Current) → VirtualDrop. The store knew the path as virtual; its
                            //     offsets index VIRTUAL content. Capturing the Closing state here (the
                            //     prior capture skipped it, forcing a live re-consult) closes the
                            //     third TOCTOU: a background close `finalize_close`ing the path AFTER
                            //     capture but BEFORE classify can no longer flip it to NotVirtual and
                            //     corrupt a same-named real `{carrier}.ts`.
                            //   • ABSENT from the capture → NotVirtual (a genuinely real same-named
                            //     file the store did not know as virtual; edit it in place).
                            crate::provider_surface_store::classify_captured_api_surface(
                                &query_snapshot,
                                api_path,
                                negotiated_encoding.clone(),
                            )
                        };
                        return Ok(merge::merge_rename_locations(
                            verter_result,
                            type_locs,
                            new_name,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            Some(&|ide_path: &str| server.external_ide_context(ide_path)),
                            Some(&api_resolver),
                            &carrier_source_exists,
                            negotiated_encoding.clone(),
                            &|p: &str| {
                                block_in_place_if_available(|| {
                                    server.documents.host().workspace_read().read_file(p)
                                })
                            },
                        ));
                    }
                }
            }
        }
    }

    Ok(verter_result)
}
