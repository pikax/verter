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

use super::child_prop_rename::{ChildPropDeclarationProof, ChildPropRenameClass};
use super::handler_guard::{block_in_place_if_available, HandlerGuard};
use super::server_utils::location_from_span;
use super::VerterLanguageServer;

/// Resolve a named export's declaration `Location` in a REAL source file through
/// the host's export tables (re-export chains followed). Fail-closed `None` when
/// the export, source, or position conversion is unavailable.
fn host_export_location(
    server: &VerterLanguageServer,
    canonical_id: &str,
    binding_name: &str,
) -> Option<Location> {
    let host = &server.documents.host;
    let (resolved_id, start, end) = host
        .get_export_span_follow_reexports(canonical_id, binding_name)
        .or_else(|| {
            let (s, e) = host.get_export_span(canonical_id, binding_name)?;
            Some((canonical_id.to_string(), s, e))
        })?;
    let source = host.get_source(&resolved_id)?;
    let encoding = server.position_encoding.read().clone();
    let li = LineIndex::new(&source, encoding);
    let range = Range {
        start: li.offset_to_position(start)?,
        end: li.offset_to_position(end)?,
    };
    // Absolute paths only (POSIX-rooted or Windows-drive); relative/virtual ids
    // never become locations. The shared owner util percent-encodes, so a path
    // with spaces/non-ASCII still parses into a valid `Uri` instead of silently
    // dropping this definition leg.
    let normalized = resolved_id.replace('\\', "/");
    if !normalized.starts_with('/') && normalized.chars().nth(1) != Some(':') {
        return None;
    }
    Some(Location {
        uri: crate::uri::path_to_file_uri(&resolved_id)?,
        range,
    })
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

    // Readiness protocol (never a starter): capture the DependencyReady
    // receipt for the live revision, JOIN an in-flight background publication,
    // or enqueue one and answer without the provider. The handler must never
    // run the import-set/barrel walk inline — a request cancelled at its
    // deadline would kill the walk and un-mint the receipt (the cancel loop).
    let dependency_readiness = server.dependency_readiness_join(uri).await;

    // Virtual file: route directly through TSGO (position is already in TSX coordinates)
    if let Some(tp) = server
        .type_provider
        .as_ref()
        .filter(|_| dependency_readiness.is_ready())
    {
        if let Some(vf_ctx) = server.virtual_file_context(uri) {
            let tsx_path = vf_ctx.tsx_path.clone();
            let vf_li = vf_ctx.line_index.clone();
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_defs) = tp.get_definition(&tsx_path, offset).await {
                    // Post-await validation (fail closed): a response produced
                    // against a superseded surface must not be mapped.
                    if !server.virtual_request_surface_still_valid(uri, &vf_ctx) {
                        return Ok(None);
                    }
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
        match def {
            GotoDefinitionResponse::Scalar(ref mut loc) => {
                if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI_STR {
                    loc.uri = uri.clone();
                }
            }
            GotoDefinitionResponse::Array(ref mut locs) => {
                for loc in locs.iter_mut() {
                    if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI_STR {
                        loc.uri = uri.clone();
                    }
                }
            }
            GotoDefinitionResponse::Link(_) => {}
        }

        Some(def)
    })();

    tracing::debug!("definition: verter found={}", verter_result.is_some());

    // B4: a GLOBAL css class token (declared non-scoped / :global) extends its
    // definition targets with every global declaration workspace-wide.
    if let Some(class_name) = super::nav_features_css::global_class_target(server, uri, position) {
        return Ok(super::nav_features_css::merge_global_class_definitions(
            server,
            uri,
            &class_name,
            verter_result,
        ));
    }

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

    // Enhance with TypeProvider for cross-file definitions — only under a
    // committed (or joined) DependencyReady receipt: querying over a missing
    // import closure returns wrong/empty cross-file answers, and the honest
    // disposition for a not-ready revision is the native result (VS Code
    // re-queries once background publication mints the receipt).
    // Extract all context synchronously — no DashMap guard held across await.
    if let Some(tp) = server
        .type_provider
        .as_ref()
        .filter(|_| dependency_readiness.is_ready())
    {
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
                // Pin the FOREIGN carrier IDE surfaces BEFORE the query, so a
                // returned foreign location maps through the generation the
                // request began against (never the merge-time current one).
                let foreign_ide_set = server.capture_foreign_carrier_ide_set();
                match tp.get_definition(&ctx.tsx_path, tsx_offset).await {
                    Ok(type_defs) => {
                        // Post-await validation: a response produced against a
                        // surface that no longer matches must be DROPPED (fail
                        // closed), never mapped through a superseded context.
                        if !server.provider_context_still_valid(uri, &ctx) {
                            tracing::debug!(
                                "definition: dropping provider locations — captured surface \
                                 no longer valid"
                            );
                            return Ok(verter_result);
                        }
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
                        let provider_had_defs = !type_defs.is_empty();
                        // GlobalComponents fallback-const NAV PROBE offsets for
                        // any same-file synthetic targets, located through the
                        // compiler-owned emission-contract reader BEFORE the
                        // merge consumes the response (fail-closed `None` for
                        // every non-fallback-const target).
                        let nav_probe_offsets: Vec<u32> = type_defs
                            .iter()
                            .filter(|d| d.path == ctx.tsx_path)
                            .filter_map(|d| {
                                verter_session::global_component_nav_probe_offset(
                                    &ctx.tsx_content,
                                    d.start,
                                    d.end,
                                )
                            })
                            .collect();
                        let merged = merge::merge_definitions_with_barrel_resolver(
                            verter_result,
                            type_defs,
                            &ctx.tsx_path,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            Some(&|ide_path: &str| {
                                server.foreign_ide_context(&foreign_ide_set, ide_path)
                            }),
                            uri,
                            &carrier_source_exists,
                            Some(&barrel_resolver),
                            negotiated_encoding.clone(),
                            &|p: &str| {
                                block_in_place_if_available(|| {
                                    server.documents.host().workspace_read().read_file(p)
                                })
                            },
                        );
                        // If the type provider resolved to a barrel file, follow
                        // re-exports to the terminal declaration.
                        let resolved = server.resolve_barrel_locations(merged);

                        // Synthetic-target fallback: the provider RESOLVED the
                        // identifier, but every returned declaration was dropped
                        // by the fail-closed merge — the targets live in
                        // unmapped generated text. When those targets are
                        // GlobalComponents fallback consts (a template tag whose
                        // binding is a synthesized const), re-issue `definition`
                        // at the const's NAV PROBE member — the
                        // (augmentation-merged) `GlobalComponents` interface
                        // member — so the tag jumps to the user's real
                        // registration declaration. An unregistered tag has no
                        // member symbol: the probe yields nothing and the result
                        // stays fail-closed EMPTY. Positions whose definition
                        // the provider could not resolve at all (`type_defs`
                        // empty) never enter this branch.
                        let resolved_is_empty = match &resolved {
                            None => true,
                            Some(GotoDefinitionResponse::Array(locs)) => locs.is_empty(),
                            Some(GotoDefinitionResponse::Link(links)) => links.is_empty(),
                            Some(GotoDefinitionResponse::Scalar(_)) => false,
                        };
                        if !(provider_had_defs && resolved_is_empty) {
                            return Ok(resolved);
                        }
                        tracing::debug!(
                            "definition: all provider targets were synthetic — retrying {} \
                             GlobalComponents nav probe(s)",
                            nav_probe_offsets.len()
                        );
                        let mut probe_defs = Vec::new();
                        for probe_offset in nav_probe_offsets {
                            match tp.get_definition(&ctx.tsx_path, probe_offset).await {
                                Ok(defs) => probe_defs.extend(defs),
                                Err(e) => {
                                    tracing::warn!(
                                        "definition: GlobalComponents nav-probe query error: {e}"
                                    );
                                }
                            }
                        }
                        // Post-await validation (fail closed), same as above.
                        if !server.provider_context_still_valid(uri, &ctx) {
                            return Ok(None);
                        }
                        if probe_defs.is_empty() {
                            return Ok(None);
                        }
                        // A provider may follow the augmentation member THROUGH
                        // `typeof C` to the component's synthesized API carrier
                        // (`{name}.vue.verter.ts`) — a virtual path whose byte
                        // offsets the fail-closed merge cannot map. Resolve that
                        // leg natively: normalize the carrier path back to its
                        // REAL source file and take the component's
                        // default-export declaration span from the host's export
                        // tables. Unresolvable legs drop (fail closed).
                        let mut native_locations: Vec<Location> = Vec::new();
                        probe_defs.retain(|d| {
                            let normalized = merge::normalize_carrier_path_owned(
                                &d.path,
                                &carrier_source_exists,
                            );
                            if normalized == d.path {
                                return true;
                            }
                            if let Some(loc) = host_export_location(server, &normalized, "default")
                            {
                                native_locations.push(loc);
                            }
                            false
                        });
                        if probe_defs.is_empty() {
                            return Ok(if native_locations.is_empty() {
                                None
                            } else {
                                Some(GotoDefinitionResponse::Array(native_locations))
                            });
                        }
                        let merged = merge::merge_definitions_with_barrel_resolver(
                            None,
                            probe_defs,
                            &ctx.tsx_path,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.carrier_line_index,
                            Some(&|ide_path: &str| {
                                server.foreign_ide_context(&foreign_ide_set, ide_path)
                            }),
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
                        let mut locations = match server.resolve_barrel_locations(merged) {
                            Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
                            Some(GotoDefinitionResponse::Array(locs)) => locs,
                            Some(GotoDefinitionResponse::Link(links)) => links
                                .into_iter()
                                .map(|link| Location {
                                    uri: link.target_uri,
                                    range: link.target_selection_range,
                                })
                                .collect(),
                            None => Vec::new(),
                        };
                        locations.extend(native_locations);
                        return Ok(if locations.is_empty() {
                            None
                        } else {
                            Some(GotoDefinitionResponse::Array(locations))
                        });
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

    // Readiness protocol (never a starter): capture / join / enqueue — the
    // handler must never run the import-set walk inline (see
    // `handle_goto_definition`).
    let dependency_readiness = server.dependency_readiness_join(uri).await;

    // Virtual file: route directly through type provider (position is already in TSX coordinates)
    if let Some(tp) = server
        .type_provider
        .as_ref()
        .filter(|_| dependency_readiness.is_ready())
    {
        if let Some(vf_ctx) = server.virtual_file_context(uri) {
            let tsx_path = vf_ctx.tsx_path.clone();
            let vf_li = vf_ctx.line_index.clone();
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_defs) = tp.get_type_definition(&tsx_path, offset).await {
                    // Post-await validation (fail closed): a response produced
                    // against a superseded surface must not be mapped.
                    if !server.virtual_request_surface_still_valid(uri, &vf_ctx) {
                        return Ok(None);
                    }
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

    // Type definition is purely a type provider operation — no verter analysis
    // phase. Query only under a committed (or joined) DependencyReady receipt;
    // a not-ready revision answers empty and background publication heals.
    if let Some(tp) = server
        .type_provider
        .as_ref()
        .filter(|_| dependency_readiness.is_ready())
    {
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
                // Pin the FOREIGN carrier IDE surfaces BEFORE the query (see
                // handle_goto_definition).
                let foreign_ide_set = server.capture_foreign_carrier_ide_set();
                match tp.get_type_definition(&ctx.tsx_path, tsx_offset).await {
                    Ok(type_defs) => {
                        // Post-await validation: a response produced against a
                        // surface that no longer matches must be DROPPED (fail
                        // closed), never mapped through a superseded context.
                        if !server.provider_context_still_valid(uri, &ctx) {
                            tracing::debug!(
                                "type_definition: dropping provider locations — captured \
                                 surface no longer valid"
                            );
                            return Ok(None);
                        }
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
                            Some(&|ide_path: &str| {
                                server.foreign_ide_context(&foreign_ide_set, ide_path)
                            }),
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
        if let Some(vf_ctx) = server.virtual_file_context(uri) {
            let tsx_path = vf_ctx.tsx_path.clone();
            let vf_li = vf_ctx.line_index.clone();
            if let Some(offset) = vf_li.position_to_offset(position) {
                if let Ok(type_refs) = tp.get_references(&tsx_path, offset).await {
                    // Post-await validation (fail closed): a response produced
                    // against a superseded surface must not be mapped.
                    if !server.virtual_request_surface_still_valid(uri, &vf_ctx) {
                        return Ok(None);
                    }
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

    // Cross-file child-prop DECLARATION injection (provider-agnostic): a
    // `<Child prop=…>` usage references the child's `defineProps` declaration,
    // but providers do not reliably enumerate it across the synthesized API
    // surface (the definition merge maps `{carrier}.verter.ts` locations
    // fail-closed). Verter resolves the declaration itself — the SAME shared
    // usage resolution the goto-definition props branch and the rename
    // classification consume, so the surfaces cannot drift. Honors the LSP
    // `includeDeclaration` contract: the declaration leg is injected only
    // when the caller asked for declarations.
    let child_prop_declaration = if include_declaration {
        (|| {
            let resolved = match server.resolve_child_prop_usage_at_cursor(uri, position) {
                super::child_prop_rename::ChildPropUsageClass::Resolved(resolved) => resolved,
                super::child_prop_rename::ChildPropUsageClass::NotChildProp => return None,
            };
            let decl_span = server.resolve_child_macro_prop_declaration(&resolved)?;
            location_from_span(&resolved.child.uri, &resolved.child.line_index, decl_span)
        })()
    } else {
        None
    };

    tracing::debug!(
        "references: verter found {}",
        verter_result.as_ref().map_or(0, |v| v.len())
    );

    // B4: workspace-wide references for GLOBAL css classes (declared in a
    // non-scoped block or under :global). The provider has no CSS knowledge —
    // this leg completes natively and returns. Scoped classes never enter
    // (fail closed: same-file only via the native path above).
    if let Some(class_name) = super::nav_features_css::global_class_target(server, uri, position) {
        let locations = super::nav_features_css::merge_global_class_references(
            server,
            uri,
            &class_name,
            verter_result.unwrap_or_default(),
        );
        return Ok((!locations.is_empty()).then_some(locations));
    }

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
                tracing::debug!("references: querying tp at tsx offset {}", tsx_offset);
                // Pin the FOREIGN carrier IDE surfaces BEFORE the query (see
                // handle_goto_definition).
                let foreign_ide_set = server.capture_foreign_carrier_ide_set();
                match tp.get_references(&ctx.tsx_path, tsx_offset).await {
                    Ok(type_refs) => {
                        // Post-await validation: a response produced against a
                        // surface that no longer matches must be DROPPED (fail
                        // closed), never mapped through a superseded context.
                        if !server.provider_context_still_valid(uri, &ctx) {
                            tracing::debug!(
                                "references: dropping provider locations — captured surface \
                                 no longer valid"
                            );
                            return Ok(inject_child_prop_declaration(
                                verter_result,
                                child_prop_declaration,
                            ));
                        }
                        tracing::debug!(
                            "references: type provider returned {} locations",
                            type_refs.len()
                        );
                        let carrier_source_exists =
                            |p: &str| server.documents.host().get_source(p).is_some();
                        let negotiated_encoding = server.position_encoding.read().clone();
                        return Ok(inject_child_prop_declaration(
                            merge::merge_references(
                                verter_result,
                                type_refs,
                                &ctx.tsx_path,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.carrier_line_index,
                                Some(&|ide_path: &str| {
                                    server.foreign_ide_context(&foreign_ide_set, ide_path)
                                }),
                                &carrier_source_exists,
                                negotiated_encoding,
                                &|p: &str| {
                                    block_in_place_if_available(|| {
                                        server.documents.host().workspace_read().read_file(p)
                                    })
                                },
                            ),
                            child_prop_declaration,
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

    Ok(inject_child_prop_declaration(
        verter_result,
        child_prop_declaration,
    ))
}

/// Append the resolved child-prop declaration to a references result when it
/// is not already present (deduped by canonical path + exact range), honoring
/// the caller's `includeDeclaration` choice (the caller computes `declaration`
/// only when declarations were requested).
fn inject_child_prop_declaration(
    result: Option<Vec<Location>>,
    declaration: Option<Location>,
) -> Option<Vec<Location>> {
    let Some(declaration) = declaration else {
        return result;
    };
    let mut locations = result.unwrap_or_default();
    let decl_path = uri_to_canonical_id(&declaration.uri);
    let present = locations.iter().any(|loc| {
        verter_span::path::fs_paths_equal(&uri_to_canonical_id(&loc.uri), &decl_path)
            && loc.range == declaration.range
    });
    if !present {
        locations.push(declaration);
    }
    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

/// The fail-closed error a rename / prepare-rename returns for a carrier owned by
/// MULTIPLE configured projects. Non-silent (the editor surfaces the message), and
/// carries NO edit — so a partial cross-project rename can never ship for the
/// newly-resolved multi-claimant case.
fn multi_claimant_rename_unavailable_error() -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        // LSP `RequestFailed` (-32803): the request failed for a known, user-facing
        // reason (not a protocol/internal fault). tower-lsp has no named variant.
        code: tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        message: "verter: rename is unavailable for a carrier owned by multiple TypeScript \
                  projects — a cross-project rename could leave the symbol dangling in sibling \
                  projects. Give the carrier a single owning tsconfig (disambiguate its \
                  include/references) to enable rename."
            .into(),
        data: None,
    }
}

pub(super) async fn handle_prepare_rename(
    server: &VerterLanguageServer,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let _hg = HandlerGuard::new("prepare_rename");
    let uri = &params.text_document.uri;
    let position = &params.position;

    if server.editor_owns_carrier_rename() {
        return Ok(None);
    }

    // Virtual file: not supported (no Verter rename context for generated code)
    if server.documents.get_virtual_source_uri(uri).is_some() {
        return Ok(None);
    }

    // Multi-claimant carrier: fail rename closed (see `handle_rename`) so the editor
    // surfaces the reason BEFORE the user starts a rename that could partial-edit
    // across sibling projects.
    if server.carrier_is_multi_claimant(uri) {
        return Err(multi_claimant_rename_unavailable_error());
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

    if server.editor_owns_carrier_rename() {
        return Ok(None);
    }

    // A carrier owned by MULTIPLE configured projects now resolves to a single tsgo
    // default owner for per-file features (hover / definition / completion /
    // references all serve), but a PROVIDER rename runs only within that one owner
    // project. Renaming a symbol that ESCAPES the owner (exported + imported by a
    // sibling configured project) would silently leave it dangling in the
    // siblings — a partial cross-project rename. Cheaply proving escape is not
    // feasible without the cross-project rename fan-out (not yet implemented), so rename
    // FAILS CLOSED here with a clear message rather than shipping a partial edit;
    // every other IDE feature still serves from the resolved owner. A
    // uniquely-owned carrier renames normally. (Checked AFTER the editor-owned
    // yield so an editor-plugin route still defers to the editor's own rename.)
    if server.carrier_is_multi_claimant(uri) {
        return Err(multi_claimant_rename_unavailable_error());
    }

    // Readiness protocol (never a starter): the cross-file rename declaration
    // surfaces via the imported component's `{carrier}.ts` PUBLIC-API surface,
    // which background publication delivers and receipts as DependencyReady.
    // The handler may capture the receipt or JOIN an in-flight publication
    // (bounded by the rename deadline); on a miss it enqueues background
    // publication and proceeds WITHOUT the provider leg — the same fail-closed
    // shape as a provider error, with the child-prop completeness gate below
    // still refusing partial cross-file edits. Runs BEFORE the fence so a
    // joined publication's provider commands are written first.
    let dependency_readiness = server.dependency_readiness_join(uri).await;

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

    // Classify the cursor with respect to the cross-file `<Child prop=…>` rename
    // ONCE, up front — so the MERGED-EDIT COMPLETENESS GATE applies on EVERY return
    // path (the provider-query success branch, the verter-only fallthrough, AND the
    // provider-Err branch), never only on provider success. A SELF-FILE rune-module
    // projection never participates (its edits are deferred), so it stays
    // `NotChildProp` (ungated). Sync resolution covers the INLINE case; the imported
    // case is upgraded async below.
    let mut rename_class = if server.is_self_file_projection(uri) {
        ChildPropRenameClass::NotChildProp
    } else {
        server.classify_child_prop_rename(uri, position)
    };

    // Enhance with TypeProvider for cross-file renames.
    // Extract all context synchronously — no DashMap guard held across await.
    //
    // GATED OFF for a SELF-FILE rune-module own buffer: its workspace-EDIT
    // positions are not yet mapped through the self-file mapper, so a returned
    // edit could land off by the prelude offset (or inside the prelude) and
    // CORRUPT the module. Rename stays DEFERRED for the self-file projection —
    // a clean no-op, never a wrong/unmapped edit. (Carrier rename unchanged.)
    //
    // The merged/available result is captured into `result` and the gate is applied
    // ONCE at the end over `rename_class`, so a confirmed child-prop rename cannot
    // escape the gate on any branch.
    let mut result = verter_result.clone();
    if !server.is_self_file_projection(uri) && dependency_readiness.is_ready() {
        if let Some(tp) = &server.type_provider {
            if let Some(ctx) = server.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::carrier_position_to_tsx_offset_validated(
                    position,
                    &ctx.carrier_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    // FENCE: hold the provider fence across snapshot-capture →
                    // declaration resolution → rename query → response so the
                    // captured carrier-API generations are the generations the
                    // provider's offsets are interpreted against, with no concurrent
                    // rename transaction interleaving its own surface mutations
                    // mid-capture. The declaration `get_definition` (imported case)
                    // runs inside this same fence — no blocking guard is held across
                    // its await.
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
                    // And the FOREIGN carrier IDE set, pinned under the same
                    // fence, so a returned foreign `.vue.tsx` location maps
                    // through the generation this request began against.
                    let foreign_ide_set = server.capture_foreign_carrier_ide_set();

                    // IMPORTED-TYPE declaration UPGRADE: a `defineProps<ImportedType>()`
                    // child prop has no inline macro-field span (its declaration lives
                    // in a THIRD file), so the sync classification left it
                    // `Unknown`. Resolve the declaration target by a provider
                    // `get_definition` at the SAME validated parent TSX offset the
                    // rename uses — the provider resolves the prop usage to its member
                    // declaration in one hop. The resolved third-file location is the
                    // `Known` target the gate proves the provider's own rename edits;
                    // an unresolved target stays `Unknown` and the gate fails closed.
                    server
                        .upgrade_imported_child_prop_declaration(
                            &mut rename_class,
                            tp.as_ref(),
                            &ctx.tsx_path,
                            tsx_offset,
                        )
                        .await;

                    match tp.get_rename_locations(&ctx.tsx_path, tsx_offset).await {
                        // Post-await validation as a match guard: the provider's
                        // locations are consumed ONLY while the captured surface is
                        // still honored and the open document still matches it.
                        Ok(mut type_locs) if server.provider_context_still_valid(uri, &ctx) => {
                            // PROVIDER-AGNOSTIC inline child-declaration synthesis. A
                            // cross-file `<Child prop=…>` rename must edit BOTH the
                            // parent usage AND the prop declaration. For an INLINE
                            // `defineProps` declaration the provider's own
                            // `textDocument/rename` does not reliably enumerate the
                            // child-declaration leg across the synthesized
                            // `{carrier}.ts` API surface (tsgo does not), so Verter
                            // synthesizes that leg itself — from its OWN Vue resolution
                            // + the pinned snapshot's source map — giving the child
                            // edit a single deterministic Verter-owned origin for BOTH
                            // providers. (The imported-member declaration is the
                            // provider's own native edit; nothing is synthesized for
                            // it.) Only a `Known` declaration with an `inline_decl_span`
                            // synthesizes.
                            if let ChildPropRenameClass::Confirmed(target) = &rename_class {
                                if let ChildPropDeclarationProof::Known {
                                    uri: child_decl_uri,
                                    inline_decl_span: Some(inline_decl_span),
                                    ..
                                } = &target.declaration
                                {
                                    if let Some(snapshot) = query_snapshot
                                        .snapshot_for(&target.usage.child_carrier_api_path)
                                    {
                                        // The API surface spells the DECLARED prop
                                        // name verbatim; a kebab-case usage
                                        // (`:my-prop` → `myProp`) must key the
                                        // synthesis on the declared name sliced from
                                        // the child source, or the byte-equality
                                        // tripwire fails closed for every
                                        // case-mapped rename.
                                        let declared_name = server
                                            .declared_prop_name_at_inline_span(
                                                child_decl_uri,
                                                *inline_decl_span,
                                            )
                                            .unwrap_or_else(|| {
                                                target.usage.parent_prop_name.clone()
                                            });
                                        if let Some((start, end)) =
                                            crate::provider_surface_store::locate_prop_decl_range_in_carrier_api(
                                                snapshot,
                                                *inline_decl_span,
                                                &declared_name,
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
                            result = merge::merge_rename_locations(
                                verter_result,
                                type_locs,
                                new_name,
                                &ctx.tsx_path,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.carrier_line_index,
                                Some(&|ide_path: &str| {
                                    server.foreign_ide_context(&foreign_ide_set, ide_path)
                                }),
                                Some(&api_resolver),
                                &carrier_source_exists,
                                negotiated_encoding.clone(),
                                &|p: &str| {
                                    block_in_place_if_available(|| {
                                        server.documents.host().workspace_read().read_file(p)
                                    })
                                },
                            );
                            // INITIATING-USAGE LEG SYNTHESIS (provider-agnostic,
                            // same doctrine as the child-declaration leg): the
                            // provider's own usage edit maps back through the
                            // case-mapped tsx token (kebab `my-prop` → camel
                            // `myProp`) and lands a PREFIX of the authored
                            // kebab name — a corrupt edit the completeness gate
                            // rightly rejects. Verter owns the exact authored
                            // usage span, so it re-anchors the initiating usage
                            // edit itself. For exact-case names this rewrites
                            // the provider's correct edit with the identical
                            // range — a deterministic no-op.
                            if let ChildPropRenameClass::Confirmed(target) = &rename_class {
                                if let Some(usage_range) = target.expected_parent_usage_range {
                                    synthesize_parent_usage_rename_edit(
                                        &mut result,
                                        &target.usage.parent_uri,
                                        usage_range,
                                        new_name,
                                    );
                                }
                            }
                        }
                        // Post-await validation failed (STRICT for rename: a corrupt
                        // edit is worse than no edit): drop the WHOLE provider edit
                        // set. The verter-only result (already in `result`) still
                        // passes through the completeness gate below, so a confirmed
                        // child-prop rename fails closed rather than shipping a
                        // usage-only partial.
                        Ok(_) => {
                            tracing::warn!(
                                "rename: dropping provider rename locations — captured \
                                 surface no longer valid"
                            );
                        }
                        Err(e) => {
                            // The provider rename failed: fall back to the verter-only
                            // result (already in `result`). It is STILL run through the
                            // gate below — a confirmed child-prop rename whose merged
                            // edit lacks the declaration leg fails closed here too,
                            // never a usage-only partial on the Err path.
                            tracing::warn!("rename: type provider error: {e}");
                        }
                    }
                }
            }
        }
    }

    // MERGED-EDIT COMPLETENESS GATE (fail-closed) — applied ONCE over the final
    // result, so it covers the provider-success, provider-Err, and verter-only
    // fallthrough paths uniformly. For a CONFIRMED cross-file child-prop rename the
    // EMITTED `WorkspaceEdit` must edit BOTH the prop declaration AND the parent
    // `.vue` usage at their EXACT full ranges, or the whole rename fails closed
    // (returns no edit) — never a usage-only / decl-only partial. A `NotChildProp`
    // result is returned untouched. See `gate_cross_file_child_prop_rename`.
    Ok(
        gate_cross_file_child_prop_rename(result, &rename_class, new_name).map(|mut edit| {
            merge::dedupe_rename_workspace_edit_with_preferred(&mut edit, Some(uri));
            edit
        }),
    )
}

/// Upsert the EXACT authored parent-usage rename edit for a confirmed
/// `<Child prop=…>` rename. The provider's own usage leg maps back through the
/// case-mapped tsx token (kebab `my-prop` → camel `myProp`) and lands a PREFIX
/// of the authored name — a corrupt edit the completeness gate rightly
/// rejects. Verter owns the authored usage span, so the initiating-usage edit
/// is Verter-synthesized exactly like the child-declaration leg: prune any
/// merged edit in the parent that OVERLAPS the authored usage range (the
/// mis-ranged provider leg), then insert the exact `(range, new_name)` edit.
/// Other edits in the parent (additional usages, the shorthand parent binding)
/// are untouched — their ranges do not overlap the initiating usage.
fn synthesize_parent_usage_rename_edit(
    result: &mut Option<WorkspaceEdit>,
    parent_uri: &Uri,
    usage_range: Range,
    new_name: &str,
) {
    let edit = result.get_or_insert_with(|| WorkspaceEdit {
        changes: Some(Default::default()),
        ..Default::default()
    });
    let changes = edit.changes.get_or_insert_with(Default::default);
    // The merge keys its mapped edits by its OWN uri form (provider paths are
    // lowercased on construction); the initiating document uri may differ in
    // case. Match the existing entry by canonical path equality so the
    // mis-ranged provider leg is actually pruned (the completeness gate
    // matches the same way).
    let expected_path = uri_to_canonical_id(parent_uri);
    let key = changes
        .keys()
        .find(|k| verter_span::path::fs_paths_equal(&uri_to_canonical_id(k), &expected_path))
        .cloned()
        .unwrap_or_else(|| parent_uri.clone());
    let edits = changes.entry(key).or_default();
    let overlaps = |range: &Range| {
        (range.start.line, range.start.character)
            < (usage_range.end.line, usage_range.end.character)
            && (usage_range.start.line, usage_range.start.character)
                < (range.end.line, range.end.character)
    };
    edits.retain(|e| !overlaps(&e.range));
    edits.push(TextEdit {
        range: usage_range,
        new_text: new_name.to_string(),
    });
}

/// Inject Verter's SYNTHESIZED child-declaration carrier rename location into the
/// provider's rename-location set, deduplicating against the provider's OWN
/// carrier location(s) for the SAME prop declaration.
///
/// The synthesized location and a provider's real carrier location (tsserver
/// returns one; tsgo does not) target the SAME `{carrier}.ts` byte range for the
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
/// SOURCE edits a confirmed `<Child prop=…>` rename MUST produce — the MERGED-EDIT
/// COMPLETENESS GATE.
///
/// A confirmed child-prop rename is incomplete unless the EMITTED `WorkspaceEdit`
/// edits BOTH:
///   1. the prop DECLARATION at `expected_decl_range` in `expected_decl_uri` with
///      `new_text == new_name` — the child component's `.vue` macro field (inline
///      case) OR a `defineProps<ImportedType>()` member declaration in a THIRD file
///      (imported case); AND
///   2. the parent's `.vue` prop USAGE at `expected_parent_range` with
///      `new_text == new_name`.
///
/// PROVIDER-AGNOSTIC by construction: it inspects only the merged, mapped source
/// `WorkspaceEdit` (`changes: HashMap<Uri, Vec<TextEdit>>`) — it does NOT care
/// whether the declaration edit came from Verter's inline synthesis or from the
/// provider's own native leg (tsserver enumerates it; tsgo does not; the imported
/// member is the provider's native edit for both). So a result whose declaration
/// leg is present passes even when Verter's own synthesis could not locate it — no
/// `is_tsgo`/`is_tsserver` branch, no regression.
///
/// Each `expected_*` range is `None` when the originating span did not resolve to
/// a position; the gate then cannot prove that leg precisely and FAILS CLOSED
/// (returns `false`) — the fail-closed boundary for an unmappable edit.
///
/// Range match is FULL-RANGE EXACT (both `start` AND `end`): an edit at the right
/// anchor but a WRONG span (e.g. the provider ranged the whole `name: type` member,
/// or the wrong end) does NOT satisfy the leg — a start-only check is too weak. A
/// `new_text` mismatch (a stray edit at the same anchor with different text) does
/// NOT satisfy the leg either.
fn workspace_edit_satisfies_child_prop_rename(
    merged: &WorkspaceEdit,
    expected_decl_uri: &Uri,
    expected_decl_range: Option<Range>,
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
        let expected_path = crate::documents::uri_to_canonical_id(uri);
        // FULL-RANGE equality (start AND end) — a right-anchor wrong-span edit must
        // NOT pass (the start-only check this replaced was too weak).
        changes.iter().any(|(edited_uri, edits)| {
            let edited_path = crate::documents::uri_to_canonical_id(edited_uri);
            verter_span::path::fs_paths_equal(&edited_path, &expected_path)
                && edits
                    .iter()
                    .any(|e| e.range == expected && e.new_text == new_name)
        })
    };

    has_edit_at(expected_decl_uri, expected_decl_range)
        && has_edit_at(expected_parent_uri, expected_parent_range)
}

/// Apply the MERGED-EDIT COMPLETENESS GATE to a cross-file rename result.
///
/// - [`ChildPropRenameClass::Confirmed`]: the merged `WorkspaceEdit` MUST satisfy
///   [`workspace_edit_satisfies_child_prop_rename`] (edits BOTH the prop
///   declaration AND the parent `.vue` usage at their EXACT full ranges). If it does
///   not — including a [`ChildPropDeclarationProof::Unknown`] declaration (no
///   resolved target to prove) — the whole rename fails closed → `None`. This is
///   the fix for the usage-only-partial gap: a confirmed child-prop rename whose
///   merged edit lacks the declaration (e.g. tsgo, synthesis leg could not be
///   produced; or an unresolvable imported type) returns NO edit rather than a
///   usage-only partial. Provider-AGNOSTIC: a result whose declaration leg already
///   lands (a tsserver native leg, or a provider's imported-member edit) passes even
///   when Verter's own synthesis could not locate it (no `is_tsgo`/`is_tsserver`
///   branch).
/// - [`ChildPropRenameClass::NotChildProp`]: do NOT gate. The provider's own merged
///   result is returned untouched — not a confirmed cross-file child-prop rename, so
///   Verter must not suppress an otherwise-valid provider result.
///
/// Inspects ONLY the merged source `WorkspaceEdit`, so it is a pure function of
/// `(merged, class, new_name)` — unit-testable without a live provider.
fn gate_cross_file_child_prop_rename(
    merged: Option<WorkspaceEdit>,
    rename_class: &ChildPropRenameClass,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let ChildPropRenameClass::Confirmed(target) = rename_class else {
        return merged;
    };
    // The resolved declaration target's URI + range — `Unknown` yields no URI/range
    // (a `None` range fails the per-leg proof, so the whole gate fails closed).
    let (expected_decl_uri, expected_decl_range) = match &target.declaration {
        ChildPropDeclarationProof::Known { uri, range, .. } => (Some(uri), *range),
        ChildPropDeclarationProof::Unknown => (None, None),
    };
    let satisfied = merged
        .as_ref()
        .zip(expected_decl_uri)
        .is_some_and(|(edit, decl_uri)| {
            workspace_edit_satisfies_child_prop_rename(
                edit,
                decl_uri,
                expected_decl_range,
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
#[path = "nav_features_navigation_tests.rs"]
mod nav_features_navigation_tests;
