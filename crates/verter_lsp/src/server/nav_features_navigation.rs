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

use super::component_resolve::ChildPropRenameClass;
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

                    if let Ok(mut type_locs) =
                        tp.get_rename_locations(&ctx.tsx_path, tsx_offset).await
                    {
                        // PROVIDER-AGNOSTIC child-declaration synthesis. A
                        // cross-file `<Child prop=…>` rename must edit BOTH the
                        // parent usage AND the child's `defineProps` declaration.
                        // The provider's own `textDocument/rename` does not reliably
                        // enumerate the child-declaration leg across the synthesized
                        // `{carrier}.ts` API surface (tgo does not), so Verter
                        // synthesizes that leg itself — from its OWN Vue resolution +
                        // the pinned snapshot's source map — giving the child edit a
                        // single deterministic Verter-owned origin for BOTH providers.
                        //
                        // 3-STATE classification (NOT a lossy `Option`): a confirmed
                        // child-prop rename whose synthesis leg cannot be produced is
                        // distinct from "not a child prop at all". Only a `Renameable`
                        // class attempts synthesis; the resulting `WorkspaceEdit` is
                        // then run through the post-merge COMPLETENESS GATE below.
                        let rename_class = server.classify_child_prop_rename(uri, position);
                        if let ChildPropRenameClass::ChildPropButNoSafeDeclaration { reason } =
                            &rename_class
                        {
                            // A confirmed child-prop rename with no synthesizable
                            // declaration leg: we do NOT synthesize and do NOT gate the
                            // provider's own result. Surface WHY (the reason) for
                            // diagnostics.
                            tracing::debug!(
                                "rename: child prop has no safe synthesizable declaration ({reason:?}); \
                                 deferring to provider result without synthesis"
                            );
                        }
                        if let ChildPropRenameClass::Renameable(target) = &rename_class {
                            if let Some(snapshot) =
                                query_snapshot.snapshot_for(&target.usage.child_carrier_api_path)
                            {
                                if let Some((start, end)) =
                                    crate::provider_surface_store::locate_prop_decl_range_in_carrier_api(
                                        snapshot,
                                        target.child_prop_decl_span,
                                        &target.usage.parent_prop_name,
                                    )
                                {
                                    inject_synthesized_carrier_rename_location(
                                        &mut type_locs,
                                        &target.usage.child_carrier_api_path,
                                        start,
                                        end,
                                    );
                                }
                            }
                        }

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
                        let merged = merge::merge_rename_locations(
                            verter_result,
                            type_locs,
                            new_name,
                            &ctx.tsx_path,
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
                        );

                        // POST-MERGE COMPLETENESS GATE (fail-closed): for a CONFIRMED
                        // (`Renameable`) cross-file child-prop rename, the EMITTED
                        // `WorkspaceEdit` must edit BOTH the child `.vue` prop
                        // declaration AND the parent `.vue` usage, or the whole rename
                        // fails closed (returns no edit) — never a usage-only /
                        // decl-only partial. See `gate_cross_file_child_prop_rename`.
                        return Ok(gate_cross_file_child_prop_rename(
                            merged,
                            &rename_class,
                            new_name,
                        ));
                    }
                }
            }
        }
    }

    Ok(verter_result)
}

