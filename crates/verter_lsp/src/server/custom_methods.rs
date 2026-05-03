//! Custom LSP protocol handlers (phase 11e.6).
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`]
//! covering the 13 custom $/* methods consumed by main.rs as
//! `.custom_method("$/...", VerterLanguageServer::<method>)` calls.
//!
//! All methods were moved verbatim from `server.rs` (now `server/mod.rs`).
//! No behaviour change. Inherent-method resolution across multiple
//! impl blocks in the same crate keeps `VerterLanguageServer::<method>`
//! reachable from main.rs unchanged (per Rust's method resolution
//! rules).

use std::sync::Arc;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::tsgo::merge;

use super::background_init::is_generated_verter_types_event;
use super::protocol_types::*;
use super::server_utils::*;
use super::VerterLanguageServer;

impl VerterLanguageServer {
    /// Handle `$/onDidChangeTsOrJsFile` notification.
    ///
    /// Called when the client edits a `.ts`, `.js`, or `.vue` file.
    /// Invalidates host caches and re-syncs to the TypeProvider.
    pub async fn on_did_change_ts_or_js_file(&self, params: OnDidChangeTsOrJsFileParams) {
        tracing::info!("onDidChangeTsOrJsFile ENTER {}", params.uri);

        // Skip .vue files — they are synced to the type provider via TSX compilation
        // in sync_ide_to_provider(). Sending raw Vue SFC source to TSGO (which
        // expects TypeScript) corrupts its internal state.
        if params.uri.ends_with(".vue") {
            return;
        }

        // For non-Vue files tracked by the extension (TS/JS), keep the host and
        // provider in sync. Exact `.vue` imports are rewritten to `.vue.ts`
        // before syncing so the provider resolves through Verter-managed files.
        if let Some(last) = params.changes.last() {
            // Convert file:// URI to filesystem path — update_file() calls
            // path_to_uri() internally, so passing a URI would double-wrap it
            // (e.g., file:///file:///...).
            let path = if let Ok(uri) = params.uri.parse::<Uri>() {
                uri_to_canonical_id(&uri)
            } else {
                params.uri.clone()
            };

            let module_references = self
                .documents
                .host
                .upsert(verter_session::UpsertRequest {
                    canonical_id: Some(path.clone()),
                    input_id: path.clone(),
                    source: Arc::from(last.text.as_str()),
                    file_kind: verter_session::FileKind::NonSfc,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default();

            if let Some(snapshot) = self.published_resolver() {
                self.sync_non_vue_file_to_provider(
                    &snapshot,
                    &path,
                    Arc::from(last.text.as_str()),
                    &module_references,
                )
                .await;
            } else {
                self.queue_snapshot_provider_sync(path);
            }
        }
    }

    /// Handle `$/onFileChanged` notification.
    ///
    /// Called when `node_modules` files are created, updated, or deleted.
    pub async fn on_file_changed(&self, params: OnFileChangedParams) {
        tracing::debug!("$/onFileChanged: {} ({})", params.uri, params.change_type);

        let canonical_id = if let Ok(uri) = params.uri.parse::<Uri>() {
            uri_to_canonical_id(&uri)
        } else {
            crate::documents::uri_to_canonical_id_from_str(&params.uri)
        };

        // Skip watcher events for Verter-generated @verter/types stubs.
        // Real installed @verter/types packages (no marker) pass through normally.
        if is_generated_verter_types_event(&canonical_id) {
            return;
        }

        if let Some(ws) = self.vfs_workspace.read().as_ref() {
            let change = match params.change_type.as_str() {
                "create" | "update" => verter_workspace::WorkspaceChange::FileChanged {
                    canonical_id: canonical_id.clone(),
                    source: None,
                },
                "delete" => verter_workspace::WorkspaceChange::FileDeleted {
                    canonical_id: canonical_id.clone(),
                },
                other => {
                    tracing::warn!("on_file_changed: unexpected change_type {:?}", other);
                    return;
                }
            };
            ws.apply_changes(vec![change]);
        }

        // Handle .vue file changes from the file watcher.
        // These are files not open in the editor — re-sync to type provider.
        if params.uri.ends_with(".vue") {
            match params.change_type.as_str() {
                "create" | "update" => {
                    self.resync_background_vue_file(&canonical_id).await;
                }
                "delete" => {
                    // Close TSX/DTS in the type provider and clean up.
                    if let Some(state) =
                        self.remove_provider_sync_state(&canonical_id).or_else(|| {
                            let profile = self.documents.tsx_profile.read().clone();
                            self.documents
                                .host
                                .get_ide(&canonical_id, &profile)
                                .and_then(|ide| {
                                    self.prepare_vue_provider_sync_transition(
                                        &canonical_id,
                                        ide.is_jsx,
                                    )
                                    .map(|transition| transition.next)
                                })
                        })
                    {
                        self.close_provider_state(&state).await;
                    }
                    self.documents.host.remove(&canonical_id);
                }
                _ => {}
            }
        }

        // Check if the changed file is a known vite config or its dependency.
        // If so, trigger a full registry rebuild to re-analyze aliases.
        let is_vite_dep = {
            let ws = self.vfs_workspace.read();
            ws.as_ref()
                .and_then(|ws| ws.load_published())
                .and_then(|published| {
                    published
                        .ext::<crate::workspace_state::LspViews>()
                        .map(|views| {
                            views
                                .project_views
                                .iter()
                                .any(|v| v.vite_config_deps.iter().any(|dep| dep == &canonical_id))
                        })
                })
                .unwrap_or(false)
        };

        if is_vite_dep {
            tracing::debug!(
                "vite config dependency changed: {} — triggering registry rebuild",
                canonical_id
            );
            self.trigger_registry_rebuild().await;
        }
    }

    pub async fn on_watcher_state_changed(&self, params: WatcherStateChangedParams) {
        tracing::warn!(
            "$/verter/watcherStateChanged: workspace_root={} reason={}",
            params.workspace_root,
            params.reason
        );

        let workspace_root = if let Ok(uri) = params.workspace_root.parse::<Uri>() {
            uri_to_canonical_id(&uri)
        } else {
            crate::documents::uri_to_canonical_id_from_str(&params.workspace_root)
        };

        if let Some(ws) = self.vfs_workspace.read().as_ref() {
            ws.apply_changes(vec![
                verter_workspace::WorkspaceChange::DirectoryTreeDirty {
                    prefix: workspace_root,
                },
            ]);
        }
    }

    /// Handle `$/getCompiledCode` request.
    ///
    /// Returns the compiled TSX output for a Vue file URI.
    pub async fn get_compiled_code(
        &self,
        params: GetCompiledCodeParams,
    ) -> Result<Option<CompiledCodeResponse>> {
        let uri = params.uri;
        tracing::debug!("$/getCompiledCode: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        if let Some(canonical_id) = self.documents.get_canonical_id(&parsed_uri) {
            self.documents.host().ensure_loaded(&canonical_id);
        }
        let tsx = self.documents.get_ide(&parsed_uri);

        Ok(tsx.map(|tsx| CompiledCodeResponse {
            js: CompiledBlock {
                code: tsx.code.to_string(),
                map: tsx.source_map.map(|m| m.to_string()),
            },
            css: CompiledBlock {
                code: String::new(),
                map: None,
            },
            wasm: CompiledBlock {
                code: String::new(),
                map: None,
            },
        }))
    }

    /// Handle `$/verter/documentDropEdit` request.
    ///
    /// When a `.vue` file is dropped into a template, inserts a component tag
    /// and an import statement.
    pub async fn document_drop_edit(
        &self,
        params: DocumentDropEditParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document.uri;
        tracing::debug!(
            "$/verter/documentDropEdit: {} -> {}",
            params.dropped_uri,
            uri.as_str()
        );

        let doc = match self.documents.get(uri) {
            Some(d) => d,
            None => return Ok(None),
        };

        let blocks = scan_sfc_blocks(&doc.source);
        // Compute preferred import path (alias-based if available)
        let canonical_target = crate::documents::uri_to_canonical_id(uri);
        let canonical_dropped = crate::documents::uri_to_canonical_id_from_str(&params.dropped_uri);
        let preferred_import_path = self
            .documents
            .host()
            .preferred_specifier(&canonical_target, &canonical_dropped);

        let edit = crate::features::document_drop_edit::document_drop_edit(
            &params.dropped_uri,
            &params.position,
            &doc.source,
            &blocks,
            &doc.line_index,
            uri,
            preferred_import_path.as_deref(),
        );

        Ok(edit)
    }

    /// Handle `$/verter/getVirtualFiles` request.
    ///
    /// Returns all virtual files for a Vue document URI.
    pub async fn get_virtual_files(
        &self,
        params: GetVirtualFilesParams,
    ) -> Result<Option<VirtualFilesResponse>> {
        let uri = params.uri;
        tracing::info!("getVirtualFiles ENTER {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        if let Some(canonical_id) = self.documents.get_canonical_id(&parsed_uri) {
            self.documents.host().ensure_loaded(&canonical_id);
        }
        let response = self.documents.get_virtual_files(&parsed_uri);
        tracing::info!("getVirtualFiles EXIT {uri}");
        Ok(response)
    }

    /// Handle `$/verter/applyStyleOverrides` request.
    ///
    /// Applies preprocessor-compiled CSS overrides to style blocks, updating the host's
    /// analysis cache. Used by the VS Code extension after transpiling Sass/Stylus.
    pub async fn apply_style_overrides(
        &self,
        params: ApplyStyleOverridesParams,
    ) -> Result<ApplyStyleOverridesResponse> {
        let uri = &params.uri;
        tracing::debug!("$/verter/applyStyleOverrides: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(ApplyStyleOverridesResponse { success: false }),
        };

        let canonical_id = uri_to_canonical_id(&parsed_uri);
        let overrides = params
            .overrides
            .into_iter()
            .map(|o| verter_session::StyleOverrideEntry {
                index: o.index as usize,
                code: Arc::from(o.code),
                source_map: o.source_map.map(Arc::from),
            })
            .collect();

        let result = self
            .documents
            .apply_style_overrides(&canonical_id, overrides);

        if result {
            // Re-publish diagnostics since analysis has changed
            self.publish_full_diagnostics(&parsed_uri).await;
        }

        Ok(ApplyStyleOverridesResponse { success: result })
    }

    /// Handle `$/verter/getAnalysis` request.
    ///
    /// Returns the full analysis snapshot as JSON for a Vue document URI.
    pub async fn get_analysis(
        &self,
        params: GetAnalysisParams,
    ) -> Result<Option<serde_json::Value>> {
        let uri = params.uri;
        tracing::debug!("$/verter/getAnalysis: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        Ok(self.documents.get_analysis_json(&parsed_uri))
    }

    /// Handle `$/verter/getStatistics` request.
    ///
    /// Returns basic statistics about the LSP session.
    pub async fn get_statistics(
        &self,
        _params: Option<StatisticsRequestParams>,
    ) -> Result<StatisticsSnapshot> {
        tracing::debug!("$/verter/getStatistics");

        let mut by_type = serde_json::Map::new();
        let mut by_file = serde_json::Map::new();

        // Collect LSP handler statistics
        for (event_type, summary) in self.statistics.summary_by_type() {
            by_type.insert(
                event_type,
                serde_json::json!({
                    "count": summary.count,
                    "totalMs": summary.total_ms,
                    "minMs": summary.min_ms,
                    "maxMs": summary.max_ms,
                    "averageMs": summary.average_ms(),
                }),
            );
        }
        for (file, summary) in self.statistics.summary_by_file() {
            by_file.insert(
                file,
                serde_json::json!({
                    "count": summary.count,
                    "totalMs": summary.total_ms,
                    "minMs": summary.min_ms,
                    "maxMs": summary.max_ms,
                    "averageMs": summary.average_ms(),
                }),
            );
        }

        // Merge host metrics (compile/upsert counters)
        let host_metrics = self.documents.host.metrics_snapshot();
        by_type.insert(
            "host:upsert".into(),
            serde_json::json!({
                "count": host_metrics.upserts,
                "totalMs": host_metrics.slice_hash_time_us_total as f64 / 1000.0,
                "minMs": 0,
                "maxMs": 0,
                "averageMs": host_metrics.avg_slice_hash_time_us / 1000.0,
            }),
        );
        by_type.insert(
            "host:compile".into(),
            serde_json::json!({
                "count": host_metrics.compile_requests,
                "totalMs": host_metrics.compile_time_us_total as f64 / 1000.0,
                "minMs": 0,
                "maxMs": 0,
                "averageMs": if host_metrics.compile_requests > 0 {
                    (host_metrics.compile_time_us_total as f64 / host_metrics.compile_requests as f64) / 1000.0
                } else {
                    0.0
                },
            }),
        );
        by_type.insert(
            "host:cache_hits".into(),
            serde_json::json!({
                "count": host_metrics.compile_cache_hits,
                "totalMs": 0,
                "minMs": 0,
                "maxMs": 0,
                "averageMs": 0,
            }),
        );

        Ok(StatisticsSnapshot {
            enabled: self.statistics.is_enabled(),
            session: StatisticsSession { by_type, by_file },
        })
    }

    /// Handle `$/verter/getProjectOverview` request.
    ///
    /// Returns a global project overview: all known files, component usage graph,
    /// and aggregate statistics.
    pub async fn get_project_overview(
        &self,
        _params: serde_json::Value,
    ) -> Result<ProjectOverviewResponse> {
        tracing::debug!("$/verter/getProjectOverview");

        let file_list = self.documents.host.list_files();

        let mut files = Vec::new();
        let mut component_graph = Vec::new();
        let mut total_vue_files = 0usize;
        let mut total_components = 0usize;
        let mut files_with_scoped_styles = 0usize;

        for (canonical_id, file_kind) in &file_list {
            let kind = match file_kind {
                verter_session::FileKind::VueSfc => "vue",
                verter_session::FileKind::NonSfc => {
                    if canonical_id.ends_with(".ts") || canonical_id.ends_with(".tsx") {
                        "ts"
                    } else {
                        "js"
                    }
                }
            };

            files.push(ProjectOverviewFile {
                path: canonical_id.clone(),
                kind,
            });

            if *file_kind == verter_session::FileKind::VueSfc {
                total_vue_files += 1;

                // Get analysis for component graph
                if let Some(analysis) = self.documents.host.get_analysis(canonical_id) {
                    // Component usage
                    if let Some(template) = &analysis.template {
                        let used: Vec<String> =
                            template.components.iter().map(|c| c.name.clone()).collect();
                        total_components += used.len();
                        if !used.is_empty() {
                            component_graph.push(ProjectOverviewComponentEdge {
                                file: canonical_id.clone(),
                                uses_components: used,
                            });
                        }
                    }

                    // Scoped styles check
                    if analysis.styles.iter().any(|s| s.scoped) {
                        files_with_scoped_styles += 1;
                    }
                }
            }
        }

        Ok(ProjectOverviewResponse {
            files,
            component_graph,
            stats: ProjectOverviewStats {
                total_vue_files,
                total_components,
                total_provide_keys: 0,
                total_inject_keys: 0,
                files_with_scoped_styles,
            },
        })
    }

    /// Handle `$/verter/getRouteTree` request.
    ///
    /// Returns a complete route analysis snapshot for the first workspace root.
    pub async fn get_route_tree(&self, _params: serde_json::Value) -> Result<serde_json::Value> {
        tracing::debug!("$/verter/getRouteTree");

        let roots = self.workspace_roots.lock().await.clone();
        let Some(root) = roots.first() else {
            return Ok(serde_json::to_value(
                verter_semantic::analysis::routes::RouteAnalysisSnapshot::default(),
            )
            .unwrap_or_default());
        };

        // Collect template components from all Vue SFC analyses
        let file_list = self.documents.host.list_files();
        let mut template_components = Vec::new();
        for (canonical_id, file_kind) in &file_list {
            if *file_kind == verter_session::FileKind::VueSfc {
                if let Some(analysis) = self.documents.host.get_analysis(canonical_id) {
                    if let Some(template) = &analysis.template {
                        template_components
                            .push((canonical_id.clone(), template.components.clone()));
                    }
                }
            }
        }

        let workspace = self.documents.host.workspace_read();
        let snapshot = verter_semantic::analysis::routes::build_route_analysis(
            &*workspace,
            root,
            &template_components,
        );

        Ok(serde_json::to_value(snapshot).unwrap_or_default())
    }

    /// Handle `$/verter/getBindingTypes` request.
    ///
    /// For each binding in the file's analysis, queries TSGO for its TypeScript type.
    /// Returns a map of binding name → type string (or null if unavailable).
    pub async fn get_binding_types(&self, params: GetAnalysisParams) -> Result<serde_json::Value> {
        let uri = params.uri;
        tracing::debug!("$/verter/getBindingTypes: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(serde_json::Value::Object(serde_json::Map::new())),
        };

        let mut result = serde_json::Map::new();

        // Get analysis for the file's bindings
        let analysis = self.documents.get_analysis(&parsed_uri);
        let Some(analysis) = analysis else {
            return Ok(serde_json::Value::Object(result));
        };

        // Need type provider and TSX context for type queries
        let Some(tp) = &self.type_provider else {
            return Ok(serde_json::Value::Object(result));
        };
        let Some((tsx_path, tsx_content, mapper)) = self.ide_context(&parsed_uri) else {
            return Ok(serde_json::Value::Object(result));
        };

        let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
        let Some(doc) = self.documents.get(&parsed_uri) else {
            return Ok(serde_json::Value::Object(result));
        };

        for binding in &analysis.bindings {
            // Convert Vue byte offset → Vue Position → TSX offset
            let vue_pos = doc.line_index.offset_to_position(binding.span.start);
            let Some(vue_pos) = vue_pos else { continue };

            let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                &vue_pos,
                &doc.line_index,
                &mapper,
                &tsx_li,
            );
            let Some(tsx_offset) = tsx_offset else {
                continue;
            };

            // Query TSGO for the type at this position
            if let Ok(Some(hover)) = tp.get_hover(&tsx_path, tsx_offset).await {
                // Extract the type from the hover contents
                // Typical format: "```typescript\nconst x: number\n```" or "(property) x: string"
                let type_str = extract_type_from_hover(&hover.contents, &binding.name);
                result.insert(
                    binding.name.clone(),
                    type_str
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                );
            } else {
                result.insert(binding.name.clone(), serde_json::Value::Null);
            }
        }

        Ok(serde_json::Value::Object(result))
    }

    /// Handle `$/verter/getComponentParents` request.
    ///
    /// Returns all files that use the component defined in the given URI,
    /// along with the props and slots they pass to it.
    pub async fn get_component_parents(
        &self,
        params: GetComponentParentsParams,
    ) -> Result<ComponentParentsResponse> {
        let uri = params.uri;
        tracing::debug!("$/verter/getComponentParents: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => {
                return Ok(ComponentParentsResponse {
                    component_path: uri,
                    parents: Vec::new(),
                });
            }
        };

        let target_canonical = self
            .documents
            .get_canonical_id(&parsed_uri)
            .unwrap_or_else(|| uri_to_canonical_id(&parsed_uri));

        // Normalize the target path for comparison
        let target_normalized = target_canonical.replace('\\', "/");

        let file_list = self.documents.host.list_files();
        let mut parents = Vec::new();
        let vue_count = file_list
            .iter()
            .filter(|(_, k)| *k == verter_session::FileKind::VueSfc)
            .count();
        tracing::info!(
            "getComponentParents: target='{}' scanning {} vue files",
            target_normalized,
            vue_count
        );

        for (canonical_id, file_kind) in &file_list {
            if *file_kind != verter_session::FileKind::VueSfc {
                continue;
            }
            // Skip the target file itself
            let normalized_id = canonical_id.replace('\\', "/");
            if normalized_id == target_normalized {
                continue;
            }

            if let Some(analysis) = self.documents.host.get_analysis(canonical_id) {
                if let Some(template) = &analysis.template {
                    for comp in &template.components {
                        if let Some(src) = &comp.import_source {
                            // Resolve the import source to an absolute path via VFS
                            let resolved = self
                                .documents
                                .host()
                                .resolve_import_via_workspace(&normalized_id, src)
                                .unwrap_or_else(|| {
                                    if src.starts_with('.') {
                                        let importer_dir = normalized_id
                                            .rfind('/')
                                            .map(|i| &normalized_id[..i])
                                            .unwrap_or("");
                                        resolve_import_path(importer_dir, src)
                                    } else {
                                        src.to_string()
                                    }
                                });
                            tracing::info!(
                                "  [{}] component '{}' import='{}' → resolved='{}'",
                                normalized_id.rsplit('/').next().unwrap_or("?"),
                                comp.name,
                                src,
                                resolved
                            );
                            let resolved_normalized = resolved.replace('\\', "/");
                            let matches = import_resolved_matches_target(
                                &resolved_normalized,
                                &target_normalized,
                            );
                            if matches {
                                tracing::info!(
                                    "  MATCH! resolved='{}' == target='{}'",
                                    resolved_normalized,
                                    target_normalized
                                );
                                let props_json = comp
                                    .props
                                    .iter()
                                    .filter_map(|p| serde_json::to_value(p).ok())
                                    .collect();
                                parents.push(ComponentParentInfo {
                                    file_path: canonical_id.clone(),
                                    component_name: comp.name.clone(),
                                    props: props_json,
                                    slots_used: comp.slots_used.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(ComponentParentsResponse {
            component_path: target_canonical,
            parents,
        })
    }
}
