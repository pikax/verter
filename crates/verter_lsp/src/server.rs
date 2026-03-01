use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::capabilities::server_capabilities;
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::{uri_to_canonical_id, DocumentRegistry};
use crate::features::action_utils::fix_placeholder_uris;
use crate::features::call_hierarchy;
use crate::features::code_lens::code_lenses;
use crate::features::color_info;
use crate::features::completion::completions_at_position;
use crate::features::definition::definition_at_position;
use crate::features::diagnostics::map_diagnostics;
use crate::features::document_highlight::highlights_at_position;
use crate::features::document_link::build_document_links;
use crate::features::document_symbol::build_document_symbols;
use crate::features::folding_range::build_folding_ranges;
use crate::features::formatting::format_document;
use crate::features::hover::hover_at_position;
use crate::features::linked_editing::linked_editing_ranges;
use crate::features::organize_imports::organize_imports_actions;
use crate::features::references::references_at_position;
use crate::features::rename::{prepare_rename, rename_at_position};
use crate::features::workspace_symbol::workspace_symbols;
use crate::statistics::Statistics;
use crate::tsgo::merge;
use crate::tsgo::project_sync::ProjectSync;
use crate::tsgo::traits::TypeProvider;
use crate::LspConfig;

// ── Custom protocol types ──────────────────────────────────────────────

/// Params for `$/onDidChangeTsOrJsFile` notification.
#[derive(Debug, Deserialize)]
pub struct OnDidChangeTsOrJsFileParams {
    pub uri: String,
    pub changes: Vec<TextChangeEvent>,
}

#[derive(Debug, Deserialize)]
pub struct TextChangeEvent {
    pub text: String,
    pub range: TextChangeRange,
}

#[derive(Debug, Deserialize)]
pub struct TextChangeRange {
    pub start: TextChangePosition,
    pub end: TextChangePosition,
}

#[derive(Debug, Deserialize)]
pub struct TextChangePosition {
    pub line: u32,
    pub character: u32,
}

/// Params for `$/onFileChanged` notification.
#[derive(Debug, Deserialize)]
pub struct OnFileChangedParams {
    pub uri: String,
    #[serde(rename = "type")]
    pub change_type: String,
}

/// Params for `$/getCompiledCode` request.
#[derive(Debug, Deserialize)]
pub struct GetCompiledCodeParams {
    pub uri: String,
}

/// Response for `$/getCompiledCode` request.
#[derive(Debug, Serialize)]
pub struct CompiledCodeResponse {
    pub js: CompiledBlock,
    pub css: CompiledBlock,
    pub wasm: CompiledBlock,
}

#[derive(Debug, Serialize)]
pub struct CompiledBlock {
    pub code: String,
    pub map: Option<String>,
}

/// Params for `$/verter/getVirtualFiles` request.
#[derive(Debug, Deserialize)]
pub struct GetVirtualFilesParams {
    pub uri: String,
}

/// Params for `$/verter/applyStyleOverrides` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStyleOverridesParams {
    pub uri: String,
    pub overrides: Vec<StyleOverrideParam>,
}

/// A single style override entry from the client.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleOverrideParam {
    pub index: u32,
    pub code: String,
    pub source_map: Option<String>,
}

/// Response for `$/verter/applyStyleOverrides` request.
#[derive(Debug, Serialize)]
pub struct ApplyStyleOverridesResponse {
    pub success: bool,
}

/// Params for `$/verter/getAnalysis` (and `$/verter/getBindingTypes`) request.
#[derive(Debug, Deserialize)]
pub struct GetAnalysisParams {
    pub uri: String,
}

/// Params for `$/verter/getComponentParents` request.
#[derive(Debug, Deserialize)]
pub struct GetComponentParentsParams {
    pub uri: String,
}

/// A single parent file that uses a component.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParentInfo {
    pub file_path: String,
    pub component_name: String,
    pub props: Vec<serde_json::Value>,
    pub slots_used: Vec<String>,
}

/// Response for `$/verter/getComponentParents` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParentsResponse {
    pub component_path: String,
    pub parents: Vec<ComponentParentInfo>,
}

/// A single virtual file entry in the response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFileEntry {
    pub kind: String,
    pub code: String,
    pub lang: String,
    pub source_map: Option<String>,
    pub stale: bool,
}

/// TSX block in the response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TsxBlock {
    pub code: String,
    pub source_map: Option<String>,
}

/// Response for `$/verter/getVirtualFiles` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFilesResponse {
    pub tsx: Option<TsxBlock>,
    pub virtual_files: Vec<VirtualFileEntry>,
}

/// Params for `$/verter/documentDropEdit` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDropEditParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub dropped_uri: String,
}

/// Params for `$/verter/getStatistics` request.
#[derive(Debug, Deserialize)]
pub struct StatisticsRequestParams {
    #[serde(default)]
    pub include_events: bool,
    pub scope: Option<String>,
}