/// Inject Verter's SYNTHESIZED child-declaration carrier rename location into the
/// provider's rename-location set, deduplicating against the provider's OWN
/// carrier location(s) for the SAME prop declaration.
///
/// The synthesized location and a provider's real carrier location (tsserver
/// returns one; tgo does not) target the SAME `{carrier}.ts` byte range for the
/// same prop, so the synthesized leg must be the SINGLE deterministic origin.
///
/// DEDUP BY OVERLAP, not just exact equality: drop EVERY provider location on the
/// SAME carrier path whose byte range `[start, end)` OVERLAPS the synthesized
/// `[start, end)`, in favor of the synthesized one. An exact-only dedup leaves a
/// provider location that shares the synthesized START but has a DIFFERENT END
/// (e.g. the provider ranged the whole `name: type` member, or only the bare
/// name) in the set; the downstream merge dedups carrier edits by mapped-`.vue`
/// `range.start`, so that same-start-different-end provider location would SUPPRESS
/// the synthesized edit (whichever lands first wins the start slot) and the child
/// decl could map to a wrong/under-covering range. Pruning by overlap here keeps
/// the synthesized leg the authoritative origin for that declaration.
///
/// NARROW: only same-path, range-OVERLAPPING provider locations are pruned. Every
/// OTHER provider location — a different-range carrier ref the Vue-prop synthesis
/// does not model, or any location on a different path (the parent usage) — is
/// PRESERVED. This narrowing is RENAME-SYNTHESIS-LOCAL; it does not touch the
/// shared `merge_rename_locations` `range.start` dedup (which other features rely
/// on).
fn inject_synthesized_carrier_rename_location(
    type_locs: &mut Vec<crate::type_provider::protocol::RenameLocation>,
    carrier_api_path: &str,
    start: u32,
    end: u32,
) {
    type_locs.retain(|loc| {
        // Keep unless it is on the SAME carrier path AND its range overlaps the
        // synthesized one. Half-open overlap: `a.start < b.end && b.start < a.end`.
        !(loc.path == carrier_api_path && loc.start < end && start < loc.end)
    });
    type_locs.push(crate::type_provider::protocol::RenameLocation {
        path: carrier_api_path.to_string(),
        start,
        end,
    });
}

/// Whether the merged cross-file rename `WorkspaceEdit` actually contains the
/// SOURCE edits a confirmed `<Child prop=…>` rename MUST produce — the
/// post-merge COMPLETENESS GATE.
///
/// A confirmed (Renameable) child-prop rename is incomplete unless the EMITTED
/// `WorkspaceEdit` edits BOTH:
///   1. the child component's `.vue` prop DECLARATION at `expected_child_range`
///      with `new_text == new_name`, AND
///   2. the parent's `.vue` prop USAGE at `expected_parent_range` with
///      `new_text == new_name`.
///
/// PROVIDER-AGNOSTIC by construction: it inspects only the merged, mapped source
/// `WorkspaceEdit` (`changes: HashMap<Uri, Vec<TextEdit>>`) — it does NOT care
/// whether the child edit came from Verter's synthesis or from the provider's own
/// native leg (tsserver enumerates it; tgo does not). So a tsserver rename whose
/// native child leg is present passes even when Verter's own synthesis could not
/// locate the leg — no `is_tsgo`/`is_tsserver` branch, no regression.
///
/// Each `expected_*` range is `None` when the originating span did not resolve to
/// a `.vue` position; the gate then cannot prove that leg precisely and FAILS
/// CLOSED (returns `false`) — the fail-closed boundary for an unmappable edit.
///
/// Range match is EXACT on `range.start` (the edit anchor), tolerant on `end`: an
/// edit at the right start position with the right new text is the declaration /
/// usage edit. A `new_text` mismatch (a stray edit at the same anchor with
/// different text) does NOT satisfy the leg.
fn workspace_edit_satisfies_child_prop_rename(
    merged: &WorkspaceEdit,
    expected_child_uri: &Uri,
    expected_child_range: Option<Range>,
    expected_parent_uri: &Uri,
    expected_parent_range: Option<Range>,
    new_name: &str,
) -> bool {
    let has_edit_at = |uri: &Uri, expected: Option<Range>| -> bool {
        let Some(expected) = expected else {
            // No precise range to assert → cannot prove this leg → fail closed.
            return false;
        };
        let Some(changes) = merged.changes.as_ref() else {
            return false;
        };
        let Some(edits) = changes.get(uri) else {
            return false;
        };
        edits
            .iter()
            .any(|e| e.range.start == expected.start && e.new_text == new_name)
    };

    has_edit_at(expected_child_uri, expected_child_range)
        && has_edit_at(expected_parent_uri, expected_parent_range)
}

