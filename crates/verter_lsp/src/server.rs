use std::sync::Arc;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::capabilities::server_capabilities;
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::DocumentRegistry;
use crate::features::completion::completions_at_position;
use crate::features::definition::definition_at_position;
use crate::features::diagnostics::map_diagnostics;
use crate::features::document_highlight::highlights_at_position;
use crate::features::document_symbol::build_document_symbols;
use crate::features::folding_range::build_folding_ranges;
use crate::features::hover::hover_at_position;
use crate::features::references::references_at_position;
use crate::features::rename::{prepare_rename, rename_at_position};
use crate::tsgo::merge;
use crate::tsgo::project_sync::ProjectSync;
use crate::tsgo::traits::TypeProvider;
use crate::LspConfig;

/// The Verter language server implementation.
///
/// Wraps `verter_host` for SFC analysis and optionally a `TypeProvider`
/// (e.g., TSGO) for richer type information.
pub struct VerterLanguageServer {
    client: Client,
    documents: DocumentRegistry,
    type_provider: Option<Arc<dyn TypeProvider>>,
    project_sync: Option<ProjectSync>,
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
        }
    }

    /// Publish diagnostics for a document URI, merging verter + TypeProvider diagnostics.
    async fn publish_diagnostics(&self, uri: &Uri) {
        let verter_diags = if let Some(doc) = self.documents.get(uri) {
            let host_diags = self.documents.get_diagnostics(uri);
            match host_diags {
                Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
                None => vec![],
            }
        } else {
            vec![]
        };

        let diagnostics = if let Some(tp) = &self.type_provider {
            match self.tsx_context(uri) {
                Some((tsx_path, tsx_content, mapper)) => {
                    let tsx_li = LineIndex::new(&tsx_content);
                    let vue_li = self.documents.get(uri).map(|d| d.line_index.clone());
                    match (tp.get_diagnostics(&tsx_path).await, vue_li) {
                        (Ok(type_diags), Some(vue_li)) => merge::merge_diagnostics(
                            verter_diags,
                            type_diags,
                            &tsx_li,
                            &mapper,
                            &vue_li,
                        ),
                        _ => verter_diags,
                    }
                }
                None => verter_diags,
            }
        } else {
            verter_diags
        };

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    /// Sync a Vue file's TSX to the TypeProvider after open/change.
    async fn sync_tsx_to_provider(&self, uri: &Uri) {
        if let Some(sync) = &self.project_sync {
            if let Some(tsx) = self.documents.get_tsx(uri) {
                let tsx_path = self.tsx_path_for_uri(uri);
                if let Err(e) = sync.sync_tsx(&tsx_path, &tsx.code).await {
                    tracing::warn!("failed to sync TSX to provider: {e}");
                }
            }
        }
    }

    /// Get TSX context for TypeProvider queries: (tsx_path, tsx_code, position_mapper).
    fn tsx_context(&self, uri: &Uri) -> Option<(String, Arc<str>, PositionMapper)> {
        let tsx = self.documents.get_tsx(uri)?;
        let mapper = self.documents.get_position_mapper(uri)?;
        let tsx_path = self.tsx_path_for_uri(uri);
        Some((tsx_path, tsx.code, mapper))
    }

    /// Generate the TSX file path for a given Vue file URI.
    fn tsx_path_for_uri(&self, uri: &Uri) -> String {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        format!("{canonical}.tsx")
    }
}

impl LanguageServer for VerterLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("verter-lsp initializing");

        if let Some(folders) = &params.workspace_folders {
            for folder in folders {
                tracing::info!("workspace folder: {}", folder.uri.as_str());
            }
        }

        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: "verter-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!("verter-lsp initialized");
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("verter-lsp shutting down");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let result = self.documents.did_open(&params.text_document);
        if result.diagnostics.has_errors {
            tracing::debug!(
                "diagnostics on open for {}: {} errors",
                params.text_document.uri.as_str(),
                result.diagnostics.diagnostics.len()
            );
        }
        self.sync_tsx_to_provider(&params.text_document.uri).await;
        self.publish_diagnostics(&params.text_document.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.last() {
            let _ = self
                .documents
                .did_change(&uri, params.text_document.version, &change.text);
        }
        self.sync_tsx_to_provider(&uri).await;
        self.publish_diagnostics(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = &params.text_document.uri;
        // Close the TSX file in the provider
        if let Some(sync) = &self.project_sync {
            let tsx_path = self.tsx_path_for_uri(uri);
            if let Err(e) = sync.close_tsx(&tsx_path).await {
                tracing::warn!("failed to close TSX in provider: {e}");
            }
        }
        self.documents.did_close(uri);
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        // No-op; document content is already tracked via did_change
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

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
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_hover) = tp.get_hover(&tsx_path, tsx_offset).await {
                            return Ok(merge::merge_hover(
                                verter_result,
                                type_hover,
                                &mapper,
                                &tsx_li,
                                &doc.line_index,
                            ));
                        }
                    }
                }
            }
        }

        Ok(verter_result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;

        let verter_items = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            completions_at_position(
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
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_completions) =
                            tp.get_completions(&tsx_path, tsx_offset).await
                        {
                            let merged = merge::merge_completions(
                                verter_items.unwrap_or_default(),
                                type_completions,
                                &mapper,
                                &tsx_li,
                                &doc.line_index,
                            );
                            return Ok(if merged.is_empty() {
                                None
                            } else {
                                Some(CompletionResponse::Array(merged))
                            });
                        }
                    }
                }
            }
        }

        Ok(verter_items.map(CompletionResponse::Array))
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        // Future: resolve with TypeProvider details (documentation, etc.)
        Ok(item)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let mut def = definition_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )?;

            // Fix up sentinel URIs: if the definition is in the same file, use the document URI
            if let GotoDefinitionResponse::Scalar(ref mut loc) = def {
                if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI {
                    loc.uri = uri.clone();
                }
            }

            Some(def)
        })();

        // Enhance with TypeProvider for cross-file definitions
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_defs) = tp.get_definition(&tsx_path, tsx_offset).await {
                            return Ok(merge::merge_definitions(
                                verter_result,
                                type_defs,
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

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

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

        // Enhance with TypeProvider if available
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_refs) = tp.get_references(&tsx_path, tsx_offset).await {
                            return Ok(merge::merge_references(
                                verter_result,
                                type_refs,
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

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let position = &params.position;

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
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
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
            let blocks = scan_sfc_blocks(&doc.source);
            let ranges = build_folding_ranges(&blocks, &doc.line_index);
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
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
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

        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset(
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

        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content);
                if let Some(doc) = self.documents.get(uri) {
                    let start_offset = merge::vue_position_to_tsx_offset(
                        &range.start,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    );
                    let end_offset = merge::vue_position_to_tsx_offset(
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
                            if !actions.is_empty() {
                                return Ok(Some(actions));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;

        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.tsx_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content);
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
}