/// Response for `$/verter/getStatistics` request.
#[derive(Debug, Serialize)]
pub struct StatisticsSnapshot {
    pub enabled: bool,
    pub session: StatisticsSession,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsSession {
    pub by_type: serde_json::Map<String, serde_json::Value>,
    pub by_file: serde_json::Map<String, serde_json::Value>,
}

/// Response for `$/verter/getProjectOverview` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewResponse {
    pub files: Vec<ProjectOverviewFile>,
    pub component_graph: Vec<ProjectOverviewComponentEdge>,
    pub stats: ProjectOverviewStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewFile {
    pub path: String,
    pub kind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewComponentEdge {
    pub file: String,
    pub uses_components: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewStats {
    pub total_vue_files: usize,
    pub total_components: usize,
    pub total_provide_keys: usize,
    pub total_inject_keys: usize,
    pub files_with_scoped_styles: usize,
}

/// The Verter language server implementation.
///
/// Wraps `verter_host` for SFC analysis and optionally a `TypeProvider`
/// (e.g., TSGO) for richer type information.
pub struct VerterLanguageServer {
    client: Client,
    documents: DocumentRegistry,
    type_provider: Option<Arc<dyn TypeProvider>>,
    project_sync: Option<ProjectSync>,
    workspace_root: tokio::sync::Mutex<Option<String>>,
    statistics: Arc<Statistics>,
    /// Negotiated position encoding (LSP 3.17). Set during `initialize()`.
    position_encoding: tokio::sync::Mutex<PositionEncodingKind>,
    /// Resolves aliased imports via tsconfig.json `compilerOptions.paths`.
    /// Initialized during `initialized()` when a workspace root is available.
    path_resolver: parking_lot::RwLock<Option<crate::config::TsConfigPathResolver>>,
    /// Diagnostics engine — runs all lint rules to produce a DiagnosticSet.
    linter: verter_diagnostics::Linter,
    /// Action engine — produces quick fixes and refactoring code actions.
    action_engine: verter_actions::ActionEngine,
    /// Cached verter diagnostics per document: URI → (version, diagnostics).
    /// Avoids re-running the linter when both push and pull paths request diagnostics
    /// for the same document version.
    cached_verter_diags: DashMap<String, (i32, Vec<Diagnostic>)>,
}

impl VerterLanguageServer {
    pub fn new(client: Client, config: LspConfig) -> Self {
        let project_sync = config
            .type_provider
            .as_ref()
            .map(|tp| ProjectSync::new(Arc::clone(tp), config.project_sync_mode));
        Self {
            client,
            documents: DocumentRegistry::new(config.host),
            type_provider: config.type_provider,
            project_sync,
            workspace_root: tokio::sync::Mutex::new(None),
            statistics: Arc::new(Statistics::new(500)),
            position_encoding: tokio::sync::Mutex::new(PositionEncodingKind::UTF16),
            path_resolver: parking_lot::RwLock::new(None),
            linter: verter_diagnostics::Linter::default(),
            action_engine: verter_actions::ActionEngine::default(),
            cached_verter_diags: DashMap::new(),
        }
    }

    /// Compute verter diagnostics (host errors + lint rules + component usage) for a document.
    /// Caches results per document version to avoid redundant re-computation when both
    /// push (didChange) and pull (textDocument/diagnostic) paths request diagnostics.
    fn compute_verter_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        // Check cache: if version matches, return cached diagnostics.
        let uri_str = uri.as_str();
        if let Some(doc) = self.documents.get(uri) {
            if let Some(cached) = self.cached_verter_diags.get(uri_str) {
                if cached.0 == doc.version {
                    return cached.1.clone();
                }
            }
        }

        let mut diags = if let Some(doc) = self.documents.get(uri) {
            let host_diags = self.documents.get_diagnostics(uri);
            match host_diags {
                Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
                None => vec![],
            }
        } else {
            vec![]
        };

        // Run the diagnostics engine (lint rules: CSS, template, a11y, etc.)
        if let Some(doc) = self.documents.get(uri) {
            if let Some(analysis) = self.documents.get_analysis(uri) {
                diags.extend(crate::features::diagnostics_bridge::run_linter(
                    &self.linter,
                    &analysis,
                    &doc.source,
                    &doc.line_index,
                ));

                // Component usage diagnostics (unknown props, unknown v-models).
                diags.extend(
                    crate::features::component_diagnostics::component_usage_diagnostics(
                        &analysis,
                        &doc.line_index,
                        &|import_source| self.resolve_component(uri, import_source),
                    ),
                );
            }
        }

        // Cache the result
        if let Some(doc) = self.documents.get(uri) {
            self.cached_verter_diags
                .insert(uri_str.to_string(), (doc.version, diags.clone()));
        }

        diags
    }

    /// Compute and push verter-only diagnostics for a document URI.
    async fn publish_diagnostics(&self, uri: &Uri) {
        let verter_diags = self.compute_verter_diagnostics(uri);
        self.publish_diagnostics_with(uri, verter_diags).await;
    }

    /// Publish verter-only diagnostics via the push (`publishDiagnostics`) path.
    ///
    /// TSGO type diagnostics are NOT included here — they are served exclusively
    /// through the pull diagnostics handler (`textDocument/diagnostic`). This
    /// avoids duplication: VS Code shows diagnostics from both push and pull, so
    /// including TSGO in both paths would double every TypeScript error.
    async fn publish_diagnostics_with(&self, uri: &Uri, verter_diags: Vec<Diagnostic>) {
        let _timer = self
            .statistics
            .timer("diagnostics", Some(uri.as_str().to_string()));

        tracing::debug!(
            "diagnostics (push): publishing {} verter diagnostics for {}",
            verter_diags.len(),
            uri.as_str()
        );

        self.client
            .publish_diagnostics(uri.clone(), verter_diags, None)
            .await;
    }

    /// Build a TextEdit for inserting an import statement into the script block.
    fn build_auto_import_edit(
        &self,
        doc_uri_str: &str,
        component_name: &str,
        import_path: &str,
    ) -> Option<TextEdit> {
        let uri: Uri = doc_uri_str.parse().ok()?;
        let doc = self.documents.get(&uri)?;
        let blocks = scan_sfc_blocks(&doc.source);

        // Find the script setup block
        let script_block = blocks
            .iter()
            .find(|b| b.tag_name == "script" && b.attrs_raw.contains("setup"))?;

        let (content_start, _content_end) = script_block.content_range();

        // Check if the component is already imported
        if let Some(analysis) = self.documents.get_analysis(&uri) {
            for import in &analysis.imports {
                if import.bindings.iter().any(|b| b.name == component_name) {
                    return None; // Already imported
                }
            }

            // Find the position after the last import statement
            let last_import_end = analysis.imports.iter().map(|imp| imp.span.end).max();

            let insert_offset = if let Some(end) = last_import_end {
                // Insert after the last import — the span_end is relative to script content
                let abs_offset = content_start + end;
                // Skip past the newline after the import
                let rest = &doc.source[abs_offset as usize..];
                let newline_skip = rest
                    .bytes()
                    .take_while(|&b| b == b'\n' || b == b'\r')
                    .count();
                abs_offset + newline_skip as u32
            } else {
                // No existing imports — insert at the beginning of the script block
                content_start
            };

            let import_stmt = format!("import {} from '{}'\n", component_name, import_path);
            let pos = doc.line_index.offset_to_position(insert_offset)?;

            Some(TextEdit {
                range: Range::new(pos, pos),
                new_text: import_stmt,
            })
        } else {
            None
        }
    }

    /// Scan the workspace for .vue files and upsert them into the host.
    /// Runs in the background so initialization isn't blocked.
    fn scan_workspace_vue_files(&self, root_uri: &str) {
        let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
        let root_path = std::path::PathBuf::from(&canonical);
        if !root_path.is_dir() {
            tracing::warn!(
                "scan_workspace: root path is not a directory: {:?}",
                root_path
            );
            return;
        }

        let mut count = 0u32;
        scan_vue_files_recursive(&root_path, &self.documents.host, &mut count);
        tracing::info!("scan_workspace: indexed {} .vue files", count);
    }

    async fn sync_tsx_to_provider(&self, uri: &Uri) {
        let _timer = self
            .statistics
            .timer("tsx_sync", Some(uri.as_str().to_string()));
        if let Some(sync) = &self.project_sync {
            if let Some(tsx) = self.documents.get_tsx(uri) {
                let tsx_path = self.tsx_path_for_uri(uri);
                tracing::info!("sync_tsx: {} ({} bytes)", tsx_path, tsx.code.len());
                if let Err(e) = sync.sync_tsx(&tsx_path, &tsx.code).await {
                    tracing::warn!("sync_tsx: failed for {tsx_path}: {e}");
                } else {
                    tracing::info!("sync_tsx: ok for {}", tsx_path);
                }
            } else {
                tracing::info!("sync_tsx: no TSX available for {}", uri.as_str());
            }
        }
    }

    /// Get TSX context for TypeProvider queries: (tsx_path, tsx_code, position_mapper).
    fn tsx_context(&self, uri: &Uri) -> Option<(String, Arc<str>, PositionMapper)> {
        let canonical_id = self.documents.get_canonical_id(uri);
        if canonical_id.is_none() {
            tracing::info!("tsx_context: no canonical_id for {}", uri.as_str());
            return None;
        }
        let tsx = self.documents.get_tsx(uri);
        if tsx.is_none() {
            tracing::info!(
                "tsx_context: no TSX for {} (canonical={})",
                uri.as_str(),
                canonical_id.as_deref().unwrap_or("?")
            );
            return None;
        }
        let tsx = tsx.unwrap();
        let mapper = self.documents.get_position_mapper(uri);
        if mapper.is_none() {
            tracing::info!("tsx_context: no position mapper for {}", uri.as_str());
            return None;
        }
        let tsx_path = self.tsx_path_for_uri(uri);
        Some((tsx_path, tsx.code, mapper.unwrap()))
    }

    /// Generate the TSX file path for a given Vue file URI.
    fn tsx_path_for_uri(&self, uri: &Uri) -> String {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        format!("{canonical}.tsx")
    }

    /// Resolve a child component's analysis from an import source path.
    ///
    /// Tries three strategies:
    /// 1. Relative imports → resolve against the parent's directory
    /// 2. Path alias resolution via tsconfig.json
    /// 3. Direct lookup (bare specifiers)
    fn resolve_component(
        &self,
        parent_uri: &Uri,
        import_source: &str,
    ) -> Option<verter_host::FileAnalysisSnapshot> {
        let canonical_id = uri_to_canonical_id(parent_uri);

        // Try 1: Relative import
        if import_source.starts_with('.') {
            let parts: Vec<&str> = canonical_id.split('/').collect();
            let dir = parts[..parts.len().saturating_sub(1)].join("/");
            let resolved = resolve_import_path(&dir, import_source);
            if let Some(a) = self.documents.host().get_analysis(&resolved) {
                return Some(a);
            }
        }

        // Try 2: Path alias resolution
        let pr_guard = self.path_resolver.read();
        if let Some(ref resolver) = *pr_guard {
            if let Some(resolved_path) = resolver.resolve(import_source) {
                if let Some(a) = self.documents.host().get_analysis(&resolved_path) {
                    return Some(a);
                }
            }
        }
        drop(pr_guard);

        // Try 3: Direct lookup
        self.documents.host().get_analysis(import_source)
    }

    /// Resolve a child component with full context for cross-file editing.
    fn resolve_component_context(
        &self,
        parent_uri: &Uri,
        import_source: &str,
    ) -> Option<crate::features::cross_file::ChildComponentContext> {
        let analysis = self.resolve_component(parent_uri, import_source)?;
        let canonical_id = uri_to_canonical_id(parent_uri);

        // Resolve the child's canonical ID
        let child_canonical_id = if import_source.starts_with('.') {
            let parts: Vec<&str> = canonical_id.split('/').collect();
            let dir = parts[..parts.len().saturating_sub(1)].join("/");
            resolve_import_path(&dir, import_source)
        } else {
            let pr_guard = self.path_resolver.read();
            if let Some(ref resolver) = *pr_guard {
                resolver
                    .resolve(import_source)
                    .unwrap_or_else(|| import_source.to_string())
            } else {
                import_source.to_string()
            }
        };

        // Get the child's source
        let child_source_arc = self.documents.host().get_source(&child_canonical_id)?;
        let child_source = child_source_arc.to_string();
        let child_uri: Uri = format!("file:///{}", child_canonical_id).parse().ok()?;
        let blocks = scan_sfc_blocks(&child_source);
        let line_index = LineIndex::new(&child_source, self.documents.encoding());

        Some(crate::features::cross_file::ChildComponentContext {
            uri: child_uri,
            source: child_source,
            analysis,
            blocks,
            line_index,
        })
    }

    /// Check if a URI is a virtual file and return its TSGO routing context.
    ///
    /// For virtual files (verter-virtual://), the content IS the TSX already.
    /// The cursor position is in TSX coordinates, so we can query TSGO directly
    /// without position mapping.
    ///
    /// Returns `Some((tsx_path, virtual_doc_line_index))` if this is a virtual file
    /// that should be routed through the source .vue file's TSX.
    fn virtual_file_context(&self, uri: &Uri) -> Option<(String, LineIndex)> {
        let source_uri_str = self.documents.get_virtual_source_uri(uri)?;
        let source_uri: Uri = source_uri_str.parse().ok()?;

        // Get the TSX path from the source .vue file
        let tsx_path = self.tsx_path_for_uri(&source_uri);

        // Build LineIndex from the virtual file's content (for offset conversion)
        let doc = self.documents.get(uri)?;
        let line_index = doc.line_index.clone();

        Some((tsx_path, line_index))
    }

    // ── Custom protocol handlers ──────────────────────────────────────

    /// Handle `$/onDidChangeTsOrJsFile` notification.
    ///
    /// Called when the client edits a `.ts`, `.js`, or `.vue` file.
    /// Invalidates host caches and re-syncs to the TypeProvider.
    pub async fn on_did_change_ts_or_js_file(&self, params: OnDidChangeTsOrJsFileParams) {
        tracing::debug!("$/onDidChangeTsOrJsFile: {}", params.uri);

        // Skip .vue files — they are synced to the type provider via TSX compilation
        // in sync_tsx_to_provider(). Sending raw Vue SFC source to TSGO (which
        // expects TypeScript) corrupts its internal state.
        if params.uri.ends_with(".vue") {
            return;
        }

        // For non-Vue files tracked by the extension (TS/JS), we notify the
        // type provider so it can update its view of the project.
        if let Some(tp) = &self.type_provider {
            // Reconstruct the full text from the last change (full sync).
            if let Some(last) = params.changes.last() {
                // Convert file:// URI to filesystem path — update_file() calls
                // path_to_uri() internally, so passing a URI would double-wrap it
                // (e.g., file:///file:///...).
                let path = if let Ok(uri) = params.uri.parse::<Uri>() {
                    uri_to_canonical_id(&uri)
                } else {
                    params.uri.clone()
                };
                if let Err(e) = tp.update_file(&path, &last.text).await {
                    tracing::warn!("failed to update file in type provider: {e}");
                }
            }
        }
    }

    /// Handle `$/onFileChanged` notification.
    ///
    /// Called when `node_modules` files are created, updated, or deleted.
    pub async fn on_file_changed(&self, params: OnFileChangedParams) {
        tracing::debug!("$/onFileChanged: {} ({})", params.uri, params.change_type);
        // Future: invalidate module resolution caches, trigger re-analysis
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

        let tsx = self.documents.get_tsx(&parsed_uri);

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
        let edit = crate::features::document_drop_edit::document_drop_edit(
            &params.dropped_uri,
            &params.position,
            &doc.source,
            &blocks,
            &doc.line_index,
            uri,
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
        tracing::debug!("$/verter/getVirtualFiles: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        let response = self.documents.get_virtual_files(&parsed_uri);
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
            .map(|o| verter_host::StyleOverrideEntry {
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
            self.publish_diagnostics(&parsed_uri).await;
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
                verter_host::FileKind::VueSfc => "vue",
                verter_host::FileKind::NonSfc => {
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

            if *file_kind == verter_host::FileKind::VueSfc {
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
        let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(&parsed_uri) else {
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

        for (canonical_id, file_kind) in &file_list {
            if *file_kind != verter_host::FileKind::VueSfc {
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
                            // Resolve the import source relative to the importing file's directory
                            let importer_dir = normalized_id
                                .rfind('/')
                                .map(|i| &normalized_id[..i])
                                .unwrap_or("");
                            let resolved = resolve_import_path(importer_dir, src);
                            if resolved == target_normalized {
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

/// Resolve a relative import path against an importer's directory.
///
/// Handles `./foo.vue`, `../bar/baz.vue`, etc.
/// Does NOT handle alias imports (e.g., `@/components/Foo.vue`).
/// Build the list of workspace components available for auto-import.
///
/// Scans all known .vue files in the host, derives PascalCase names from filenames,
/// and computes relative import paths from the current file.
fn build_workspace_components(
    host: &verter_host::VerterHost,
    current_file_id: &str,
) -> Vec<crate::features::completion::WorkspaceComponent> {
    let files = host.list_files();
    let current_dir = current_file_id
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");

    let mut components = Vec::new();

    for (file_id, kind) in &files {
        // Only .vue files
        if *kind != verter_host::FileKind::VueSfc {
            continue;
        }
        // Skip the current file
        if file_id == current_file_id {
            continue;
        }
        // Skip node_modules
        if file_id.contains("node_modules") {
            continue;
        }

        // Derive component name from filename: `src/components/MyButton.vue` → `MyButton`
        let filename = file_id.rsplit('/').next().unwrap_or(file_id);
        let stem = filename.strip_suffix(".vue").unwrap_or(filename);
        if stem.is_empty() {
            continue;
        }

        // Convert to PascalCase: `my-button` → `MyButton`, `index` stays `Index`
        let component_name = to_pascal_case(stem);

        // Compute relative path from current file to this file
        let import_path = compute_relative_path(current_dir, file_id);

        components.push(crate::features::completion::WorkspaceComponent {
            name: component_name,
            import_path,
        });
    }

    components
}

/// Convert a kebab-case or mixed-case filename stem to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Compute a relative path from `from_dir` to `to_file`.
fn compute_relative_path(from_dir: &str, to_file: &str) -> String {
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_file.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    if ups == 0 {
        format!("./{}", remaining.join("/"))
    } else {
        let up_str = "../".repeat(ups);
        format!("{}{}", up_str, remaining.join("/"))
    }
}

/// Recursively scan a directory for .vue files and upsert them into the host.
/// Skips `node_modules`, dot-directories, and `dist`/`build` directories.
fn scan_vue_files_recursive(
    dir: &std::path::Path,
    host: &verter_host::VerterHost,
    count: &mut u32,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if path.is_dir() {
            // Skip node_modules, dot-dirs, dist, build
            if name == "node_modules" || name.starts_with('.') || name == "dist" || name == "build"
            {
                continue;
            }
            scan_vue_files_recursive(&path, host, count);
        } else if name.ends_with(".vue") {
            // Read file and upsert into host (analysis-only, no compilation needed)
            if let Ok(source) = std::fs::read_to_string(&path) {
                let file_path = path.to_string_lossy().replace('\\', "/");
                let _ = host.upsert(verter_host::UpsertRequest {
                    canonical_id: None,
                    input_id: file_path,
                    source: source.into(),
                    file_kind: verter_host::FileKind::VueSfc,
                    aliases: Vec::new(),
                });
                *count += 1;
            }
        }
    }
}

fn resolve_import_path(importer_dir: &str, import_source: &str) -> String {
    if !import_source.starts_with('.') {
        // Not a relative import — return as-is (alias import)
        return import_source.to_string();
    }

    let mut parts: Vec<&str> = importer_dir.split('/').filter(|s| !s.is_empty()).collect();

    for segment in import_source.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    // Reconstruct: preserve drive letter on Windows (e.g., "C:/...")
    if importer_dir.chars().nth(1) == Some(':') {
        parts.join("/")
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Extract a TypeScript type annotation from a hover markdown string.
///
/// Handles formats like:
/// - "```typescript\nconst x: number\n```"
/// - "(property) x: string"
/// - "let x: Ref<number>"
fn extract_type_from_hover(contents: &str, binding_name: &str) -> Option<String> {
    // Look for pattern: `name: type` or `name = value`
    let patterns = [format!("{binding_name}: "), format!("{binding_name}:")];

    for line in contents.lines() {
        let trimmed = line.trim().trim_start_matches("```typescript").trim();
        for pattern in &patterns {
            if let Some(idx) = trimmed.find(pattern.as_str()) {
                let after = &trimmed[idx + pattern.len()..];
                let type_str = after.trim().trim_end_matches("```").trim();
                if !type_str.is_empty() {
                    return Some(type_str.to_string());
                }
            }
        }
    }

    None
}

impl LanguageServer for VerterLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("verter-lsp initializing");
        tracing::info!(
            "type provider: {}",
            if self.type_provider.is_some() {
                "TSGO connected"
            } else {
                "NONE — no TypeScript intellisense"
            }
        );

        // ── Position encoding negotiation (LSP 3.17) ────────────────────
        // Prefer UTF-8 (native Rust encoding — no conversion needed),
        // then UTF-32, then UTF-16. Default to UTF-16 per LSP spec.
        let encoding = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_ref())
            .and_then(|encodings| {
                if encodings.contains(&PositionEncodingKind::UTF8) {
                    Some(PositionEncodingKind::UTF8)
                } else if encodings.contains(&PositionEncodingKind::UTF32) {
                    Some(PositionEncodingKind::UTF32)
                } else if encodings.contains(&PositionEncodingKind::UTF16) {
                    Some(PositionEncodingKind::UTF16)
                } else {
                    None
                }
            })
            .unwrap_or(PositionEncodingKind::UTF16);
        tracing::info!("negotiated position encoding: {}", encoding.as_str());
        *self.position_encoding.lock().await = encoding.clone();
        self.documents.set_encoding(encoding.clone());

        // Extract and store workspace root
        if let Some(folders) = &params.workspace_folders {
            for folder in folders {
                tracing::info!("workspace folder: {}", folder.uri.as_str());
            }
            if let Some(first) = folders.first() {
                *self.workspace_root.lock().await = Some(first.uri.as_str().to_string());
            }
        }

        // Parse initialization options (statistics config, etc.)
        if let Some(opts) = &params.initialization_options {
            tracing::debug!("initialization options: {opts}");
            if let Some(stats_enabled) = opts
                .get("statistics")
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool())
            {
                self.statistics.set_enabled(stats_enabled);
                tracing::info!(
                    "statistics: {}",
                    if stats_enabled { "enabled" } else { "disabled" }
                );
            }
        }

        Ok(InitializeResult {
            capabilities: server_capabilities(&encoding),
            server_info: Some(ServerInfo {
                name: "verter-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!("verter-lsp initialized");
        let tp_label = if self.type_provider.is_some() {
            "TSGO"
        } else {
            "none"
        };
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "verter-lsp {} initialized (type provider: {tp_label})",
                    env!("CARGO_PKG_VERSION")
                ),
            )
            .await;

        // Build path alias resolver from tsconfig.json (if workspace root is available)
        {
            let root = self.workspace_root.lock().await;
            if let Some(root_uri) = root.as_ref() {
                let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
                let root_path = std::path::PathBuf::from(&canonical);

                // Discover tsconfig.json files
                let mut discovery = crate::config::TsConfigDiscovery::new();
                discovery.discover(&root_path);

                // Use the root tsconfig.json (most common case)
                if let Some(entry) = discovery
                    .find_config_for(&root_path.join("src/dummy.ts"))
                    .or_else(|| discovery.configs().first())
                {
                    let resolver =
                        crate::config::TsConfigPathResolver::from_tsconfig(&entry.config_path);
                    if !resolver.is_empty() {
                        tracing::info!(
                            "path resolver: loaded {} aliases from {}",
                            "tsconfig",
                            entry.config_path.display()
                        );
                        *self.path_resolver.write() = Some(resolver);
                    }
                }
            }
        }

        // Ensure @verter/types is available for TSGO module resolution.
        // If the package is not installed, materialise a minimal version in
        // node_modules so that `import { ... } from "@verter/types"` resolves.
        // This replaces the old `embed_ambient_types` approach which used
        // `declare module` blocks that TSGO cannot resolve.
        {
            let root = self.workspace_root.lock().await;
            if let Some(root_uri) = root.as_ref() {
                // Use the same URI→path conversion as uri_to_canonical_id (handles
                // percent-encoded characters like %3A for ':' on Windows).
                let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
                let root_path = std::path::PathBuf::from(&canonical);
                let types_index = root_path.join("node_modules/@verter/types/index.d.ts");
                if !types_index.exists() {
                    // Write a minimal @verter/types package to node_modules
                    let types_dir = root_path.join("node_modules/@verter/types");
                    match std::fs::create_dir_all(&types_dir) {
                        Ok(()) => {
                            let dts = verter_host::VERTER_TYPES_STANDALONE_DTS;
                            let pkg = r#"{"name":"@verter/types","types":"index.d.ts"}"#;
                            if let Err(e) = std::fs::write(types_dir.join("index.d.ts"), dts) {
                                tracing::warn!("failed to write @verter/types index.d.ts: {e}");
                                // Fall back to embedded ambient types
                                self.documents.set_embed_ambient_types(true);
                            } else if let Err(e) =
                                std::fs::write(types_dir.join("package.json"), pkg)
                            {
                                tracing::warn!("failed to write @verter/types package.json: {e}");
                            } else {
                                tracing::info!(
                                    "@verter/types not installed — materialised at {}",
                                    types_dir.display()
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "failed to create @verter/types dir: {e} — falling back to embed"
                            );
                            self.documents.set_embed_ambient_types(true);
                        }
                    }
                }
            }
        }

        // Scan workspace for .vue files to enable auto-import completions.
        {
            let root = self.workspace_root.lock().await;
            if let Some(root_uri) = root.as_ref() {
                self.scan_workspace_vue_files(root_uri);
            }
        }
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("verter-lsp shutting down");
        self.client
            .log_message(MessageType::INFO, "verter-lsp shutting down")
            .await;
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = &params.text_document.uri;
        let _timer = self
            .statistics
            .timer("did_open", Some(uri.as_str().to_string()));
        tracing::info!("did_open: {}", uri.as_str());
        let result = self.documents.did_open(&params.text_document);
        if result.diagnostics.has_errors {
            tracing::debug!(
                "did_open: {} errors for {}",
                result.diagnostics.diagnostics.len(),
                uri.as_str(),
            );
        }
        self.sync_tsx_to_provider(uri).await;
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let _timer = self
            .statistics
            .timer("did_change", Some(uri.as_str().to_string()));
        let is_virtual = self.documents.get_virtual_source_uri(&uri).is_some();
        tracing::debug!(
            "did_change: {} (v{}, virtual={})",
            uri.as_str(),
            params.text_document.version,
            is_virtual
        );
        let update_result = self.documents.did_change_incremental(
            &uri,
            params.text_document.version,
            params.content_changes,
        );

        // Virtual files don't need TSX sync or diagnostics.
        if is_virtual {
            return;
        }

        let style_only = update_result.changed && update_result.slice_changes.is_style_only();
        if style_only {
            tracing::debug!(
                "did_change: style-only change for {} — skipping TSGO sync",
                uri.as_str()
            );
        }

        // Compute verter diagnostics (sync, CPU-bound) before TSGO sync (async, IPC).
        // Push only verter diagnostics; TSGO diagnostics come via pull path.
        let verter_diags = self.compute_verter_diagnostics(&uri);

        // Use timeout to prevent indefinite hangs when TSGO is slow/crashed.
        if !style_only
            && tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.sync_tsx_to_provider(&uri),
            )
            .await
            .is_err()
        {
            tracing::warn!("did_change: TSGO sync timed out for {}", uri.as_str());
        }

        if tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.publish_diagnostics_with(&uri, verter_diags),
        )
        .await
        .is_err()
        {
            tracing::warn!(
                "did_change: publish_diagnostics timed out for {}",
                uri.as_str()
            );
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = &params.text_document.uri;
        tracing::info!("did_close: {}", uri.as_str());
        // Virtual files don't have TSX in the provider
        if self.documents.get_virtual_source_uri(uri).is_none() {
            if let Some(sync) = &self.project_sync {
                let tsx_path = self.tsx_path_for_uri(uri);
                if let Err(e) = sync.close_tsx(&tsx_path).await {
                    tracing::warn!("did_close: failed to close TSX in provider: {e}");
                }
            }
        }
        self.documents.did_close(uri);
        self.cached_verter_diags.remove(uri.as_str());
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        // No-op; document content is already tracked via did_change
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        for file in &params.files {
            // Only index .vue files
            if !file.uri.ends_with(".vue") {
                continue;
            }
            let uri: Uri = match file.uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let canonical_id = uri_to_canonical_id(&uri);
            // Read and upsert the file so it's indexed without needing to open in editor
            if let Ok(content) =
                std::fs::read_to_string(uri.path().as_str().trim_start_matches('/'))
            {
                let _ = self.documents.host().upsert(verter_host::UpsertRequest {
                    canonical_id: Some(canonical_id),
                    input_id: file.uri.clone(),
                    source: Arc::from(content.as_str()),
                    file_kind: verter_host::FileKind::VueSfc,
                    aliases: vec![],
                });
            }
            tracing::debug!("did_create_files: indexed {}", file.uri);
        }
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        for file in &params.files {
            if !file.uri.ends_with(".vue") {
                continue;
            }
            let uri: Uri = match file.uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let canonical_id = uri_to_canonical_id(&uri);
            self.documents.host().remove(&canonical_id);
            self.cached_verter_diags.remove(uri.as_str());
            tracing::debug!("did_delete_files: removed {}", file.uri);
        }
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let uri = &params.text_document.uri;
        tracing::debug!("diagnostic (pull): {}", uri.as_str());

        let verter_diags = self.compute_verter_diagnostics(uri);

        let diagnostics = if let Some(tp) = &self.type_provider {
            match self.tsx_context(uri) {
                Some((tsx_path, tsx_content, mapper)) => {
                    let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                    let vue_li = self.documents.get(uri).map(|d| d.line_index.clone());
                    match (tp.get_diagnostics(&tsx_path).await, vue_li) {
                        (Ok(type_diags), Some(vue_li)) => {
                            tracing::debug!(
                                "diagnostic (pull): TSGO returned {} for {}",
                                type_diags.len(),
                                uri.as_str()
                            );
                            merge::merge_diagnostics(
                                verter_diags,
                                type_diags,
                                &tsx_li,
                                &mapper,
                                &vue_li,
                            )
                        }
                        (Err(e), _) => {
                            tracing::warn!(
                                "diagnostic (pull): TSGO error for {}: {e}",
                                uri.as_str()
                            );
                            verter_diags
                        }
                        _ => verter_diags,
                    }
                }
                None => verter_diags,
            }
        } else {
            verter_diags
        };

        tracing::debug!(
            "diagnostic (pull): returning {} for {}",
            diagnostics.len(),
            uri.as_str()
        );

        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: diagnostics,
                },
            }),
        ))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let _timer = self
            .statistics
            .timer("hover", Some(uri.as_str().to_string()));
        let position = &params.text_document_position_params.position;
        tracing::debug!(
            "hover: {} at {}:{}",
            uri.as_str(),
            position.line,
            position.character
        );

        // Virtual file: route directly through TSGO (position is already in TSX coordinates)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
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

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            hover_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )
        })();

        // Enhance with TypeProvider if available
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    // Use validated mapping to avoid querying TSGO at synthetic TSX
                    // positions (e.g., <div> → generated JSX) which can crash it.
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        // Log TSX context snippet around the hover offset for debugging
                        if let Some((before, after)) =
                            debug_snippet(&tsx_content, tsx_offset as usize)
                        {
                            tracing::info!(
                                "hover TSX context at offset {}: «{}⸽{}»",
                                tsx_offset,
                                before.replace('\n', "↵"),
                                after.replace('\n', "↵"),
                            );
                        }
                        match tp.get_hover(&tsx_path, tsx_offset).await {
                            Ok(type_hover) => {
                                tracing::info!(
                                    "hover TSGO result: {}",
                                    if type_hover.is_some() {
                                        type_hover
                                            .as_ref()
                                            .map(|h| h.contents.as_str())
                                            .unwrap_or("Some(empty)")
                                    } else {
                                        "None"
                                    }
                                );
                                return Ok(merge::merge_hover(
                                    verter_result,
                                    type_hover,
                                    &mapper,
                                    &tsx_li,
                                    &doc.line_index,
                                ));
                            }
                            Err(e) => {
                                tracing::warn!("hover TSGO error: {}", e);
                            }
                        }
                    } else {
                        tracing::info!(
                            "hover: vue_to_tsx validation failed for {}:{} — position is in synthetic TSX region",
                            position.line,
                            position.character
                        );
                    }
                } else {
                    tracing::info!("hover: no document state for {}", uri.as_str());
                }
            } else {
                tracing::info!("hover: no tsx_context for {}", uri.as_str());
            }
        } else {
            tracing::info!("hover: no type_provider");
        }

        Ok(verter_result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let _timer = self
            .statistics
            .timer("completion", Some(uri.as_str().to_string()));
        let position = &params.text_document_position.position;
        let trigger_character = params
            .context
            .as_ref()
            .and_then(|ctx| ctx.trigger_character.as_deref());
        tracing::debug!(
            "completion: {} at {}:{} (trigger={:?})",
            uri.as_str(),
            position.line,
            position.character,
            trigger_character
        );

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
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

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let canonical_id = crate::documents::uri_to_canonical_id(uri);
            let resolve_component =
                |import_source: &str| -> Option<verter_host::FileAnalysisSnapshot> {
                    // Try 1: Relative import → resolve against current file
                    if import_source.starts_with('.') {
                        let parts: Vec<&str> = canonical_id.split('/').collect();
                        let dir = parts[..parts.len().saturating_sub(1)].join("/");
                        let resolved = if let Some(stripped) = import_source.strip_prefix("./") {
                            format!("{}/{}", dir, stripped)
                        } else if import_source.starts_with("../") {
                            // Simple parent resolution
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
                        if let Some(a) = self.documents.host().get_analysis(&resolved) {
                            return Some(a);
                        }
                    }

                    // Try 2: Path alias resolution (tsconfig paths)
                    let pr_guard = self.path_resolver.read();
                    if let Some(ref resolver) = *pr_guard {
                        if let Some(resolved_path) = resolver.resolve(import_source) {
                            if let Some(a) = self.documents.host().get_analysis(&resolved_path) {
                                return Some(a);
                            }
                        }
                    }
                    drop(pr_guard);

                    // Try 3: Direct lookup (bare specifiers, already-resolved)
                    self.documents.host().get_analysis(import_source)
                };
            // Build workspace component list for auto-import
            let ws_components = build_workspace_components(&self.documents.host, &canonical_id);
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
            )
        })();

        let verter_is_incomplete = verter_result
            .as_ref()
            .map(|r| r.is_incomplete)
            .unwrap_or(false);
        let verter_items = verter_result.map(|r| r.items);

        // Enhance with TypeProvider if available
        if let Some(tp) = &self.type_provider {
            let tsx_ctx = self.tsx_context(uri);
            if tsx_ctx.is_none() {
                tracing::debug!("completion: no tsx_context for {}", uri.as_str());
            }
            if let Some((tsx_path, tsx_content, mapper)) = tsx_ctx {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    );
                    if tsx_offset.is_none() {
                        tracing::debug!(
                            "completion: position mapping failed for {}:{},{}",
                            uri.as_str(),
                            position.line,
                            position.character,
                        );
                    }
                    if let Some(tsx_offset) = tsx_offset {
                        match tp
                            .get_completions(&tsx_path, tsx_offset, trigger_character)
                            .await
                        {
                            Ok(type_result) => {
                                tracing::debug!(
                                    "completion: TSGO returned {} items (incomplete={})",
                                    type_result.items.len(),
                                    type_result.is_incomplete
                                );
                                let (merged, is_incomplete) = merge::merge_completions(
                                    verter_items.unwrap_or_default(),
                                    type_result,
                                    &mapper,
                                    &tsx_li,
                                    &doc.line_index,
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
                                tracing::warn!("completion: TSGO error: {e}");
                            }
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

    async fn completion_resolve(&self, mut item: CompletionItem) -> Result<CompletionItem> {
        // Check if this item requires auto-import
        if let Some(ref data) = item.data {
            if data.get("auto_import").and_then(|v| v.as_bool()) == Some(true) {
                if let (Some(import_path), Some(component_name), Some(doc_uri)) = (
                    data.get("import_path").and_then(|v| v.as_str()),
                    data.get("component_name").and_then(|v| v.as_str()),
                    data.get("uri").and_then(|v| v.as_str()),
                ) {
                    if let Some(edit) =
                        self.build_auto_import_edit(doc_uri, component_name, import_path)
                    {
                        item.additional_text_edits = Some(vec![edit]);
                    }
                }
            }
        }
        Ok(item)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let _timer = self
            .statistics
            .timer("definition", Some(uri.as_str().to_string()));
        let position = &params.text_document_position_params.position;
        tracing::debug!(
            "definition: {} at {}:{}",
            uri.as_str(),
            position.line,
            position.character
        );

        // Virtual file: route directly through TSGO (position is already in TSX coordinates)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_defs) = tp.get_definition(&tsx_path, offset).await {
                        let locations: Vec<Location> = type_defs
                            .into_iter()
                            .filter_map(|d| {
                                // Strip .tsx suffix for .vue.tsx files so user navigates to .vue
                                let target_path = if d.path.ends_with(".vue.tsx") {
                                    d.path.trim_end_matches(".tsx").to_string()
                                } else {
                                    d.path.clone()
                                };
                                let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                                // Convert byte offsets to positions using vf LineIndex for
                                // same-file refs; for external files, fall back to 0:0
                                let range = if d.path == tsx_path {
                                    Range {
                                        start: vf_li
                                            .offset_to_position(d.start)
                                            .unwrap_or_default(),
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
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let pr_guard = self.path_resolver.read();
            let resolve_path = pr_guard
                .as_ref()
                .map(|r| move |specifier: &str| r.resolve(specifier));
            #[allow(clippy::type_complexity)]
            let resolve_fn: Option<&dyn Fn(&str) -> Option<String>> = resolve_path
                .as_ref()
                .map(|f| f as &dyn Fn(&str) -> Option<String>);
            let mut def = definition_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
                resolve_fn,
            )?;

            // Fix up sentinel URIs: if the definition is in the same file, use the document URI
            if let GotoDefinitionResponse::Scalar(ref mut loc) = def {
                if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI {
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
                tracing::debug!("definition: verter resolved cross-file, skipping TSGO");
                return Ok(verter_result);
            }
        }

        // Enhance with TypeProvider for cross-file definitions
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    // Use validated mapping to avoid querying TSGO at synthetic TSX
                    // positions (e.g., <div> → generated JSX) which can crash it.
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        tracing::debug!("definition: querying TSGO at tsx offset {}", tsx_offset);
                        match tp.get_definition(&tsx_path, tsx_offset).await {
                            Ok(type_defs) => {
                                tracing::debug!(
                                    "definition: TSGO returned {} locations",
                                    type_defs.len()
                                );
                                return Ok(merge::merge_definitions(
                                    verter_result,
                                    type_defs,
                                    &tsx_li,
                                    &mapper,
                                    &doc.line_index,
                                ));
                            }
                            Err(e) => {
                                tracing::warn!("definition: TSGO error: {e}");
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
        }

        Ok(verter_result)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let _timer = self
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
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_refs) = tp.get_references(&tsx_path, offset).await {
                        let locations: Vec<Location> = type_refs
                            .into_iter()
                            .filter_map(|r| {
                                let target_path = if r.path.ends_with(".vue.tsx") {
                                    r.path.trim_end_matches(".tsx").to_string()
                                } else {
                                    r.path.clone()
                                };
                                let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                                let range = if r.path == tsx_path {
                                    Range {
                                        start: vf_li
                                            .offset_to_position(r.start)
                                            .unwrap_or_default(),
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
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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
                if loc.uri.as_str() == crate::features::references::SAME_FILE_URI {
                    loc.uri = uri.clone();
                }
            }

            Some(locations)
        })();

        tracing::debug!(
            "references: verter found {}",
            verter_result.as_ref().map_or(0, |v| v.len())
        );

        // Enhance with TypeProvider if available
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        tracing::debug!("references: querying TSGO at tsx offset {}", tsx_offset);
                        match tp.get_references(&tsx_path, tsx_offset).await {
                            Ok(type_refs) => {
                                tracing::debug!(
                                    "references: TSGO returned {} locations",
                                    type_refs.len()
                                );
                                return Ok(merge::merge_references(
                                    verter_result,
                                    type_refs,
                                    &tsx_li,
                                    &mapper,
                                    &doc.line_index,
                                ));
                            }
                            Err(e) => {
                                tracing::warn!("references: TSGO error: {e}");
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
        }

        Ok(verter_result)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let position = &params.position;

        // Virtual file: not supported (no Verter rename context for generated code)
        if self.documents.get_virtual_source_uri(uri).is_some() {
            return Ok(None);
        }

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;
        let new_name = &params.new_name;

        // Virtual file: not supported (renaming in generated code isn't meaningful)
        if self.documents.get_virtual_source_uri(uri).is_some() {
            return Ok(None);
        }

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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
                let sentinel: Uri = crate::features::rename::SAME_FILE_URI.parse().unwrap();
                if let Some(edits) = changes.remove(&sentinel) {
                    changes.insert(uri.clone(), edits);
                }
            }

            Some(edit)
        })();

        // Enhance with TypeProvider for cross-file renames
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_locs) = tp.get_rename_locations(&tsx_path, tsx_offset).await
                        {
                            return Ok(merge::merge_rename_locations(
                                verter_result,
                                type_locs,
                                new_name,
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

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let symbols = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;

        let ranges = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let uri = &params.text_document.uri;

        let result = (|| {
            let doc = self.documents.get(uri)?;
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

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_highlights) = tp.get_document_highlights(&tsx_path, offset).await
                    {
                        let highlights: Vec<DocumentHighlight> = type_highlights
                            .into_iter()
                            .filter_map(|h| {
                                Some(DocumentHighlight {
                                    range: Range {
                                        start: vf_li.offset_to_position(h.start)?,
                                        end: vf_li.offset_to_position(h.end)?,
                                    },
                                    kind: Some(match h.kind {
                                        crate::tsgo::protocol::TypeDocumentHighlightKind::Read => {
                                            DocumentHighlightKind::READ
                                        }
                                        crate::tsgo::protocol::TypeDocumentHighlightKind::Write => {
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
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
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

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_sig) = tp.get_signature_help(&tsx_path, offset).await {
                        return Ok(merge::merge_signature_help(type_sig));
                    }
                }
                return Ok(None);
            }
        }

        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_sig) = tp.get_signature_help(&tsx_path, tsx_offset).await {
                            return Ok(merge::merge_signature_help(type_sig));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let range = &params.range;

        let mut all_actions: Vec<CodeActionOrCommand> = Vec::new();

        // Verter's own code actions (organize imports)
        if let Some(doc) = self.documents.get(uri) {
            let analysis = self.documents.get_analysis(uri);
            let mut verter_actions =
                organize_imports_actions(&doc.source, analysis.as_ref(), &doc.line_index);
            fix_placeholder_uris(&mut verter_actions, uri);
            all_actions.extend(verter_actions);

            // Extract component refactoring
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

            // Macro code actions (defineSlots, defineEmits generation/augmentation)
            let mut macro_actions = crate::features::macro_actions::macro_code_actions(
                &doc.source,
                analysis.as_ref(),
                &blocks,
                &doc.line_index,
            );
            fix_placeholder_uris(&mut macro_actions, uri);
            all_actions.extend(macro_actions);

            // Component code actions (add unknown props/v-models to child)
            if let Some(ref analysis) = analysis {
                let comp_actions = crate::features::component_actions::component_code_actions(
                    analysis,
                    &|import_source| self.resolve_component_context(uri, import_source),
                );
                all_actions.extend(comp_actions);

                // Event handler type hint actions
                let mut event_actions = crate::features::event_type_hints::event_type_hint_actions(
                    analysis,
                    &doc.source,
                    &doc.line_index,
                );
                fix_placeholder_uris(&mut event_actions, uri);
                all_actions.extend(event_actions);

                // Action engine quick fixes (e.g., remove unused CSS selector)
                all_actions.extend(crate::features::diagnostics_bridge::action_engine_fixes(
                    &self.action_engine,
                    analysis,
                    &doc.source,
                    &doc.line_index,
                    &self.linter,
                    &params.context.diagnostics,
                    uri,
                ));
            }
        }

        // TypeProvider code actions (TSGO quick fixes, refactorings)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    let start_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.start,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    );
                    let end_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.end,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    );
                    if let (Some(so), Some(eo)) = (start_offset, end_offset) {
                        if let Ok(type_actions) = tp.get_code_actions(&tsx_path, so, eo).await {
                            let actions = merge::merge_code_actions(
                                type_actions,
                                &tsx_li,
                                &mapper,
                                &doc.line_index,
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

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;

        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    if let Ok(type_tokens) = tp.get_semantic_tokens(&tsx_path).await {
                        let tokens = merge::merge_semantic_tokens(
                            type_tokens,
                            &tsx_li,
                            &mapper,
                            &doc.line_index,
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

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;

        let lenses = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            Some(code_lenses(&blocks, analysis.as_ref(), &doc.line_index))
        })();

        match lenses {
            Some(v) if !v.is_empty() => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let range = &params.range;

        // Virtual file: route directly through TSGO (positions already in TSX coordinates)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                let start = vf_li.position_to_offset(&range.start);
                let end = vf_li.position_to_offset(&range.end);
                if let (Some(so), Some(eo)) = (start, end) {
                    if let Ok(type_hints) = tp.get_inlay_hints(&tsx_path, so, eo).await {
                        let hints: Vec<InlayHint> = type_hints
                            .into_iter()
                            .filter_map(|h| {
                                let pos = vf_li.offset_to_position(h.position)?;
                                let kind = h.kind.map(|k| match k {
                                    crate::tsgo::protocol::InlayHintKind::Type => {
                                        InlayHintKind::TYPE
                                    }
                                    crate::tsgo::protocol::InlayHintKind::Parameter => {
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

        // Collect Verter-specific hints (DOM queries, useTemplateRef)
        let mut hints: Vec<InlayHint> = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri)?;
            let blocks = scan_sfc_blocks(&doc.source);
            Some(crate::features::inlay_hints::verter_inlay_hints(
                &doc.source,
                &blocks,
                &analysis,
                &doc.line_index,
            ))
        })()
        .unwrap_or_default();

        // Standard .vue file: merge with TSGO type hints when available
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    let start_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.start,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    );
                    let end_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.end,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    );
                    if let (Some(so), Some(eo)) = (start_offset, end_offset) {
                        if let Ok(type_hints) = tp.get_inlay_hints(&tsx_path, so, eo).await {
                            let mut tsgo_hints = merge::merge_inlay_hints(
                                type_hints,
                                &tsx_li,
                                &mapper,
                                &doc.line_index,
                            );
                            hints.append(&mut tsgo_hints);
                        }
                    }
                }
            }
        }

        Ok(if hints.is_empty() { None } else { Some(hints) })
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = &params.text_document.uri;

        let links = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let links =
                build_document_links(&doc.source, &blocks, analysis.as_ref(), &doc.line_index);
            if links.is_empty() {
                None
            } else {
                Some(links)
            }
        })();

        Ok(links)
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        let uri = &params.text_document.uri;

        let colors = (|| {
            let doc = self.documents.get(uri)?;
            let blocks = scan_sfc_blocks(&doc.source);
            Some(color_info::document_colors(
                &doc.source,
                &blocks,
                &doc.line_index,
            ))
        })();

        Ok(colors.unwrap_or_default())
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        Ok(color_info::color_presentations(&params.color))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;

        let edits = (|| {
            let doc = self.documents.get(uri)?;
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

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;

        let edits = (|| {
            let doc = self.documents.get(uri)?;
            let offset = doc.line_index.position_to_offset(position)? as usize;
            let snippet = crate::features::auto_close_tag::auto_close_tag(&doc.source, offset)?;

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

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let symbols = workspace_symbols(&self.documents.host, &params.query);
        Ok(if symbols.is_empty() {
            None
        } else {
            Some(symbols)
        })
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let uri = &params.item.uri;

        let calls = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let uri = &params.item.uri;

        let calls = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
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
}

/// Extract a debug snippet around `offset` in `content`, returning `(before_cursor, after_cursor)`.
/// Returns `None` if the offset is out of bounds.
fn debug_snippet(content: &str, offset: usize) -> Option<(String, String)> {
    if offset > content.len() {
        return None;
    }
    // Snap to char boundaries so we never slice inside a multi-byte UTF-8 sequence
    let snippet_start = content.floor_char_boundary(offset.saturating_sub(20));
    let snippet_end = content.ceil_char_boundary((offset + 30).min(content.len()));
    let cursor = content.floor_char_boundary(offset);
    if snippet_end <= snippet_start || cursor < snippet_start || cursor > snippet_end {
        return None;
    }
    let before = &content[snippet_start..cursor];
    let after = &content[cursor..snippet_end];
    Some((before.to_string(), after.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_snippet_ascii() {
        let content = "abcdefghijklmnopqrstuvwxyz0123456789";
        let (before, after) = debug_snippet(content, 10).unwrap();
        assert_eq!(before, "abcdefghij");
        assert_eq!(after.len(), 26); // 10..40 clamped to 10..36 = 26
    }

    #[test]
    fn debug_snippet_multibyte_offset_inside_char() {
        // "否" is 3 bytes in UTF-8 (E5 90 A6). Place offset at byte 1 = middle of '否'.
        let content = "否abc";
        // byte 0..3 = '否', 3 = 'a', 4 = 'b', 5 = 'c'
        // offset 1 is inside '否' — must NOT panic, snaps to char boundary
        let (before, after) = debug_snippet(content, 1).unwrap();
        // Cursor snaps back to byte 0 (start of '否')
        assert!(before.is_empty(), "cursor snapped to start");
        assert!(after.contains('否'), "after contains the full character");
        assert!(after.contains('a'), "after contains subsequent ASCII");
    }

    #[test]
    fn debug_snippet_multibyte_in_snippet_window() {
        // Reproduces the crash scenario: Chinese characters in JSDoc comments
        // with offset landing in the middle of a multi-byte char
        let content = "  /** 是否显示冷返 */\n  cold?: boolean";
        // '是' starts at byte 6, '否' at byte 9 (each CJK char is 3 bytes)
        // offset 8 lands inside '是' — must NOT panic
        let (before, after) = debug_snippet(content, 8).unwrap();
        // Cursor snaps to byte 6 (start of '是')
        assert!(before.ends_with(' '), "before ends at space before CJK");
        assert!(
            after.starts_with('是'),
            "after starts at snapped char boundary"
        );
        assert!(
            !before.contains('\u{FFFD}'),
            "no replacement chars in before"
        );
        assert!(!after.contains('\u{FFFD}'), "no replacement chars in after");
    }

    #[test]
    fn debug_snippet_at_exact_char_boundary() {
        let content = "abc否def";
        // '否' is at bytes 3..6
        let (before, after) = debug_snippet(content, 3).unwrap();
        assert!(before.ends_with('c'));
        assert!(after.starts_with('否'));
    }

    #[test]
    fn debug_snippet_out_of_bounds() {
        let content = "abc";
        assert!(debug_snippet(content, 100).is_none());
    }

    #[test]
    fn debug_snippet_at_end() {
        let content = "abc";
        let result = debug_snippet(content, 3);
        // offset == len is valid (cursor at end)
        assert!(result.is_some());
    }
}