/// Apply the post-merge COMPLETENESS GATE to a cross-file rename result.
///
/// - [`ChildPropRenameClass::Renameable`]: the merged `WorkspaceEdit` MUST satisfy
///   [`workspace_edit_satisfies_child_prop_rename`] (edits BOTH the child `.vue`
///   declaration AND the parent `.vue` usage). If it does not, the whole rename
///   fails closed → `None`. This is the fix for the usage-only-partial gap: a
///   confirmed child-prop rename whose merged edit lacks the child declaration
///   (e.g. tgo, synthesis leg could not be produced) returns NO edit rather than a
///   usage-only partial. Provider-AGNOSTIC: a tsserver result whose NATIVE child
///   leg already lands the declaration edit passes the gate even when Verter's own
///   synthesis could not locate the leg (no `is_tsgo`/`is_tsserver` branch).
/// - [`ChildPropRenameClass::ChildPropButNoSafeDeclaration`] and
///   [`ChildPropRenameClass::NotChildProp`]: do NOT gate. The provider's own
///   merged result is returned untouched — these are not a confirmed
///   synthesizable-declaration rename Verter promises both legs for, so Verter must
///   not suppress an otherwise-valid provider result.
///
/// Inspects ONLY the merged source `WorkspaceEdit`, so it is a pure function of
/// `(merged, class, new_name)` — unit-testable without a live provider.
fn gate_cross_file_child_prop_rename(
    merged: Option<WorkspaceEdit>,
    rename_class: &ChildPropRenameClass,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let ChildPropRenameClass::Renameable(target) = rename_class else {
        return merged;
    };
    let satisfied = merged.as_ref().is_some_and(|edit| {
        workspace_edit_satisfies_child_prop_rename(
            edit,
            &target.usage.child_uri,
            target.expected_child_decl_range,
            &target.usage.parent_uri,
            target.expected_parent_usage_range,
            new_name,
        )
    });
    if satisfied {
        merged
    } else {
        None
    }
}

#[cfg(test)]
mod synthesized_rename_injection_tests {
    use super::inject_synthesized_carrier_rename_location;
    use crate::type_provider::protocol::RenameLocation;

    const API: &str = "/src/MyComp.vue.ts";

    fn loc(path: &str, start: u32, end: u32) -> RenameLocation {
        RenameLocation {
            path: path.to_string(),
            start,
            end,
        }
    }

    fn count_matching(locs: &[RenameLocation], path: &str, start: u32, end: u32) -> usize {
        locs.iter()
            .filter(|l| l.path == path && l.start == start && l.end == end)
            .count()
    }

    #[test]
    fn dedups_provider_location_for_same_prop_decl_to_exactly_one() {
        // The provider (tsserver) ALSO returned the carrier location for the SAME
        // prop declaration the synthesis targets.
        let mut locs = vec![loc(API, 40, 43)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        // EXACTLY one — discriminating: WITHOUT the dedup `retain` this is 2
        // (the provider's + the synthesized), a duplicate child edit.
        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "the child-declaration carrier edit must appear exactly once (one deterministic origin)"
        );
    }

