//! Custom LSP protocol handlers.
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

mod component_meta;

use std::sync::Arc;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::uri_to_canonical_id;
use crate::type_provider::merge;

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

        // For non-carrier files tracked by the extension (TS/JS), keep the host and
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

            // Skip framework carriers — `.vue` files sync to the type
            // provider via TSX compilation in sync_ide_to_provider()
            // (sending raw SFC source to TSGO, which expects TypeScript,
            // corrupts its internal state), and a carrier-less row
            // (`.svelte`) produces no provider sync state at all.
            let Some(file_language) =
                crate::provider_sync::provider_script_language(&self.documents.host, &path)
            else {
                return;
            };

            let module_references = self
                .documents
                .host
                .upsert(verter_session::UpsertRequest {
                    canonical_id: Some(path.clone()),
                    input_id: path.clone(),
                    source: Arc::from(last.text.as_str()),
                    file_language,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default();

            if let Some(snapshot) = self.published_resolver() {
                self.sync_non_carrier_file_to_provider(
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

        // Handle framework CARRIER (`.vue`, `.svelte`, …) changes from the
        // file watcher. These are files not open in the editor — re-sync to
        // the type provider.
        if crate::server::carrier_language_for(&params.uri).is_some() {
            match params.change_type.as_str() {
                "create" | "update" => {
                    self.resync_background_carrier_file(&canonical_id).await;
                }
                "delete" => {
                    // Close TSX/DTS in the type provider and clean up.
                    if let Some(state) =
                        self.remove_provider_sync_state(&canonical_id).or_else(|| {
                            let profile = self.documents.tsx_profile.read().clone();
                            self.documents
                                .host
                                .get_ide(&canonical_id, &profile)
                                .and_then(|ide| self.carrier_close_state(&canonical_id, ide.is_jsx))
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
        let response = match self.documents.get_virtual_files(&parsed_uri) {
            Ok(response) => response,
            Err(error) => {
                return Err(crate::public_api_projection_jsonrpc_error(
                    "getVirtualFiles",
                    &uri_to_canonical_id(&parsed_uri),
                    error,
                ));
            }
        };
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
            self.publish_full_diagnostics_with_audit(&parsed_uri).await;
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
        let mut total_component_files = 0usize;
        let mut total_components = 0usize;
        let mut files_with_scoped_styles = 0usize;

        for (canonical_id, file_language) in &file_list {
            let kind = if file_language.is_framework_carrier() {
                "component"
            } else if canonical_id.ends_with(".ts") || canonical_id.ends_with(".tsx") {
                "ts"
            } else {
                "js"
            };

            files.push(ProjectOverviewFile {
                path: canonical_id.clone(),
                kind,
            });

            if file_language.is_framework_carrier() {
                total_component_files += 1;

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
                total_component_files,
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

        // Collect template components from all framework CARRIER analyses
        let file_list = self.documents.host.list_files();
        let mut template_components = Vec::new();
        for (canonical_id, file_language) in &file_list {
            if file_language.is_framework_carrier() {
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
        // The context is built from ONE captured immutable provider surface.
        let Some(ctx) = self.type_provider_context(&parsed_uri) else {
            return Ok(serde_json::Value::Object(result));
        };

        for binding in &analysis.bindings {
            // Convert Vue byte offset → Vue Position → TSX offset
            let carrier_pos = ctx
                .carrier_line_index
                .offset_to_position(binding.span.start);
            let Some(carrier_pos) = carrier_pos else {
                continue;
            };

            let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
                &carrier_pos,
                &ctx.carrier_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            );
            let Some(tsx_offset) = tsx_offset else {
                continue;
            };

            // Query TSGO for the type at this position
            if let Ok(Some(hover)) = tp.get_hover(&ctx.tsx_path, tsx_offset).await {
                // Post-await validation: a hover produced against a surface that
                // no longer matches must be DROPPED (fail closed) — the binding
                // reports `null` rather than a type read off a superseded surface.
                if !self.provider_context_still_valid(&parsed_uri, &ctx) {
                    tracing::debug!(
                        "getBindingTypes: dropping provider hover — captured surface \
                         no longer valid"
                    );
                    result.insert(binding.name.clone(), serde_json::Value::Null);
                    continue;
                }
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

        // Canonicalize the target path for comparison through the shared owner
        // (NOT a bare `\`→`/`): the comparison below is string-equality, so both
        // sides must be in the same canonical form (drive-lowercased, extended
        // prefix stripped) or a Windows `C:/…` target would never equal an
        // owner-canonicalized `c:/…` import resolution.
        let target_normalized = verter_span::path::canonicalize_path(&target_canonical);

        let file_list = self.documents.host.list_files();
        let mut parents = Vec::new();
        let carrier_count = file_list
            .iter()
            .filter(|(_, k)| k.is_framework_carrier())
            .count();
        tracing::info!(
            "getComponentParents: target='{}' scanning {} carrier files",
            target_normalized,
            carrier_count
        );

        for (canonical_id, file_language) in &file_list {
            if !file_language.is_framework_carrier() {
                continue;
            }
            // Skip the target file itself (canonical-form comparison via owner)
            let normalized_id = verter_span::path::canonicalize_path(canonical_id);
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
                            let resolved_normalized =
                                verter_span::path::canonicalize_path(&resolved);
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

    /// Handle `$/verter/audit/getRecord` request.
    ///
    /// Read-only query: returns the audit record for `request_id` if it
    /// is currently published in the host's records store. The record
    /// is returned as a JSON value (matching the `RequestAuditRecord`
    /// schema in `audit.generated.ts`). Returns `Ok(None)` when the
    /// record was never inserted (capture disabled) or already drained
    /// by an earlier consumer (e.g. `host.take_audit_record`).
    ///
    /// This handler does NOT mutate audit state — it consults the
    /// records store via a non-draining iterator and clones the
    /// matching record. A subsequent `getRecord` for the same
    /// `request_id` will return the same payload.
    pub async fn get_audit_record(
        &self,
        params: GetAuditRecordParams,
    ) -> Result<Option<serde_json::Value>> {
        use verter_audit::batch::AuditRecordSource;
        tracing::debug!("$/verter/audit/getRecord: {}", params.request_id);

        let target_id = match params.request_id.parse::<u64>() {
            Ok(id) => id,
            Err(_) => return Ok(None),
        };

        let host = self.documents.host_arc();
        let store = host.host_audit_runtime().audit_records_store();
        let mut found: Option<verter_audit::RequestAuditRecord> = None;
        store.for_each_record(&mut |_inserted_at, record| {
            if record.request_id == target_id {
                found = Some(record.clone());
            }
        });

        Ok(found.and_then(|record| serde_json::to_value(&record).ok()))
    }

    /// Handle `$/verter/audit/getRecent` request.
    ///
    /// Read-only query: returns recent records from the host's
    /// `AuditRecordsStore` as a JSON array. Optional `kind` filters by
    /// `RequestKind` variant tag (matched via
    /// [`verter_audit::RequestKind::matches_filter`]); optional `limit`
    /// caps the result size (default 50, hard capped at 1024).
    ///
    /// Records are sorted by request id descending so the most recent
    /// records appear first regardless of the underlying store's
    /// iteration order. The handler does NOT drain the store.
    pub async fn get_audit_recent(
        &self,
        params: Option<GetAuditRecentParams>,
    ) -> Result<Vec<serde_json::Value>> {
        use verter_audit::batch::AuditRecordSource;
        let params = params.unwrap_or_default();
        tracing::debug!(
            "$/verter/audit/getRecent: kind={:?} limit={:?}",
            params.kind,
            params.limit
        );

        const DEFAULT_LIMIT: usize = 50;
        const MAX_LIMIT: usize = 1024;
        let limit = params
            .limit
            .map(|n| (n as usize).min(MAX_LIMIT))
            .unwrap_or(DEFAULT_LIMIT);
        let kind_filter = params.kind;

        let host = self.documents.host_arc();
        let store = host.host_audit_runtime().audit_records_store();
        let mut collected: Vec<verter_audit::RequestAuditRecord> = Vec::new();
        store.for_each_record(&mut |_inserted_at, record| {
            if let Some(ref filter) = kind_filter {
                if !record.kind.matches_filter(filter) {
                    return;
                }
            }
            collected.push(record.clone());
        });

        // Sort descending by request_id so the newest records lead the
        // response — the underlying store does not promise iteration
        // order, so we impose one here for client predictability.
        collected.sort_by_key(|record| std::cmp::Reverse(record.request_id));
        collected.truncate(limit);

        Ok(collected
            .into_iter()
            .filter_map(|r| serde_json::to_value(&r).ok())
            .collect())
    }
}