    #[test]
    fn preserves_other_provider_locations() {
        // The provider returned the matching carrier decl AND other valid locations
        // (the parent usage in App.vue.tsx, and a DIFFERENT-range carrier ref the
        // Vue-prop synthesis does not model).
        let app = "/src/App.vue.tsx";
        let mut locs = vec![
            loc(app, 1000, 1003), // parent usage — must survive
            loc(API, 40, 43),     // same prop decl — deduped against synthesis
            loc(API, 80, 83),     // a different carrier ref — must survive
        ];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);

        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "the synthesized prop-decl edit is the single origin"
        );
        assert_eq!(
            count_matching(&locs, app, 1000, 1003),
            1,
            "an unrelated provider location (parent usage) must be preserved"
        );
        assert_eq!(
            count_matching(&locs, API, 80, 83),
            1,
            "a different-range provider carrier location must be preserved (not broadly dropped)"
        );
    }

    #[test]
    fn injects_when_provider_did_not_report_the_child_decl() {
        // tgo: the provider did NOT enumerate the child-declaration leg, so the
        // synthesized location is the ONLY one for the prop decl — it must be added.
        let mut locs: Vec<RenameLocation> = vec![loc("/src/App.vue.tsx", 1000, 1003)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "the synthesized child-declaration leg must be injected when the provider omits it"
        );
    }

    #[test]
    fn prunes_same_start_different_end_provider_location_by_overlap() {
        // The provider returned a carrier location for the SAME prop declaration but
        // with a DIFFERENT end (it ranged `foo: string`, bytes 40..51, where the
        // synthesis ranges only the name `foo`, 40..43). The downstream merge dedups
        // carrier edits by mapped-`.vue` `range.start`; a same-start provider edit
        // left in the set would SUPPRESS the synthesized one (whichever lands first
        // wins the start slot) → the child decl could map to a wrong/over-covering
        // range. The overlap-prune must drop the provider's overlapping location.
        let mut locs = vec![loc(API, 40, 51)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        // DISCRIMINATING: with the OLD exact-only `retain` this is 2 (the provider's
        // 40..51 survives alongside the synthesized 40..43); with overlap-prune it is 1.
        assert_eq!(
            locs.len(),
            1,
            "a same-start-different-end provider carrier location must be pruned by overlap"
        );
        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "only the synthesized exact-name range survives"
        );
        assert_eq!(
            count_matching(&locs, API, 40, 51),
            0,
            "the provider's overlapping (wider) range must be dropped, not kept"
        );
    }

    #[test]
    fn prunes_partial_overlap_provider_location() {
        // A provider location that PARTIALLY overlaps the synthesized range (44..47
        // vs synthesized 40..46 — provider start inside the synthesized range) must
        // also be pruned: it would map into the same declaration region.
        let mut locs = vec![loc(API, 44, 47)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 46);
        assert_eq!(
            locs.len(),
            1,
            "a partially-overlapping provider carrier location must be pruned"
        );
        assert_eq!(count_matching(&locs, API, 40, 46), 1);
    }

    #[test]
    fn keeps_adjacent_non_overlapping_provider_location() {
        // A provider carrier location that is ADJACENT but does NOT overlap (43..46,
        // touching the synthesized 40..43 at the half-open boundary) is a DIFFERENT
        // reference the synthesis does not model — it must be PRESERVED (the narrowing
        // is overlap-only, never broader).
        let mut locs = vec![loc(API, 43, 46)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        assert_eq!(
            locs.len(),
            2,
            "an adjacent, non-overlapping provider carrier location must be preserved"
        );
        assert_eq!(count_matching(&locs, API, 40, 43), 1);
        assert_eq!(count_matching(&locs, API, 43, 46), 1);
    }
}

#[cfg(test)]
mod cross_file_rename_gate_tests {
    use super::{gate_cross_file_child_prop_rename, workspace_edit_satisfies_child_prop_rename};
    use crate::server::component_resolve::{
        ChildPropMissReason, ChildPropRenameClass, ChildPropRenameTarget, ChildPropUsage,
    };
    use std::collections::HashMap;
    use tower_lsp_server::ls_types::{Position, Range, TextEdit, Uri, WorkspaceEdit};

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    fn child_uri() -> Uri {
        uri("file:///src/MyComp.vue")
    }

    fn parent_uri() -> Uri {
        uri("file:///src/App.vue")
    }

    fn rng(line: u32, start_ch: u32, end_ch: u32) -> Range {
        Range {
            start: Position {
                line,
                character: start_ch,
            },
            end: Position {
                line,
                character: end_ch,
            },
        }
    }

    /// The child decl is at MyComp.vue 5:11..5:14, the parent usage at App.vue 3:9..3:12.
    fn child_decl_range() -> Range {
        rng(5, 11, 14)
    }
    fn parent_usage_range() -> Range {
        rng(3, 9, 12)
    }

    /// A `Renameable` class targeting the ranges above. Both expected ranges present.
    fn renameable_target() -> ChildPropRenameClass {
        ChildPropRenameClass::Renameable(Box::new(ChildPropRenameTarget {
            usage: ChildPropUsage {
                parent_uri: parent_uri(),
                parent_prop_name: "foo".to_string(),
                parent_prop_name_span: verter_span::Span { start: 0, end: 3 },
                parent_is_shorthand: false,
                child_uri: child_uri(),
                child_carrier_api_path: "/src/MyComp.vue.ts".to_string(),
            },
            child_prop_decl_span: verter_span::Span {
                start: 100,
                end: 103,
            },
            expected_child_decl_range: Some(child_decl_range()),
            expected_parent_usage_range: Some(parent_usage_range()),
        }))
    }

    fn edit_with(entries: Vec<(Uri, Vec<TextEdit>)>) -> WorkspaceEdit {
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for (u, edits) in entries {
            changes.insert(u, edits);
        }
        WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }
    }

    fn te(range: Range, new_text: &str) -> TextEdit {
        TextEdit {
            range,
            new_text: new_text.to_string(),
        }
    }

    // ── The load-bearing fail-closed discriminator (RED-proof target) ──────────

    #[test]
    fn renameable_usage_only_merge_fails_closed_to_none() {
        // A CONFIRMED child-prop rename whose merged edit contains ONLY the parent
        // usage leg (the tgo synthesis-failure shape: child leg dropped, provider
        // did not enumerate it). The gate MUST fail closed → None, never the
        // usage-only partial.
        //
        // DISCRIMINATING / RED-PROOF: revert the gate (make
        // `gate_cross_file_child_prop_rename` always return `merged`) and this goes
        // RED — it would return the usage-only edit instead of None.
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "fooRenamed")],
        )]);
        let result =
            gate_cross_file_child_prop_rename(Some(merged), &renameable_target(), "fooRenamed");
        assert!(
            result.is_none(),
            "a Renameable rename with a usage-ONLY merged edit must fail closed (None), \
             not ship a usage-only partial"
        );
    }

    #[test]
    fn renameable_decl_only_merge_fails_closed_to_none() {
        // A declaration-ONLY merged edit (parent usage leg missing) is ALSO
        // incomplete → fail closed. Guards the optional-but-adopted parent-usage
        // assertion.
        let merged = edit_with(vec![(
            child_uri(),
            vec![te(child_decl_range(), "fooRenamed")],
        )]);
        let result =
            gate_cross_file_child_prop_rename(Some(merged), &renameable_target(), "fooRenamed");
        assert!(
            result.is_none(),
            "a Renameable rename with a declaration-ONLY merged edit must fail closed (None)"
        );
    }

    #[test]
    fn renameable_both_legs_present_is_returned() {
        // BOTH legs present at the expected ranges with the right new text → the gate
        // passes and returns the merged edit unchanged.
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (child_uri(), vec![te(child_decl_range(), "fooRenamed")]),
        ]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged.clone()),
            &renameable_target(),
            "fooRenamed",
        );
        let returned = result.expect("a complete Renameable rename (both legs) must be returned");
        let changes = returned.changes.expect("changes present");
        assert!(
            changes.contains_key(&parent_uri()) && changes.contains_key(&child_uri()),
            "the returned edit must keep both the parent usage and child declaration legs"
        );
    }

    #[test]
    fn renameable_child_leg_from_provider_passes_without_synthesis() {
        // tsserver case (c): Verter's own synthesis could not run, but the MERGED
        // edit ALREADY contains the child declaration leg (the provider's native
        // leg). The gate is provider-AGNOSTIC — it inspects the merged result, not
        // whether synthesis ran — so it PASSES. This proves the gate does NOT
        // regress tsserver when synthesis is absent.
        //
        // (Identical merged shape to `renameable_both_legs_present_is_returned`; the
        // DISTINCTION this test characterizes is intent: the child leg's ORIGIN is
        // irrelevant to the gate. Asserted via the same both-legs-present input.)
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (child_uri(), vec![te(child_decl_range(), "fooRenamed")]),
        ]);
        let result =
            gate_cross_file_child_prop_rename(Some(merged), &renameable_target(), "fooRenamed");
        assert!(
            result.is_some(),
            "a Renameable rename whose child leg is present (from the provider) must pass the gate \
             even without Verter synthesis (no provider regression)"
        );
    }

    #[test]
    fn renameable_wrong_new_text_at_child_range_fails_closed() {
        // An edit at the right child START but with the WRONG new text does NOT
        // satisfy the child leg → fail closed. Guards against a stray same-anchor edit
        // masquerading as the declaration edit.
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (child_uri(), vec![te(child_decl_range(), "WRONG")]),
        ]);
        let result =
            gate_cross_file_child_prop_rename(Some(merged), &renameable_target(), "fooRenamed");
        assert!(
            result.is_none(),
            "an edit at the child decl anchor with the wrong new_text must NOT satisfy the gate"
        );
    }

    #[test]
    fn not_child_prop_does_not_gate_usage_only_result() {
        // A NotChildProp rename (e.g. a local binding) is NOT a confirmed
        // synthesizable-declaration rename: the gate must NOT touch the provider's own
        // merged result, even a single-file one. DISCRIMINATING against an
        // over-broad gate that would suppress valid non-child renames.
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "renamed")],
        )]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &ChildPropRenameClass::NotChildProp,
            "renamed",
        );
        assert!(
            result.is_some(),
            "a NotChildProp rename's merged result must be returned untouched (no over-gating)"
        );
    }

    #[test]
    fn child_prop_but_no_safe_declaration_does_not_gate() {
        // A confirmed child prop with NO synthesizable declaration leg
        // (`ChildPropButNoSafeDeclaration`) does NOT gate: Verter did not promise a
        // synthesized declaration for it, so it must not suppress the provider's own
        // (possibly complete) result. Reads `reason` to characterize the variant.
        let class = ChildPropRenameClass::ChildPropButNoSafeDeclaration {
            reason: ChildPropMissReason::NoMacroDeclaration,
        };
        if let ChildPropRenameClass::ChildPropButNoSafeDeclaration { reason } = &class {
            assert_eq!(*reason, ChildPropMissReason::NoMacroDeclaration);
        } else {
            panic!("expected ChildPropButNoSafeDeclaration");
        }
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "renamed")],
        )]);
        let result = gate_cross_file_child_prop_rename(Some(merged), &class, "renamed");
        assert!(
            result.is_some(),
            "ChildPropButNoSafeDeclaration must not gate the provider's own result"
        );
    }

    #[test]
    fn renameable_unmappable_expected_range_fails_closed() {
        // If the child decl's `.vue` range could not be computed
        // (`expected_child_decl_range == None`), the gate cannot prove the leg
        // precisely and FAILS CLOSED — the fail-closed boundary for an unmappable
        // edit. Even a both-files-touched merged edit does not satisfy it.
        let mut class = renameable_target();
        if let ChildPropRenameClass::Renameable(target) = &mut class {
            target.expected_child_decl_range = None;
        }
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (child_uri(), vec![te(child_decl_range(), "fooRenamed")]),
        ]);
        let result = gate_cross_file_child_prop_rename(Some(merged), &class, "fooRenamed");
        assert!(
            result.is_none(),
            "a Renameable rename with no precise child range must fail closed (None)"
        );
    }

    #[test]
    fn satisfies_helper_requires_both_legs() {
        // Direct unit of the satisfaction predicate: both legs → true; missing
        // either → false; None range → false.
        let both = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "x")]),
            (child_uri(), vec![te(child_decl_range(), "x")]),
        ]);
        assert!(workspace_edit_satisfies_child_prop_rename(
            &both,
            &child_uri(),
            Some(child_decl_range()),
            &parent_uri(),
            Some(parent_usage_range()),
            "x"
        ));
        // Missing child leg → false.
        let usage_only = edit_with(vec![(parent_uri(), vec![te(parent_usage_range(), "x")])]);
        assert!(!workspace_edit_satisfies_child_prop_rename(
            &usage_only,
            &child_uri(),
            Some(child_decl_range()),
            &parent_uri(),
            Some(parent_usage_range()),
            "x"
        ));
        // None expected child range → false (fail closed).
        assert!(!workspace_edit_satisfies_child_prop_rename(
            &both,
            &child_uri(),
            None,
            &parent_uri(),
            Some(parent_usage_range()),
            "x"
        ));
    }
}
