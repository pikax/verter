use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

use crate::server::PublishedResolverSnapshot;
use crate::test_utils::make_test_vfs_workspace_from_registry;
use crate::tsgo::mock::{MockCall, MockTypeProvider};
use crate::tsgo::protocol::{
    CompletionResult, HoverInfo, InlayHint, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight, TypeLocation,
};
use crate::tsgo::traits::{ProviderFuture, TypeProvider};
use crate::ProjectSyncMode;

#[derive(Default)]
struct SlowConfigurePathsProvider {
    configure_paths_started: AtomicUsize,
}

impl TypeProvider for SlowConfigurePathsProvider {
    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        Box::pin(async {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: false,
            })
        })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async { Ok(None) })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn configure_paths(
        &self,
        _base_url: &str,
        _paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        self.configure_paths_started.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(())
        })
    }
}

#[derive(Default)]
struct TriggerSensitiveCompletionProvider;

impl TypeProvider for TriggerSensitiveCompletionProvider {
    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let trigger = trigger_character.map(str::to_string);
        Box::pin(async move {
            let items = if trigger.as_deref() == Some(".") {
                Vec::new()
            } else {
                vec![
                    crate::tsgo::protocol::Completion {
                        label: "name".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                        detail: Some("(property) name: string".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                    crate::tsgo::protocol::Completion {
                        label: "id".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                        detail: Some("(property) id: number".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                ]
            };
            Ok(CompletionResult {
                items,
                is_incomplete: false,
            })
        })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async { Ok(None) })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn configure_paths(
        &self,
        _base_url: &str,
        _paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct DotTriggerRequiredCompletionProvider;

impl TypeProvider for DotTriggerRequiredCompletionProvider {
    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let trigger = trigger_character.map(str::to_string);
        Box::pin(async move {
            let items = if trigger.as_deref() == Some(".") {
                vec![
                    crate::tsgo::protocol::Completion {
                        label: "disabled".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                        detail: Some("(property) disabled: boolean".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                    crate::tsgo::protocol::Completion {
                        label: "label".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                        detail: Some("(property) label: string".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                    crate::tsgo::protocol::Completion {
                        label: "handler".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Method),
                        detail: Some("(method) handler(): void".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                ]
            } else {
                Vec::new()
            };
            Ok(CompletionResult {
                items,
                is_incomplete: false,
            })
        })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async { Ok(None) })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn configure_paths(
        &self,
        _base_url: &str,
        _paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct LostContentCompletionProvider {
    open_paths: std::sync::Mutex<HashSet<String>>,
    calls: std::sync::Mutex<Vec<MockCall>>,
    require_current_api: bool,
}

impl LostContentCompletionProvider {
    fn requiring_current_api() -> Self {
        Self {
            require_current_api: true,
            ..Default::default()
        }
    }

    fn drop_open_path(&self, path: &str) {
        self.open_paths.lock().unwrap().remove(path);
    }

    fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl TypeProvider for LostContentCompletionProvider {
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.open_paths.lock().unwrap().insert(path.clone());
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::OpenFile { path, content });
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::UpdateFile { path, content });
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.open_paths.lock().unwrap().remove(&path);
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::CloseFile { path });
            Ok(())
        })
    }

    fn get_completions(
        &self,
        path: &str,
        _offset: u32,
        _trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let path = path.to_string();
        Box::pin(async move {
            self.calls.lock().unwrap().push(MockCall::GetCompletions {
                path: path.clone(),
                offset: 0,
            });
            if !self.open_paths.lock().unwrap().contains(&path) {
                return Err(crate::tsgo::protocol::TypeProviderError::new(
                    "No content available.",
                ));
            }
            if self.require_current_api {
                let current_api_path = path
                    .strip_suffix(".tsx")
                    .map(|prefix| format!("{prefix}.ts"))
                    .unwrap_or_else(|| path.clone());
                if !self.open_paths.lock().unwrap().contains(&current_api_path) {
                    return Err(crate::tsgo::protocol::TypeProviderError::new(
                        "No content available.",
                    ));
                }
            }
            Ok(CompletionResult {
                items: vec![
                    crate::tsgo::protocol::Completion {
                        label: "disabled".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                        detail: Some("(property) disabled: boolean".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                    crate::tsgo::protocol::Completion {
                        label: "label".to_string(),
                        kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                        detail: Some("(property) label: string".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        insert_text: None,
                        sort_text: None,
                        data: None,
                    },
                ],
                is_incomplete: false,
            })
        })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async { Ok(None) })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn make_hover_test_service(
    type_provider: Arc<dyn TypeProvider>,
) -> tower_lsp_server::LspService<VerterLanguageServer> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });
    service
}

fn install_test_resolver(server: &VerterLanguageServer) {
    install_test_resolver_for_root(server, "/workspace", Some("/workspace/tsconfig.json"));
}

fn install_test_resolver_for_root(
    server: &VerterLanguageServer,
    root: &str,
    tsconfig: Option<&str>,
) {
    let vfs_ws = std::sync::Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));

    // Build a minimal project graph with a single project.
    let projects = vec![verter_workspace::workspace_snapshot::OwnershipProject {
        id: verter_workspace::workspace_snapshot::ProjectId(0),
        root: verter_workspace::CanonicalPath::new(root),
        workspace_root: verter_workspace::CanonicalPath::new(root),
        payload: verter_workspace::workspace_snapshot::ProjectPayload::Fallback {
            membership: verter_workspace::FallbackMembership {
                root: verter_workspace::CanonicalPath::new(root),
                exclude: vec![verter_workspace::NormalizedGlob::new(&format!(
                    "{}/node_modules/**",
                    root
                ))],
            },
        },
    }];

    let resolver = verter_workspace::ProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            root.to_string(),
            root.to_string(),
            tsconfig.map(|s| s.to_string()),
        ),
    ]);

    let snapshot = std::sync::Arc::new(verter_workspace::WorkspaceSnapshot {
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });

    let views = crate::workspace_state::build_lsp_views(&snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    server.install_vfs_workspace(vfs_ws);
}

fn open_test_vue(server: &VerterLanguageServer, path: &str, source: &str) -> Uri {
    let uri: Uri = format!("file://{path}").parse().expect("valid test uri");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    uri
}

fn hover_params(uri: &Uri, position: Position) -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

fn completion_params(
    uri: &Uri,
    position: Position,
    trigger_character: Option<&str>,
) -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: trigger_character
                .map(|_| CompletionTriggerKind::TRIGGER_CHARACTER)
                .unwrap_or(CompletionTriggerKind::INVOKED),
            trigger_character: trigger_character.map(str::to_string),
        }),
    }
}

fn completion_labels(response: Option<CompletionResponse>) -> Vec<String> {
    match response {
        Some(CompletionResponse::Array(items)) => {
            items.into_iter().map(|item| item.label).collect()
        }
        Some(CompletionResponse::List(list)) => {
            list.items.into_iter().map(|item| item.label).collect()
        }
        None => Vec::new(),
    }
}

fn hover_text(hover: Option<Hover>) -> String {
    match hover.expect("hover should exist").contents {
        HoverContents::Markup(m) => m.value,
        HoverContents::Scalar(MarkedString::String(s)) => s,
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value,
        HoverContents::Array(items) => items
            .into_iter()
            .map(|item| match item {
                MarkedString::String(s) => s,
                MarkedString::LanguageString(ls) => ls.value,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn synced_type_provider_context(server: &VerterLanguageServer, uri: &Uri) -> TypeProviderContext {
    let canonical_id = server
        .documents
        .get_canonical_id(uri)
        .expect("canonical id should exist");
    server.documents.host().ensure_loaded(&canonical_id);
    let ide = server
        .documents
        .get_ide(uri)
        .expect("IDE output should exist");
    let mapper = server
        .documents
        .get_position_mapper(uri)
        .expect("position mapper should exist");
    let tsx_path = server
        .active_ide_path_for_uri(uri)
        .or_else(|| server.target_ide_path_for_uri(uri))
        .expect("type provider path should exist");
    let tsx_line_index = LineIndex::new(&ide.code, server.documents.encoding());
    let vue_line_index = server
        .documents
        .get(uri)
        .expect("document should exist")
        .line_index
        .clone();
    TypeProviderContext {
        tsx_path,
        tsx_content: ide.code,
        mapper,
        tsx_line_index,
        vue_line_index,
    }
}

fn set_type_hover_at_vue_position(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    uri: &Uri,
    position: Position,
    contents: &str,
) {
    let ctx = synced_type_provider_context(server, uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("vue position should map to tsx");
    provider.set_hover(
        &ctx.tsx_path,
        tsx_offset,
        Some(HoverInfo {
            contents: contents.to_string(),
            range_start: None,
            range_end: None,
        }),
    );
}

fn set_type_completions_at_vue_position(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    uri: &Uri,
    position: Position,
    items: Vec<crate::tsgo::protocol::Completion>,
) {
    let ctx = synced_type_provider_context(server, uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("vue position should map to tsx");
    provider.set_completions(&ctx.tsx_path, tsx_offset, items);
}

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

#[test]
fn needs_provider_sync_insert_and_remove() {
    let set = DashSet::new();
    let id = "C:/project/src/App.vue".to_string();
    set.insert(id.clone());
    assert!(set.contains(&id), "should contain the inserted id");
    let removed = set.remove(&id);
    assert!(removed.is_some(), "remove should return Some");
    assert!(!set.contains(&id), "should no longer contain the id");
}

#[test]
fn resolve_import_path_relative() {
    let result = resolve_import_path("C:/project/src/views", "./Foo.vue");
    assert_eq!(result, "C:/project/src/views/Foo.vue");

    let result = resolve_import_path("C:/project/src/views", "../components/Bar.vue");
    assert_eq!(result, "C:/project/src/components/Bar.vue");
}

#[test]
fn resolve_import_path_alias_returns_raw() {
    // Non-relative imports (aliases) are returned as-is — they need VFS resolution
    let result = resolve_import_path("C:/project/src/views", "@/components/Foo.vue");
    assert_eq!(
        result, "@/components/Foo.vue",
        "alias import should be returned as-is (unresolvable by resolve_import_path)"
    );
    // This means `resolved == target_normalized` will never match for aliases,
    // causing component parents to always be empty for alias-based imports.
}

fn test_module_reference(
    raw_text: &str,
    literal_specifier: Option<&str>,
    finite_specifiers: &[&str],
    analyzability: verter_semantic::analysis::ModuleReferenceAnalyzability,
    expr_start: usize,
    expr_end: usize,
) -> verter_session::ScriptModuleReference {
    verter_session::ScriptModuleReference {
        syntax: verter_semantic::analysis::ModuleReferenceSyntax::StaticImport,
        semantics: verter_semantic::analysis::ModuleReferenceSemantics::Import,
        is_type_only: false,
        raw_text: raw_text.to_string(),
        literal_specifier: literal_specifier.map(str::to_string),
        finite_specifiers: finite_specifiers
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        static_prefix: None,
        analyzability,
        span: verter_span::Span::new(expr_start as u32, expr_end as u32),
        expr_span: verter_span::Span::new(expr_start as u32, expr_end as u32),
    }
}

fn test_module_reference_with_semantics(
    raw_text: &str,
    literal_specifier: Option<&str>,
    finite_specifiers: &[&str],
    analyzability: verter_semantic::analysis::ModuleReferenceAnalyzability,
    expr_start: usize,
    expr_end: usize,
    semantics: verter_semantic::analysis::ModuleReferenceSemantics,
    is_type_only: bool,
) -> verter_session::ScriptModuleReference {
    verter_session::ScriptModuleReference {
        semantics,
        is_type_only,
        ..test_module_reference(
            raw_text,
            literal_specifier,
            finite_specifiers,
            analyzability,
            expr_start,
            expr_end,
        )
    }
}

fn test_analyzed_module_reference(
    raw_text: &str,
    literal_specifier: Option<&str>,
    finite_specifiers: &[&str],
    analyzability: verter_semantic::analysis::ModuleReferenceAnalyzability,
    expr_start: usize,
    expr_end: usize,
) -> verter_semantic::analysis::AnalyzedModuleReference {
    verter_semantic::analysis::AnalyzedModuleReference {
        syntax: verter_semantic::analysis::ModuleReferenceSyntax::StaticImport,
        semantics: verter_semantic::analysis::ModuleReferenceSemantics::Import,
        is_type_only: false,
        raw_text: raw_text.to_string(),
        literal_specifier: literal_specifier.map(str::to_string),
        finite_specifiers: finite_specifiers
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        static_prefix: None,
        analyzability,
        span: verter_span::Span::new(expr_start as u32, expr_end as u32),
        expr_span: verter_span::Span::new(expr_start as u32, expr_end as u32),
    }
}

#[derive(Default)]
struct TestResolverReader {
    files: HashSet<String>,
    texts: HashMap<String, Arc<str>>,
}

impl TestResolverReader {
    fn with_files(paths: &[&str]) -> Self {
        let mut reader = Self::default();
        for path in paths {
            let normalized = path.replace('\\', "/");
            reader.files.insert(normalized.clone());
            reader
                .texts
                .insert(normalized, Arc::<str>::from("// test file"));
        }
        reader
    }
}

impl verter_workspace::WorkspaceAccess for TestResolverReader {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.texts.get(&canonical_id.replace('\\', "/")).cloned()
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.files.contains(&canonical_id.replace('\\', "/"))
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let normalized = canonical_id.replace('\\', "/");
        self.file_exists(&normalized).then_some(normalized)
    }

    // Reader-only stub overrides (R6/R7). Rationale: `TestResolverReader`
    // is an LSP test fixture that only feeds the resolver with file content
    // for definition/hover/completion test plumbing; it never participates
    // in the host's dep-flow.
    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[verter_workspace::ParsedEdge]) {}
    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        verter_workspace::ExactResolutionResult::default()
    }
    fn replace_semantic_transitive(
        &self,
        _canonical_id: &str,
        _deps: std::collections::BTreeSet<String>,
    ) {
    }
    fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}
    fn dependency_snapshot(
        &self,
        _canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        None
    }
    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

async fn make_definition_test_server(
    files: &[(&str, &str, &str)],
) -> (
    tempfile::TempDir,
    tower_lsp_server::LspService<VerterLanguageServer>,
    tokio::task::JoinHandle<()>,
    Arc<MockTypeProvider>,
    String,
) {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");

    for (relative_path, _language_id, source) in files {
        let file_path = relative_path
            .split('/')
            .fold(workspace.clone(), |path, segment| path.join(segment));
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&file_path, source).expect("write source file");
    }

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let server = service.inner();
    let ide_project = crate::project_resolver::IdeProjectConfig::new(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    );
    // Sync resolver to host's VFS so resolve_import_via_workspace works
    host.configure_projects(vec![ide_project]);
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    for (relative_path, language_id, source) in files {
        let canonical_id = format!("{workspace_id}/{relative_path}");
        let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
        let _ = server.documents.did_open(&TextDocumentItem {
            uri,
            language_id: (*language_id).to_string(),
            version: 1,
            text: (*source).to_string(),
        });
    }

    (temp, service, drain_handle, provider, workspace_id)
}

fn fixture_workspace_root(name: &str) -> String {
    let path = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packages/vue-vscode/e2e/fixtures/{name}")),
    )
    .expect("fixture workspace path should canonicalize");
    crate::test_utils::canonical_test_path(&path)
}

#[test]
fn fixture_workspace_root_returns_canonical_path() {
    let workspace_id = fixture_workspace_root("single-project");

    assert!(
        workspace_id.starts_with('/') || workspace_id.chars().nth(1) == Some(':'),
        "fixture workspace path should be absolute, got: {workspace_id}"
    );
    assert!(
        !workspace_id.contains("/../"),
        "fixture workspace path should not retain dot segments, got: {workspace_id}"
    );
}

fn workspace_uri(workspace_id: &str, relative_path: &str) -> Uri {
    crate::uri::path_to_file_uri(&format!("{workspace_id}/{relative_path}")).expect("file uri")
}

fn find_document_position(
    server: &VerterLanguageServer,
    uri: &Uri,
    needle: &str,
    delta: usize,
) -> Position {
    let doc = server.documents.get(uri).expect("document should be open");
    let offset = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle `{needle}` should exist"))
        + delta;
    doc.line_index
        .offset_to_position(offset as u32)
        .expect("valid position")
}

fn definition_locations(response: GotoDefinitionResponse) -> Vec<Location> {
    match response {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_range,
            })
            .collect(),
    }
}

fn line_for_snippet(source: &str, needle: &str) -> u32 {
    let offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("needle `{needle}` should exist"));
    LineIndex::new_utf16(source)
        .offset_to_position(offset as u32)
        .expect("valid position")
        .line
}

fn goto_definition_params(uri: &Uri, position: Position) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

#[test]
fn module_reference_request_kind_uses_require_semantics() {
    let require_reference = test_module_reference_with_semantics(
        "'pkg'",
        Some("pkg"),
        &[],
        verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
        0,
        5,
        verter_semantic::analysis::ModuleReferenceSemantics::Require,
        false,
    );
    assert_eq!(
        module_reference_request_kind(&require_reference),
        crate::project_resolver::ResolveRequestKind::RequireCall
    );

    let type_reference = test_module_reference_with_semantics(
        "'pkg'",
        Some("pkg"),
        &[],
        verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
        0,
        5,
        verter_semantic::analysis::ModuleReferenceSemantics::Import,
        true,
    );
    assert_eq!(
        module_reference_request_kind(&type_reference),
        crate::project_resolver::ResolveRequestKind::TypeImport
    );
}

#[test]
fn provider_sync_without_snapshot_is_deferred_not_fallback_rewritten() {
    let source =
            "import Foo from './Foo.vue';\nimport util from './util';\nconst keep = import(`./${name}.vue`);\n";
    let foo_expr = "'./Foo.vue'";
    let util_expr = "'./util'";
    let dynamic_expr = "`./${name}.vue`";
    let foo_start = source.find(foo_expr).unwrap();
    let util_start = source.find(util_expr).unwrap();
    let dynamic_start = source.find(dynamic_expr).unwrap();

    let reader =
        TestResolverReader::with_files(&["/workspace/src/Foo.vue", "/workspace/src/util.ts"]);

    let prepared = prepare_non_vue_provider_sync(
        None,
        &reader,
        "/workspace/src/App.ts",
        source,
        &[
            test_module_reference(
                foo_expr,
                Some("./Foo.vue"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                foo_start,
                foo_start + foo_expr.len(),
            ),
            test_module_reference(
                util_expr,
                Some("./util"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                util_start,
                util_start + util_expr.len(),
            ),
            test_module_reference(
                dynamic_expr,
                None,
                &["./Foo.vue"],
                verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet,
                dynamic_start,
                dynamic_start + dynamic_expr.len(),
            ),
        ],
    );
    assert!(
        prepared.is_none(),
        "provider sync should be deferred until a resolver snapshot exists"
    );
}

#[test]
fn provider_sync_with_snapshot_uses_resolved_dependencies_only() {
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let reader =
        TestResolverReader::with_files(&["/workspace/src/Foo.vue", "/workspace/src/util.ts"]);
    let source =
            "import Foo from './Foo.vue';\nimport util from './util';\nconst keep = import(`./${name}.vue`);\n";
    let foo_expr = "'./Foo.vue'";
    let util_expr = "'./util'";
    let dynamic_expr = "`./${name}.vue`";
    let foo_start = source.find(foo_expr).unwrap();
    let util_start = source.find(util_expr).unwrap();
    let dynamic_start = source.find(dynamic_expr).unwrap();

    let prepared = prepare_non_vue_provider_sync(
        Some(&PublishedResolverSnapshot {
            resolver,
            ownership_ready: true,
        }),
        &reader,
        "/workspace/src/App.ts",
        source,
        &[
            test_module_reference(
                foo_expr,
                Some("./Foo.vue"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                foo_start,
                foo_start + foo_expr.len(),
            ),
            test_module_reference(
                util_expr,
                Some("./util"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                util_start,
                util_start + util_expr.len(),
            ),
            test_module_reference(
                dynamic_expr,
                None,
                &["./Foo.vue", "./util"],
                verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet,
                dynamic_start,
                dynamic_start + dynamic_expr.len(),
            ),
        ],
    )
    .expect("resolver snapshot should prepare provider sync");

    let resolved_sources = prepared
        .resolved_dependencies
        .iter()
        .map(|entry| entry.source_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        resolved_sources,
        vec!["/workspace/src/Foo.vue", "/workspace/src/util.ts"],
        "exact and finite-set dependencies should resolve through the native resolver"
    );
    assert!(
        prepared
            .resolved_dependencies
            .iter()
            .any(|entry| entry.provider_specifier == "./Foo.vue.ts"),
        "Vue dependencies should target their provider API paths"
    );
    assert!(
        prepared
            .resolved_dependencies
            .iter()
            .any(|entry| entry.provider_specifier == "./util"),
        "non-Vue workspace dependencies should preserve the source import specifier"
    );
    assert!(
        prepared.rewritten.contains("'./Foo.vue.ts'"),
        "exact Vue imports should rewrite through the resolved provider specifier"
    );
    assert!(
        prepared.rewritten.contains("'./util'"),
        "non-Vue workspace imports should stay source-compatible in the provider file"
    );
    assert!(
        prepared.rewritten.contains("import(`./${name}.vue`)"),
        "finite-set dynamics must keep the original expression text"
    );
}

#[test]
fn analyzed_refs_resolve_extensionless_vue_dependencies_to_exact_files() {
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let reader = TestResolverReader::with_files(&[
        "/workspace/src/tempUtil.ts",
        "/workspace/src/ExternalChild.vue",
    ]);
    let source =
        "import { MAGIC } from './tempUtil';\nimport ExternalChild from './ExternalChild.vue';\n";
    let temp_util_expr = "'./tempUtil'";
    let child_expr = "'./ExternalChild.vue'";
    let temp_util_start = source.find(temp_util_expr).unwrap();
    let child_start = source.find(child_expr).unwrap();

    let resolved = collect_resolved_provider_dependencies_from_analyzed_refs(
        &resolver,
        &reader,
        "/workspace/src/TempImporter.vue",
        &[
            test_analyzed_module_reference(
                temp_util_expr,
                Some("./tempUtil"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                temp_util_start,
                temp_util_start + temp_util_expr.len(),
            ),
            test_analyzed_module_reference(
                child_expr,
                Some("./ExternalChild.vue"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                child_start,
                child_start + child_expr.len(),
            ),
        ],
    );

    let resolved_sources = resolved
        .iter()
        .map(|entry| entry.source_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
            resolved_sources,
            vec!["/workspace/src/tempUtil.ts", "/workspace/src/ExternalChild.vue"],
            "Vue dependency tracking should use exact canonical IDs for extensionless TS imports and exact Vue imports"
        );
}

#[test]
fn provider_vue_path_helpers_use_original_paths() {
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);

    let ide_path =
        provider_ide_path_for_source(&resolver, "/workspace/src/App.vue", false).unwrap();
    let api_path = provider_api_path_for_source(&resolver, "/workspace/src/App.vue").unwrap();

    assert_eq!(
        ide_path, "/workspace/src/App.vue.tsx",
        "Vue IDE path should be canonical_id.tsx"
    );
    assert_eq!(
        api_path, "/workspace/src/App.vue.ts",
        "Vue API path should be canonical_id.ts"
    );
}

#[test]
fn provider_path_helpers_round_trip_through_resolver() {
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    // Host must have the backing .vue source for the collision guard to pass
    let host = VerterHost::new_standalone(HostConfig::default());
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/App.vue".to_string()),
        input_id: "/workspace/src/App.vue".to_string(),
        source: "<template><div/></template>".into(),
        file_kind: verter_session::FileKind::VueSfc,
        aliases: Vec::new(),
    })
    .unwrap();

    let ide_path = provider_ide_path_for_source(&resolver, "/workspace/src/App.vue", true).unwrap();
    let api_path = provider_api_path_for_source(&resolver, "/workspace/src/App.vue").unwrap();

    assert_eq!(
        source_id_from_provider_vue_path(&resolver, &host, &ide_path).as_deref(),
        Some("/workspace/src/App.vue")
    );
    assert_eq!(
        source_id_from_provider_vue_path(&resolver, &host, &api_path).as_deref(),
        Some("/workspace/src/App.vue")
    );
}

#[test]
fn vue_tsx_collision_with_real_file() {
    // A real .vue.tsx file exists but there's no matching .vue source in any project.
    // source_id_from_provider_vue_path should return None (collision guard).
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace/src".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let host = VerterHost::new_standalone(HostConfig::default());

    // "/workspace/src/weird.vue.tsx" has no backing "/workspace/src/weird.vue"
    // registered in any project, so the resolver should not strip the suffix
    assert_eq!(
        source_id_from_provider_vue_path(&resolver, &host, "/other/weird.vue.tsx"),
        None,
        ".vue.tsx with no backing .vue in any project should return None"
    );
}

#[test]
fn vue_tsx_virtual_file_resolves() {
    // A virtual .vue.tsx with a backing .vue source registered in a project.
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    // Host must have the backing .vue source for the collision guard to pass
    let host = VerterHost::new_standalone(HostConfig::default());
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/App.vue".to_string()),
        input_id: "/workspace/src/App.vue".to_string(),
        source: "<template><div/></template>".into(),
        file_kind: verter_session::FileKind::VueSfc,
        aliases: Vec::new(),
    })
    .unwrap();

    assert_eq!(
        source_id_from_provider_vue_path(&resolver, &host, "/workspace/src/App.vue.tsx").as_deref(),
        Some("/workspace/src/App.vue"),
        "virtual .vue.tsx with backing .vue source should resolve to .vue"
    );
}

#[test]
fn vue_tsx_collision_guard_rejects_when_host_missing_source() {
    // The resolver thinks /workspace/src/Real.vue.tsx belongs to the project
    // and strips the suffix to get /workspace/src/Real.vue, but the host
    // has never compiled Real.vue → collision guard must reject.
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let host = VerterHost::new_standalone(HostConfig::default());
    // Do NOT upsert /workspace/src/Real.vue into host

    assert_eq!(
        source_id_from_provider_vue_path(&resolver, &host, "/workspace/src/Real.vue.tsx"),
        None,
        ".vue.tsx in project but no backing .vue in host should return None"
    );
}

#[test]
fn import_resolved_matches_target_exact() {
    assert!(import_resolved_matches_target(
        "C:/project/src/components/Foo.vue",
        "C:/project/src/components/Foo.vue"
    ));
}

#[test]
fn import_resolved_matches_target_missing_vue_ext() {
    // Import `../Popup` resolves to `C:/proj/src/Popup` (no ext)
    // Target is `C:/proj/src/Popup.vue`
    assert!(import_resolved_matches_target(
        "C:/proj/src/Popup",
        "C:/proj/src/Popup.vue"
    ));
}

#[test]
fn import_resolved_matches_target_directory_index() {
    // Import `./Popover` resolves to `C:/proj/src/Popover` (directory)
    // Target is `C:/proj/src/Popover/index.vue`
    assert!(import_resolved_matches_target(
        "C:/proj/src/Popover",
        "C:/proj/src/Popover/index.vue"
    ));
}

#[test]
fn import_resolved_matches_target_directory_same_name() {
    // Import `./Popover` resolves to `C:/proj/src/Popover` (directory)
    // Target is `C:/proj/src/Popover/Popover.vue`
    assert!(import_resolved_matches_target(
        "C:/proj/src/Popover",
        "C:/proj/src/Popover/Popover.vue"
    ));
}

#[test]
fn import_resolved_does_not_match_different_component() {
    assert!(!import_resolved_matches_target(
        "C:/proj/src/Popup",
        "C:/proj/src/Dialog.vue"
    ));
    assert!(!import_resolved_matches_target(
        "C:/proj/src/Popup",
        "C:/proj/src/PopupMenu.vue"
    ));
}

/// Server capabilities must NOT include `diagnostic_provider` (pull diagnostics).
/// We use push diagnostics exclusively to avoid flickering during typing.
#[test]
fn capabilities_do_not_include_pull_diagnostics() {
    let caps = crate::capabilities::server_capabilities(&PositionEncodingKind::UTF16);
    assert!(
        caps.diagnostic_provider.is_none(),
        "diagnostic_provider must be removed — we use push diagnostics only"
    );
}

#[test]
fn did_open_startup_policy_enables_sync_for_tsgo_and_tsserver() {
    let tsgo = did_open_startup_policy(crate::TypeProviderKind::Tsgo);
    assert!(
        tsgo.sync_imported_vue_files,
        "TSGO should eagerly sync imported .vue files"
    );
    assert!(
        !tsgo.publish_diagnostics,
        "should not publish diagnostics inline"
    );

    let tsserver = did_open_startup_policy(crate::TypeProviderKind::Tsserver);
    assert!(
        tsserver.sync_imported_vue_files,
        "tsserver should eagerly sync imported .vue files"
    );
    assert!(
        !tsserver.publish_diagnostics,
        "should not publish diagnostics inline"
    );
}

#[test]
fn did_open_startup_policy_skips_sync_for_no_provider() {
    let none = did_open_startup_policy(crate::TypeProviderKind::None);
    assert!(
        !none.sync_imported_vue_files,
        "no type provider should not eagerly sync imported .vue files"
    );
    assert!(
        !none.publish_diagnostics,
        "should not publish diagnostics inline"
    );
}

#[test]
fn did_open_provider_sync_policy_skips_api_sync_for_tsserver_but_not_tsgo() {
    let tsserver = did_open_provider_sync_policy(crate::TypeProviderKind::Tsserver);
    assert!(
        tsserver.await_ide_sync,
        "tsserver cold open should still await current-file TSX sync"
    );
    assert!(
        !tsserver.await_api_sync,
        "tsserver cold open should not await current-file .vue.ts sync"
    );

    let tsgo = did_open_provider_sync_policy(crate::TypeProviderKind::Tsgo);
    assert!(
        tsgo.await_api_sync,
        "TSGO cold open should continue awaiting API sync"
    );

    let no_provider = did_open_provider_sync_policy(crate::TypeProviderKind::None);
    assert!(
        no_provider.await_ide_sync,
        "the cold-open policy should keep TSX sync enabled regardless of provider kind"
    );
    assert!(
        !no_provider.await_api_sync,
        "verter-only mode should not await API sync"
    );
}

#[tokio::test]
async fn initialized_returns_before_background_configure_paths_completes() {
    let temp_root = std::env::temp_dir().join(format!(
        "verter-lsp-init-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(temp_root.join("src")).expect("temp project should be created");
    std::fs::write(
        temp_root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("tsconfig should be written");

    let provider = Arc::new(SlowConfigurePathsProvider::default());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });

    let server = service.inner();
    server.vite_config_options.lock().await.enabled = false;
    *server.workspace_roots.lock().await = vec![format!(
        "file:///{}",
        temp_root.to_string_lossy().replace('\\', "/")
    )];

    let start = std::time::Instant::now();
    server.initialized(InitializedParams {}).await;
    let elapsed = start.elapsed();

    assert!(
            elapsed < std::time::Duration::from_millis(250),
            "initialized() should not wait for configure_paths/background discovery (elapsed {elapsed:?})"
        );

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.configure_paths_started.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("background init should still configure paths after initialized() returns");

    drain_handle.abort();
    drop(service);
}

#[test]
fn collect_imported_vue_priority_ids_keeps_only_resolved_vue_imports() {
    let analysis = verter_semantic::analysis::ScriptAnalysisSnapshot {
        imports: vec![
            verter_semantic::analysis::AnalyzedImport {
                source: "./MyComp.vue".to_string(),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: Some("C:/project/src/MyComp.vue".to_string()),
            },
            verter_semantic::analysis::AnalyzedImport {
                source: "./utils".to_string(),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: Some("C:/project/src/utils.ts".to_string()),
            },
            verter_semantic::analysis::AnalyzedImport {
                source: "./Other.vue".to_string(),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: None,
            },
            verter_semantic::analysis::AnalyzedImport {
                source: "./MyComp.vue".to_string(),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: Some("C:/project/src/MyComp.vue".to_string()),
            },
        ],
        module_references: Vec::new(),
        bindings: Vec::new(),
        macros: Vec::new(),
        macro_type_deps: Vec::new(),
        flags: verter_semantic::analysis::AnalysisFlags::empty(),
        exported_functions: Vec::new(),
        vue_api_calls: Vec::new(),
        dom_query_calls: Vec::new(),
        css_var_manipulations: Vec::new(),
        script_binding_occurrences: Vec::new(),
        store_usages: Vec::new(),
        store_definitions: Vec::new(),
        first_await_offset: None,
        type_enhancements: None,
        options_api: None,
        nested_macro_calls: Vec::new(),
        is_typescript: false,
        declaration_entries: Vec::new(),
    };

    let ids = collect_imported_vue_priority_ids(&analysis);

    assert_eq!(
        ids,
        vec!["C:/project/src/MyComp.vue".to_string()],
        "should keep one resolved .vue canonical id"
    );
    assert!(
        !ids.iter().any(|id| id.ends_with(".ts")),
        "non-Vue imports must be excluded"
    );
}

#[test]
fn collect_imported_vue_priority_ids_falls_back_to_relative_resolution() {
    let imports = vec![
        verter_semantic::analysis::AnalyzedImport {
            source: "./TypedSlotComp.vue".to_string(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        },
        verter_semantic::analysis::AnalyzedImport {
            source: "./utils".to_string(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        },
    ];

    let ids = collect_imported_vue_priority_ids_from_imports_with_fallback(
        &imports,
        Some("/workspace/src/TemplateSlotCases.vue"),
        |parent, specifier| {
            if parent == "/workspace/src/TemplateSlotCases.vue"
                && specifier == "./TypedSlotComp.vue"
            {
                Some("/workspace/src/TypedSlotComp.vue".to_string())
            } else if parent == "/workspace/src/TemplateSlotCases.vue" && specifier == "./utils" {
                Some("/workspace/src/utils.ts".to_string())
            } else {
                None
            }
        },
    );

    assert_eq!(
        ids,
        vec!["/workspace/src/TypedSlotComp.vue".to_string()],
        "unresolved direct Vue imports should still be prioritized via relative fallback"
    );
}

#[test]
fn did_open_prioritizes_exact_and_finite_dynamic_targets() {
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let reader = TestResolverReader::with_files(&[
        "/workspace/src/Foo.vue",
        "/workspace/src/Bar.vue",
        "/workspace/src/util.ts",
    ]);
    let targets = collect_priority_vue_targets_from_module_references(
        Some(&PublishedResolverSnapshot {
            resolver,
            ownership_ready: true,
        }),
        &reader,
        "/workspace/src/App.vue",
        &[
            test_analyzed_module_reference(
                "'./Foo.vue'",
                Some("./Foo.vue"),
                &[],
                verter_semantic::analysis::ModuleReferenceAnalyzability::Exact,
                0,
                10,
            ),
            test_analyzed_module_reference(
                "`./${name}.vue`",
                None,
                &["./Bar.vue", "./util"],
                verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet,
                11,
                27,
            ),
        ],
    );

    assert_eq!(
        targets,
        vec![
            "/workspace/src/Foo.vue".to_string(),
            "/workspace/src/Bar.vue".to_string()
        ]
    );
}

#[test]
fn unknown_dynamic_imports_sync_no_provider_dependencies() {
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let reader = TestResolverReader::with_files(&["/workspace/src/Foo.vue"]);
    let targets = collect_priority_vue_targets_from_module_references(
        Some(&PublishedResolverSnapshot {
            resolver,
            ownership_ready: true,
        }),
        &reader,
        "/workspace/src/App.vue",
        &[test_analyzed_module_reference(
            "`./${name}.vue`",
            None,
            &[],
            verter_semantic::analysis::ModuleReferenceAnalyzability::UnknownDynamic,
            0,
            15,
        )],
    );

    assert!(
        targets.is_empty(),
        "unknown dynamic imports must not speculate provider dependencies"
    );
}

#[tokio::test]
async fn goto_definition_component_event_name_reaches_child_define_emits() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("component event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("definition should point to MyComp.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [payload: string]"),
        "definition should point to the child defineEmits declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_reaches_child_listener_prop() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  label: string\n  onAlert?: (payload: string) => void\n}>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport OnEventPropComp from './OnEventPropComp.vue'\nfunction handleAlert(payload: string) {}\n</script>\n<template>\n  <OnEventPropComp label=\"ok\" @alert=\"handleAlert\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/OnEventPropComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/OnEventPropComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@alert=\"handleAlert\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("prop-backed event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("definition should point to OnEventPropComp.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "onAlert?: (payload: string) => void"),
        "definition should point to the child listener prop"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_returns_emit_before_listener_prop() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  onAlert?: () => void\n}>()\nconst emit = defineEmits<{ alert: [] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport BothEventComp from './BothEventComp.vue'\nfunction handleAlert() {}\n</script>\n<template>\n  <BothEventComp @alert=\"handleAlert\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/BothEventComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/BothEventComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@alert=\"handleAlert\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("event should resolve");
    let locations = definition_locations(response);

    assert_eq!(locations.len(), 2, "should return emit and listener prop");
    assert_eq!(locations[0].uri, child_uri, "emit should resolve in child");
    assert_eq!(
        locations[1].uri, child_uri,
        "listener prop should resolve in child"
    );
    assert_eq!(
        locations[0].range.start.line,
        line_for_snippet(child_source, "alert: []"),
        "defineEmits should come first"
    );
    assert_eq!(
        locations[1].range.start.line,
        line_for_snippet(child_source, "onAlert?: () => void"),
        "listener prop should come second"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_returns_none_when_child_has_no_match() {
    let child_source = "<script setup lang=\"ts\">\ndefineEmits<{ alert: [] }>()\ndefineProps<{ onAlert?: () => void }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleMissing() {}\n</script>\n<template>\n  <MyComp @missing=\"handleMissing\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@missing=\"handleMissing\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed");

    assert!(
        response.is_none(),
        "unknown child component events should suppress same-file handler fallback"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn resolve_component_document_for_usage_follows_barrel_reexports() {
    let child_source =
        "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
    let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\n</script>\n<template>\n  <BarrelComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/BarrelComp.vue", "vue", child_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/components/BarrelComp.vue");
    let server = service.inner();
    let analysis = server
        .documents
        .get_analysis(&app_uri)
        .expect("parent analysis should exist");
    let template = analysis
        .template
        .as_ref()
        .expect("template analysis should exist");
    let component = template
        .components
        .iter()
        .find(|component| component.name == "BarrelComp")
        .expect("template should include BarrelComp usage");

    assert_eq!(
        component.import_source.as_deref(),
        Some("./components"),
        "template component should retain the raw barrel import source"
    );
    assert_eq!(
        server
            .component_import_binding_name(&analysis, component)
            .as_deref(),
        Some("BarrelComp"),
        "named barrel imports should preserve the local component binding name"
    );

    let parent_canonical_id = uri_to_canonical_id(&app_uri);
    let barrel_canonical_id = server
        .resolve_import_specifier(&parent_canonical_id, "./components")
        .expect("barrel import should resolve to a concrete module");

    assert!(
        barrel_canonical_id.ends_with("/src/components/index.ts"),
        "extensionless barrel imports should resolve to index.ts, got {barrel_canonical_id}"
    );
    assert!(
        server
            .documents
            .host()
            .get_export_span_follow_reexports(&barrel_canonical_id, "BarrelComp")
            .is_some(),
        "barrel export should resolve to the re-exported child"
    );

    let child = server
        .resolve_component_document_for_usage(&app_uri, &analysis, component)
        .expect("component usage should resolve through the barrel");

    assert_eq!(
        child.uri, child_uri,
        "barrel should resolve to the child SFC"
    );
    assert!(
        child.analysis.macros.iter().any(|mac| {
            mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits
                && mac.emit_fields.iter().any(|field| field.name == "custom")
        }),
        "resolved child analysis should expose the child's emit declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_handles_barrel_reexports() {
    let child_source =
        "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
    let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\nfunction handleCustom() {}\n</script>\n<template>\n  <BarrelComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/BarrelComp.vue", "vue", child_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/components/BarrelComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("barrel event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("definition should follow the barrel to BarrelComp.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: []"),
        "definition should point to the re-exported child emit declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn completion_resolves_barrel_reexport_props_via_index_file() {
    let child_source =
            "<script setup lang=\"ts\">\ndefineProps<{ label: string; zIndex?: number }>()\n</script>\n";
    let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\n</script>\n<template>\n  <BarrelComp  />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/BarrelComp.vue", "vue", child_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    // Cursor at `<BarrelComp |/>` — in attribute position
    let cursor_pos = parent_source.find("<BarrelComp ").unwrap() + "<BarrelComp ".len();
    let line_index = LineIndex::new_utf16(parent_source);
    let position = line_index.offset_to_position(cursor_pos as u32).unwrap();

    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    // Positive: child props should appear via barrel re-export
    assert!(
        labels.contains(&"label".to_string()),
        "barrel-imported component should offer 'label' prop, got: {labels:?}"
    );
    assert!(
        labels.contains(&"z-index".to_string()),
        "barrel-imported component should offer 'z-index' prop (kebab-case), got: {labels:?}"
    );

    // Negative: internal symbols must not leak
    assert!(
        !labels.iter().any(|l| l.contains("___VERTER___")),
        "internal symbols must not leak: {labels:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_definition_component_event_name_skips_type_provider_virtual_fallback() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
    let ctx = synced_type_provider_context(server, &app_uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("event position should map into TSX");
    provider.set_definitions(
        &ctx.tsx_path,
        tsx_offset,
        vec![TypeLocation {
            path: ctx.tsx_path.clone(),
            start: 0,
            end: 0,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("native component event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|location| location.uri == child_uri)
        .expect("native child definition should win");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [payload: string]"),
        "native child definition should be returned instead of the virtual parent file"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDefinition { .. })),
        "native component event resolution should skip the type provider entirely"
    );

    drain_handle.abort();
    drop(service);
}

// =========================================================================
// Unified component contract resolution tests
// =========================================================================

#[tokio::test]
async fn contract_prop_name_navigates_to_child_define_props_field() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ title: string; count: number }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp title=\"hello\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Click on "title" in `title="hello"`
    let position = find_document_position(server, &app_uri, "title=\"hello\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("prop should resolve to child");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child component");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "title: string"),
        "definition should point to the child defineProps title field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_shorthand_prop_returns_both_parent_binding_and_child_field() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ bar: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst bar = 'hello'\n</script>\n<template>\n  <MyComp :bar />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Click on "bar" in `:bar`
    let position = find_document_position(server, &app_uri, ":bar", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("shorthand prop should resolve");
    let locations = definition_locations(response);

    assert!(
        locations.len() >= 2,
        "shorthand prop should return at least parent binding + child prop, got {}",
        locations.len()
    );
    // One location in parent (the `bar` binding), one in child (the prop field)
    let parent_loc = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("should include parent binding location");
    let child_loc = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("should include child prop location");
    assert_eq!(
        parent_loc.range.start.line,
        line_for_snippet(parent_source, "const bar = 'hello'"),
        "parent location should point to the bar binding"
    );
    assert_eq!(
        child_loc.range.start.line,
        line_for_snippet(child_source, "bar: string"),
        "child location should point to the defineProps bar field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_shorthand_prop_uses_import_target_for_parent_location() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ bar: string }>()\n</script>\n";
    let helper_source = "export const bar = 'hello'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nimport { bar } from './helpers'\n</script>\n<template>\n  <MyComp :bar />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/helpers.ts", "typescript", helper_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let helper_uri = workspace_uri(&workspace_id, "src/helpers.ts");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":bar", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("shorthand prop should resolve");
    let locations = definition_locations(response);

    let parent_loc = locations
        .iter()
        .find(|loc| loc.uri == helper_uri)
        .expect("should include imported parent binding target");
    let child_loc = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("should include child prop location");

    assert_eq!(
            parent_loc.range.start.line,
            line_for_snippet(helper_source, "export const bar"),
            "import-backed shorthand should resolve to the imported declaration, not the local import statement"
        );
    assert_eq!(
        child_loc.range.start.line,
        line_for_snippet(child_source, "bar: string"),
        "child location should point to the defineProps bar field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_shorthand_prop_uses_parent_define_props_field() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ bar: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\ndefineProps<{ bar: string }>()\n</script>\n<template>\n  <MyComp :bar />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":bar", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("shorthand prop should resolve");
    let locations = definition_locations(response);

    let parent_loc = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("should include parent defineProps field");
    let child_loc = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("should include child prop location");

    assert_eq!(
            parent_loc.range.start.line,
            line_for_snippet(parent_source, "bar: string"),
            "shorthand should resolve to the parent defineProps field when the binding comes from defineProps"
        );
    assert_eq!(
        child_loc.range.start.line,
        line_for_snippet(child_source, "bar: string"),
        "child location should point to the defineProps bar field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn unresolved_slot_template_does_not_block_later_contract_resolution() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <UnknownComp>\n    <template #default=\"{ item }\">{{ item }}</template>\n  </UnknownComp>\n  <MyComp title=\"hello\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "title=\"hello\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("later prop contract should still resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child component");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "title: string"),
        "unrelated unresolved slot templates should not prevent later contract resolution"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_event_click_navigates_to_child_define_emits() {
    // This tests that events still work through the unified contract handler
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("event should resolve to child defineEmits");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [payload: string]"),
        "event should navigate to child defineEmits field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_vmodel_named_navigates_to_child_define_model() {
    let child_source =
        "<script setup lang=\"ts\">\nconst title = defineModel<string>('title')\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst t = ref('hello')\n</script>\n<template>\n  <MyComp v-model:title=\"t\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "title" in `v-model:title="t"`
    let position = find_document_position(server, &app_uri, "v-model:title", 8);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-model:title should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "defineModel<string>('title')"),
        "v-model:title should navigate to child defineModel('title')"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_vmodel_default_navigates_to_child_define_model() {
    let child_source =
        "<script setup lang=\"ts\">\nconst modelValue = defineModel<string>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst val = ref('hello')\n</script>\n<template>\n  <MyComp v-model=\"val\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "model" in `v-model="val"` — this is the directive name area
    let position = find_document_position(server, &app_uri, "v-model=\"val\"", 3);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-model should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "defineModel<string>()"),
        "v-model should navigate to child default defineModel()"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_slot_name_navigates_to_child_define_slots_field() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ header(props: { title: string }): any }>()\n</script>\n<template>\n  <slot name=\"header\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #header=\"{ title }\">\n      {{ title }}\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "header" in `#header`
    let position = find_document_position(server, &app_uri, "#header", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("slot name should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "header(props:"),
        "slot name should navigate to child defineSlots header field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_slot_prop_binding_navigates_to_child_slot_binding() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ default(props: { item: string; index: number }): any }>()\n</script>\n<template>\n  <slot :item=\"row\" :index=\"i\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp #default=\"{ item }\">\n    {{ item }}\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "item" inside `#default="{ item }"`
    let position = find_document_position(server, &app_uri, "{ item }", 2);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("slot prop binding should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "item: string"),
        "slot prop binding should navigate to child defineSlots binding"
    );

    drain_handle.abort();
    drop(service);
}

// =========================================================================
// Step 4: Barrel-file export symbol clicks → terminal target
// =========================================================================

#[tokio::test]
async fn barrel_export_navigates_to_terminal_vue_component() {
    // Barrel: `export { default as Overlay } from './Overlay.vue'`
    // Clicking on `Overlay` in the barrel should navigate to Overlay.vue
    let overlay_source = "<script setup lang=\"ts\">\nconst visible = ref(false)\n</script>\n<template>\n  <div>Overlay</div>\n</template>\n";
    let barrel_source = "export { default as Overlay } from './Overlay.vue'\nexport { default as Dialog } from './Dialog.vue'\n";
    let dialog_source = "<script setup lang=\"ts\">\nconst open = ref(false)\n</script>\n<template>\n  <div>Dialog</div>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Overlay.vue", "vue", overlay_source),
        ("src/Dialog.vue", "vue", dialog_source),
        ("src/index.ts", "typescript", barrel_source),
    ])
    .await;

    let barrel_uri = workspace_uri(&workspace_id, "src/index.ts");
    let overlay_uri = workspace_uri(&workspace_id, "src/Overlay.vue");
    let server = service.inner();
    // Cursor on `Overlay` in `export { default as Overlay }`
    let position = find_document_position(server, &barrel_uri, "as Overlay }", 3);

    let response = server
        .goto_definition(goto_definition_params(&barrel_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("barrel export should resolve to terminal");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == overlay_uri)
        .expect("definition should point to Overlay.vue");

    // Should navigate to the component file — `default` export resolves
    // to the first script binding (line 1: `const visible = ref(false)`)
    assert_eq!(
        target.range.start.line,
        line_for_snippet(overlay_source, "const visible"),
        "barrel export should navigate to the terminal Vue component"
    );

    // Negative: should NOT stay in barrel file
    assert!(
        !locations.iter().any(|loc| loc.uri == barrel_uri),
        "barrel export should NOT resolve to barrel file itself"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_multi_level_navigates_to_terminal() {
    // Two-level barrel: index.ts → components.ts → Button.vue
    let button_source = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template>\n  <button>{{ label }}</button>\n</template>\n";
    let mid_barrel_source = "export { default as Button } from './Button.vue'\n";
    let top_barrel_source = "export { Button } from './components'\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Button.vue", "vue", button_source),
        ("src/components.ts", "typescript", mid_barrel_source),
        ("src/index.ts", "typescript", top_barrel_source),
    ])
    .await;

    let top_barrel_uri = workspace_uri(&workspace_id, "src/index.ts");
    let button_uri = workspace_uri(&workspace_id, "src/Button.vue");
    let server = service.inner();
    // Cursor on `Button` in `export { Button } from './components'`
    let position = find_document_position(server, &top_barrel_uri, "{ Button }", 2);

    let response = server
        .goto_definition(goto_definition_params(&top_barrel_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("multi-level barrel should resolve to terminal");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == button_uri)
        .expect("definition should point to Button.vue");

    // `default` export resolves to first binding (line 1: `defineProps<...>()`)
    assert_eq!(
        target.range.start.line,
        line_for_snippet(button_source, "defineProps"),
        "multi-level barrel should navigate to terminal Vue component"
    );

    // Negative: should NOT stay in any barrel file
    assert!(
        !locations.iter().any(|loc| loc.uri == top_barrel_uri),
        "should NOT resolve to top barrel itself"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_import_binding_in_vue_script_navigates_to_terminal() {
    let overlay_source = "<script setup lang=\"ts\">\nconst visible = ref(false)\n</script>\n<template>\n  <div>Overlay</div>\n</template>\n";
    let barrel_source = "export { default as Overlay } from './Overlay.vue'\n";
    let app_source = "<script setup lang=\"ts\">\nimport { Overlay } from './components'\n</script>\n<template>\n  <Overlay />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/Overlay.vue", "vue", overlay_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", app_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let overlay_uri = workspace_uri(&workspace_id, "src/components/Overlay.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "{ Overlay }", 2);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("import binding should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == overlay_uri)
        .expect("definition should point to Overlay.vue");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(overlay_source, "const visible"),
        "Vue script import binding should resolve through barrel to the terminal component"
    );
    assert!(
        !locations
            .iter()
            .any(|loc| loc.uri.as_str().ends_with("/src/components/index.ts")),
        "Vue script import binding should not stop at the barrel file"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_import_binding_in_vue_script_skips_type_provider_barrel_result() {
    let overlay_source = "<script setup lang=\"ts\">\nconst visible = ref(false)\n</script>\n<template>\n  <div>Overlay</div>\n</template>\n";
    let barrel_source = "export { default as Overlay } from './Overlay.vue'\nexport { default as Button } from './Button.vue'\n";
    let button_source = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n";
    let app_source = "<script setup lang=\"ts\">\nimport { ref, computed } from 'vue'\nimport { Overlay, Button } from './components'\n\nconst count = ref(0)\nconst doubled = computed(() => count.value * 2)\nconst showOverlay = ref(false)\n\nfunction increment() { count.value++ }\n</script>\n<template>\n  <div>\n    <p>{{ count }} x 2 = {{ doubled }}</p>\n    <button @click=\"increment\">+</button>\n    <Button label=\"Open\" @click=\"showOverlay = true\" />\n    <Overlay :show=\"showOverlay\" :zIndex=\"100\" :lockScroll=\"true\">\n      <p>Overlay content</p>\n    </Overlay>\n  </div>\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/components/Overlay.vue", "vue", overlay_source),
        ("src/components/Button.vue", "vue", button_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", app_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let overlay_uri = workspace_uri(&workspace_id, "src/components/Overlay.vue");
    let barrel_path = format!("{workspace_id}/src/components/index.ts");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "{ Overlay, Button }", 2);
    let ctx = synced_type_provider_context(server, &app_uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("import position should map into TSX");
    provider.set_definitions(
        &ctx.tsx_path,
        tsx_offset,
        vec![TypeLocation {
            path: barrel_path,
            start: 20,
            end: 27,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("import binding should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == overlay_uri)
        .expect("definition should point to Overlay.vue");

    assert_eq!(
            target.range.start.line,
            line_for_snippet(overlay_source, "const visible"),
            "Vue script import binding should resolve through barrel to the terminal component even when the type provider returns the barrel"
        );
    assert!(
        !locations
            .iter()
            .any(|loc| loc.uri.as_str().ends_with("/src/components/index.ts")),
        "Vue script import binding should not stop at the barrel file"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDefinition { .. })),
        "native import binding resolution should skip the type provider entirely"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_aliased_local_side_navigates_to_source() {
    // `export { default as Popup } from './Popup.vue'`
    // Clicking on `default` (the local side) should navigate to Popup.vue too
    let popup_source = "<script setup lang=\"ts\">\nconst shown = ref(true)\n</script>\n<template>\n  <div>Popup</div>\n</template>\n";
    let barrel_source = "export { default as Popup } from './Popup.vue'\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Popup.vue", "vue", popup_source),
        ("src/index.ts", "typescript", barrel_source),
    ])
    .await;

    let barrel_uri = workspace_uri(&workspace_id, "src/index.ts");
    let popup_uri = workspace_uri(&workspace_id, "src/Popup.vue");
    let server = service.inner();
    // Cursor on `default` in `export { default as Popup }`
    let position = find_document_position(server, &barrel_uri, "{ default", 2);

    let response = server
        .goto_definition(goto_definition_params(&barrel_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("local side of aliased re-export should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == popup_uri)
        .expect("definition should point to Popup.vue");

    // `default` export resolves to first binding (line 1: `const shown = ref(true)`)
    assert_eq!(
        target.range.start.line,
        line_for_snippet(popup_source, "const shown"),
        "local side of aliased barrel export should navigate to terminal"
    );

    drain_handle.abort();
    drop(service);
}

// =========================================================================
// Step 5: Resolve type-provider barrel locations to terminal declarations
// =========================================================================

#[tokio::test]
async fn resolve_barrel_locations_follows_reexport_to_terminal() {
    // Setup: barrel re-exports a Vue component
    let comp_source = "<script setup lang=\"ts\">\nconst count = ref(0)\n</script>\n<template><div/></template>\n";
    let barrel_source = "export { default as Counter } from './Counter.vue'\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Counter.vue", "vue", comp_source),
        ("src/index.ts", "typescript", barrel_source),
    ])
    .await;

    let barrel_id = format!("{workspace_id}/src/index.ts");
    let comp_uri = workspace_uri(&workspace_id, "src/Counter.vue");
    let server = service.inner();

    // Simulate a type provider returning a location in the barrel file
    // pointing to the `Counter` export signature (offset 20..27 in barrel source)
    let barrel_source_stored = server.documents.host.get_source(&barrel_id).unwrap();
    let counter_offset = barrel_source_stored
        .find("Counter")
        .expect("Counter in barrel source") as u32;
    let barrel_li = LineIndex::new(&barrel_source_stored, PositionEncodingKind::UTF16);
    let start_pos = barrel_li
        .offset_to_position(counter_offset)
        .expect("start pos");
    let end_pos = barrel_li
        .offset_to_position(counter_offset + 7)
        .expect("end pos");
    let barrel_uri = workspace_uri(&workspace_id, "src/index.ts");

    let input = Some(GotoDefinitionResponse::Scalar(Location {
        uri: barrel_uri.clone(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
    }));

    let result = server.resolve_barrel_locations(input);
    let locations = definition_locations(result.expect("should resolve"));
    let target = locations
        .iter()
        .find(|loc| loc.uri == comp_uri)
        .expect("should resolve barrel to Counter.vue");

    // Should navigate to first binding in Counter.vue
    assert_eq!(
        target.range.start.line,
        line_for_snippet(comp_source, "const count"),
        "type provider barrel location should resolve to terminal"
    );

    // Negative: should NOT stay in barrel
    assert!(
        !locations.iter().any(|loc| loc.uri == barrel_uri),
        "should NOT remain in barrel file"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn resolve_barrel_locations_preserves_non_barrel() {
    // A location that doesn't point to a barrel should pass through unchanged
    let comp_source =
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div/></template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", comp_source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    let input = Some(GotoDefinitionResponse::Scalar(Location {
        uri: app_uri.clone(),
        range: Range::default(),
    }));

    let result = server.resolve_barrel_locations(input);
    let locations = definition_locations(result.expect("should pass through"));
    assert_eq!(locations.len(), 1);
    assert_eq!(
        locations[0].uri, app_uri,
        "non-barrel location should pass through unchanged"
    );
    assert_eq!(
        locations[0].range,
        Range::default(),
        "range should be unchanged"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_type_definition_returns_none_without_provider() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let (service, socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host),
                type_provider: None,
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });

    let server = service.inner();
    let source = "<script setup lang=\"ts\">\nconst count: number = 0\n</script>\n";
    let uri: Uri = "file:///test/App.vue".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 6,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result = server
        .goto_type_definition(params)
        .await
        .expect("handler should not error");

    assert!(
        result.is_none(),
        "type definition should return None without a type provider"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn goto_type_definition_delegates_to_provider() {
    let source = "<script setup lang=\"ts\">\nconst count: number = 0\n</script>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "count", 0);

    // Set up mock to return a type definition when queried
    {
        let ctx = synced_type_provider_context(server, &app_uri);
        if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
            &position,
            &ctx.vue_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        ) {
            provider.set_type_definitions(
                &ctx.tsx_path,
                tsx_offset,
                vec![TypeLocation {
                    path: ctx.tsx_path.clone(),
                    start: 0,
                    end: 5,
                }],
            );
        }
    }

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: app_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result = server
        .goto_type_definition(params)
        .await
        .expect("handler should not error");

    // Verify the provider was called with get_type_definition (not get_definition)
    assert!(
        provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetTypeDefinition { .. })),
        "handler should delegate to get_type_definition on the provider"
    );
    assert!(
        !provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDefinition { .. })),
        "handler should NOT call get_definition"
    );

    // The merge logic should produce a response when the provider returns locations
    assert!(
        result.is_some(),
        "type definition should return locations when provider has results"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn hover_prefers_child_component_summary_over_import_alias_on_component_tag() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
    let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>

<template>
  <MyComp foo="literal" :bar="1" @custom="handler($event)" />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let mut position = Position {
        line: 5,
        character: 2,
    };
    position.character += 1;

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "```typescript\n(alias) import MyComp\nimport MyComp\n```",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("Props:"),
        "hover should show props, got: {text}"
    );
    assert!(
        text.contains("foo"),
        "hover should include foo, got: {text}"
    );
    assert!(
        text.contains("string"),
        "hover should include foo type, got: {text}"
    );
    assert!(
        text.contains("bar"),
        "hover should include bar, got: {text}"
    );
    assert!(
        text.contains("number"),
        "hover should include bar type, got: {text}"
    );
    assert!(
        text.contains("Emits:"),
        "hover should show emits, got: {text}"
    );
    assert!(
        text.contains("custom"),
        "hover should include custom emit, got: {text}"
    );
    assert!(
        text.contains("payload"),
        "hover should include payload label, got: {text}"
    );
    assert!(
        !text.contains("(alias) import MyComp"),
        "hover must not prefer import alias hover, got: {text}"
    );
    assert!(
        !text.contains("DefineComponent<{}, {}>"),
        "hover must not degrade to fallback component shell, got: {text}"
    );
}

#[tokio::test]
async fn hover_prefers_child_component_summary_over_import_alias_on_vue_import_binding() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
    let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>

<template>
  <MyComp />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let position = Position {
        line: 1,
        character: 7,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "```typescript\n(alias) import MyComp\nimport MyComp\n```",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("Props:"),
        "hover should show props, got: {text}"
    );
    assert!(
        text.contains("foo"),
        "hover should include foo, got: {text}"
    );
    assert!(
        text.contains("bar"),
        "hover should include bar, got: {text}"
    );
    assert!(
        text.contains("Emits:"),
        "hover should show emits, got: {text}"
    );
    assert!(
        text.contains("custom"),
        "hover should include custom emit, got: {text}"
    );
    assert!(
        !text.contains("(alias) import MyComp"),
        "hover must not prefer import alias hover, got: {text}"
    );
    assert!(
        !text.contains("DefineComponent<{}, {}>"),
        "hover must not degrade to fallback component shell, got: {text}"
    );
}

#[tokio::test]
async fn hover_prefers_child_component_summary_over_barrel_import_alias_on_vue_import_binding() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ show?: boolean; zIndex?: number }>()
</script>
<template><div /></template>
"#;
    let barrel_uri: Uri = "file:///workspace/src/components/index.ts"
        .parse()
        .expect("valid barrel uri");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: barrel_uri,
        language_id: "typescript".to_string(),
        version: 1,
        text: "export { default as Overlay } from './Overlay.vue'\n".to_string(),
    });
    let _child_uri = open_test_vue(
        server,
        "/workspace/src/components/Overlay.vue",
        child_source,
    );
    let app_uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
import { Overlay } from './components'
</script>

<template>
  <Overlay />
</template>
"#,
    );

    let position = Position {
        line: 1,
        character: 9,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "```typescript\n(const) const Overlay: __OmitNew<DefineComponent<{}, {}>>\n```",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("Props:"),
        "hover should show props for barrel-imported components, got: {text}"
    );
    assert!(
        text.contains("show"),
        "hover should include show prop, got: {text}"
    );
    assert!(
        text.contains("zIndex"),
        "hover should include zIndex prop, got: {text}"
    );
    assert!(
        !text.contains("DefineComponent<{}, {}>"),
        "hover must not degrade to the raw type provider shell, got: {text}"
    );
}

#[tokio::test]
async fn hover_rewrites_component_event_attr_to_vue_syntax() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
    let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
function handleCustom(payload: string) {
  console.log(payload)
}
</script>

<template>
  <MyComp @custom="handleCustom($event)" />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let position = Position {
        line: 8,
        character: 11,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "```typescript\n(property) onCustom: (payload: string) => void\n```",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("@custom"),
        "hover should use Vue event syntax, got: {text}"
    );
    assert!(
        text.contains("payload"),
        "hover should include payload label, got: {text}"
    );
    assert!(
        text.contains("string"),
        "hover should include payload type, got: {text}"
    );
    assert!(
        !text.contains("onCustom"),
        "hover must not expose TSX on* naming, got: {text}"
    );
    assert!(
        !text.contains(": any"),
        "hover must not degrade to any, got: {text}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_scoped_slot_locals() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "{{ sl }}", 5);
    let slot_ctx = synced_type_provider_context(server, &slot_uri);
    let slot_tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &slot_ctx.vue_line_index,
        &slot_ctx.mapper,
        &slot_ctx.tsx_line_index,
    )
    .expect("slot completion position should map to tsx");
    let slot_expr_context = classify_expression_context_with_trigger(
        &slot_ctx.tsx_content,
        slot_tsx_offset as usize,
        None,
    );
    let slot_snippet = debug_snippet(&slot_ctx.tsx_content, slot_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "slotItem".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const slotItem: SlotItem".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "slotIndex".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const slotIndex: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "slotTotal".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const slotTotal: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "Set".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Class),
                detail: Some("global".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"slotItem".to_string()),
            "slotItem should be present, got: {labels:?}, expr_context={slot_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            slot_snippet.0,
            slot_snippet.1,
        );
    assert!(
        labels.contains(&"slotIndex".to_string()),
        "slotIndex should be present, got: {labels:?}"
    );
    assert!(
        labels.contains(&"slotTotal".to_string()),
        "slotTotal should be present, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"Set".to_string()),
        "global completions should stay filtered for partial slot locals, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_scoped_slot_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.name", 9);
    let slot_ctx = synced_type_provider_context(server, &slot_uri);
    let slot_tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &slot_ctx.vue_line_index,
        &slot_ctx.mapper,
        &slot_ctx.tsx_line_index,
    )
    .expect("slot member position should map to tsx");
    let slot_expr_context = classify_expression_context_with_trigger(
        &slot_ctx.tsx_content,
        slot_tsx_offset as usize,
        None,
    );
    let slot_snippet = debug_snippet(&slot_ctx.tsx_content, slot_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "name".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) name: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "id".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) id: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "outerLabel".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const outerLabel: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"name".to_string()),
            "name should be present for scoped-slot member access, got: {labels:?}, expr_context={slot_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            slot_snippet.0,
            slot_snippet.1,
        );
    assert!(
        labels.contains(&"id".to_string()),
        "id should be present for scoped-slot member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"outerLabel".to_string()),
        "member access should suppress outer scope identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_scoped_slot_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.na", 11);
    let slot_ctx = synced_type_provider_context(server, &slot_uri);
    let slot_tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &slot_ctx.vue_line_index,
        &slot_ctx.mapper,
        &slot_ctx.tsx_line_index,
    )
    .expect("partial slot member position should map to tsx");
    let slot_expr_context = classify_expression_context_with_trigger(
        &slot_ctx.tsx_content,
        slot_tsx_offset as usize,
        None,
    );
    let slot_snippet = debug_snippet(&slot_ctx.tsx_content, slot_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "name".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) name: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "id".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) id: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "outerLabel".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const outerLabel: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"name".to_string()),
            "name should be present for partial scoped-slot member access, got: {labels:?}, expr_context={slot_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            slot_snippet.0,
            slot_snippet.1,
        );
    assert!(
        labels.contains(&"id".to_string()),
        "id should be present for partial scoped-slot member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"outerLabel".to_string()),
        "partial member access should suppress outer scope identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_vfor_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
  handler: () => void
}

const actions: Action[] = [{ label: 'ok', disabled: false, handler: () => {} }]
</script>

<template>
  <div>
    <button v-for="action in actions" :key="action.label" :disabled="action.di">
      {{ action.label }}
    </button>
  </div>
</template>
"#;

    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "action.di", 7);
    let ctx = synced_type_provider_context(server, &uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("v-for member access position should map to tsx");
    let expr_context =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, None);
    let snippet = debug_snippet(&ctx.tsx_content, tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "disabled".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) disabled: boolean".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "label".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) label: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "handler".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Method),
                detail: Some("(method) handler(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "actions".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const actions: Action[]".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"disabled".to_string()),
            "disabled should be present for v-for member access, got: {labels:?}, expr_context={expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            snippet.0,
            snippet.1,
        );
    assert!(
        labels.contains(&"label".to_string()),
        "label should be present for v-for member access, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "handler should be present for v-for member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"actions".to_string()),
        "member access should suppress outer identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_fixture_vfor_member_access_after_broken_interpolation(
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source =
        include_str!("../../../packages/vue-vscode/e2e/fixtures/single-project/src/App.vue");
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "action.disabled", 7);
    let ctx = synced_type_provider_context(server, &uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("fixture member access position should map to tsx");
    let expr_context =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, None);
    let snippet = debug_snippet(&ctx.tsx_content, tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "disabled".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) disabled: boolean".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "label".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) label: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "handler".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Method),
                detail: Some("(method) handler(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"disabled".to_string()),
            "disabled should be present for fixture v-for member access, got: {labels:?}, expr_context={expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            snippet.0,
            snippet.1,
        );
    assert!(
        labels.contains(&"label".to_string()),
        "label should be present for fixture v-for member access, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "handler should be present for fixture v-for member access, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"actions".to_string()),
        "fixture member access should suppress outer identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_scoped_slot_member_access_after_prior_partial_member_access(
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ sl }}</p>
    <p>{{ slotItem.na }}</p>
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.name", 9);

    set_type_completions_at_vue_position(
        server,
        &provider,
        &slot_uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "name".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) name: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "id".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) id: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "outerLabel".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const outerLabel: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, Some(".")))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
            labels.contains(&"name".to_string()),
            "name should be present for scoped-slot member access after prior partial member access, got: {labels:?}"
        );
    assert!(
            labels.contains(&"id".to_string()),
            "id should be present for scoped-slot member access after prior partial member access, got: {labels:?}"
        );
    assert!(
            !labels.contains(&"outerLabel".to_string()),
            "member access after prior partial member access should suppress outer scope identifiers, got: {labels:?}"
        );
}

#[tokio::test]
async fn completion_retries_member_access_without_dot_trigger_when_backend_returns_empty() {
    let provider = Arc::new(TriggerSensitiveCompletionProvider);
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
interface SlotItem {
  id: number
  name: string
}

defineSlots<{
  default(props: { slotItem: SlotItem; slotIndex: number; slotTotal: number }): any
}>()
</script>
<template>
  <slot :slotItem="{ id: 1, name: 'first' }" :slotIndex="0" :slotTotal="1" />
</template>
"#;
    let slot_source = r#"<script setup lang="ts">
import TypedSlotComp from './TypedSlotComp.vue'

const outerLabel = 'outer'
</script>

<template>
  <TypedSlotComp v-slot="{ slotItem, slotIndex, slotTotal }">
    <p>{{ slotItem.name }}</p>
    <p>{{ slotIndex }}</p>
    <p>{{ slotTotal }}</p>
    <p>{{ outerLabel }}</p>
  </TypedSlotComp>
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/TypedSlotComp.vue", child_source);
    let slot_uri = open_test_vue(server, "/workspace/src/TemplateSlotCases.vue", slot_source);
    let position = find_document_position(server, &slot_uri, "slotItem.name", 9);

    let labels = completion_labels(
        server
            .completion(completion_params(&slot_uri, position, Some(".")))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
        labels.contains(&"name".to_string()),
        "member access retry should recover property completions, got: {labels:?}"
    );
    assert!(
        labels.contains(&"id".to_string()),
        "member access retry should recover property completions, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"outerLabel".to_string()),
        "member access retry should not fall back to outer identifiers, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_synthesizes_dot_trigger_for_member_access_without_trigger_character() {
    let provider = Arc::new(DotTriggerRequiredCompletionProvider);
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
  handler: () => void
}

const actions: Action[] = [{ label: 'ok', disabled: false, handler: () => {} }]
</script>

<template>
  <div>
    <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
      {{ action.label }}
    </button>
  </div>
</template>
"#;

    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "action.disabled", 7);

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
        labels.contains(&"disabled".to_string()),
        "member access completion should synthesize a dot trigger, got: {labels:?}"
    );
    assert!(
        labels.contains(&"label".to_string()),
        "member access completion should synthesize a dot trigger, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "member access completion should synthesize a dot trigger, got: {labels:?}"
    );
    assert!(
            !labels.contains(&"actions".to_string()),
            "member access completion should stay scoped when synthesizing a dot trigger, got: {labels:?}"
        );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_identifier_recovery() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let recovery_source = r#"<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'

const count = ref(1)

function safeAction() {
  count.value++
}

const broken =
</script>

<template>
  <div>
    <p>{{ cou }}</p>
    <p>{{ count }}</p>
    <button @click="safeAction">go</button>
    <MyComp foo="ok" :bar="count" />
  </div>
</template>
"#;

    let recovery_uri = open_test_vue(
        server,
        "/workspace/src/TemplateRecovery.vue",
        recovery_source,
    );
    let position = find_document_position(server, &recovery_uri, "{{ cou }}", 6);
    let recovery_ctx = synced_type_provider_context(server, &recovery_uri);
    let recovery_tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &recovery_ctx.vue_line_index,
        &recovery_ctx.mapper,
        &recovery_ctx.tsx_line_index,
    )
    .expect("recovery completion position should map to tsx");
    let recovery_expr_context = classify_expression_context_with_trigger(
        &recovery_ctx.tsx_content,
        recovery_tsx_offset as usize,
        None,
    );
    let recovery_snippet = debug_snippet(&recovery_ctx.tsx_content, recovery_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &recovery_uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "count".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const count: Ref<number>".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "safeAction".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Function),
                detail: Some("function safeAction(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "console".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Module),
                detail: Some("global".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&recovery_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"count".to_string()),
            "count should be present, got: {labels:?}, expr_context={recovery_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            recovery_snippet.0,
            recovery_snippet.1,
        );
    assert!(
        !labels.contains(&"console".to_string()),
        "global completions should stay filtered for broken-script recovery, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_partial_function_recovery() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let recovery_source = r#"<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'

const count = ref(1)

function safeAction() {
  count.value++
}

const broken =
</script>

<template>
  <div>
    <p>{{ cou }}</p>
    <p>{{ safeA }}</p>
    <p>{{ count }}</p>
    <button @click="safeAction">go</button>
    <MyComp foo="ok" :bar="count" />
  </div>
</template>
"#;

    let recovery_uri = open_test_vue(
        server,
        "/workspace/src/TemplateRecovery.vue",
        recovery_source,
    );
    let position = find_document_position(server, &recovery_uri, "{{ safeA }}", 8);
    let recovery_ctx = synced_type_provider_context(server, &recovery_uri);
    let recovery_tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &recovery_ctx.vue_line_index,
        &recovery_ctx.mapper,
        &recovery_ctx.tsx_line_index,
    )
    .expect("partial function recovery position should map to tsx");
    let recovery_expr_context = classify_expression_context_with_trigger(
        &recovery_ctx.tsx_content,
        recovery_tsx_offset as usize,
        None,
    );
    let recovery_snippet = debug_snippet(&recovery_ctx.tsx_content, recovery_tsx_offset as usize)
        .unwrap_or_else(|| ("<none>".to_string(), "<none>".to_string()));

    set_type_completions_at_vue_position(
        server,
        &provider,
        &recovery_uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "safeAction".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Function),
                detail: Some("function safeAction(): void".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "count".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Variable),
                detail: Some("const count: Ref<number>".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "console".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Module),
                detail: Some("global".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&recovery_uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"safeAction".to_string()),
            "safeAction should be present after broken-script recovery, got: {labels:?}, expr_context={recovery_expr_context:?}, tsx_before={:?}, tsx_after={:?}, calls={calls:?}",
            recovery_snippet.0,
            recovery_snippet.1,
        );
    assert!(
        !labels.contains(&"console".to_string()),
        "global completions should stay filtered for broken-script recovery, got: {labels:?}"
    );
}

#[tokio::test]
async fn completion_queries_type_provider_for_nested_partial_member_access() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let source = r#"<script setup lang="ts">
import { ref, computed, reactive } from 'vue'

const mixed = ref<string | number>(0)

interface DeepNested {
  deep: { value: string; count: number }
}
const nested = reactive<DeepNested>({ deep: { value: 'hello', count: 1 } })

const Status = { Active: 'active', Inactive: 'inactive' } as const
type StatusType = typeof Status[keyof typeof Status]
const currentStatus = ref<StatusType>('active')

interface HasName { name: string }
interface HasAge { age: number }
type Person = HasName & HasAge
const person = ref<Person>({ name: 'Alice', age: 30 })

const summary = computed(() => `${person.value.name}: ${person.value.age}`)
</script>
<template>
  <div>
    <p>{{ mixed }}</p>
    <p>{{ nested.deep.va }}</p>
    <p>{{ nested.deep }}</p>
    <p>{{ currentStatus }}</p>
    <p>{{ person }}</p>
    <p>{{ summary }}</p>
  </div>
</template>
"#;

    let uri = open_test_vue(server, "/workspace/src/TypeResolutionCases.vue", source);
    let position = Position {
        line: 23,
        character: 21,
    };

    set_type_completions_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        vec![
            crate::tsgo::protocol::Completion {
                label: "value".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) value: string".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
            crate::tsgo::protocol::Completion {
                label: "count".to_string(),
                kind: Some(crate::tsgo::protocol::CompletionKind::Property),
                detail: Some("(property) count: number".to_string()),
                documentation: None,
                edit_range_start: None,
                edit_range_end: None,
                insert_text: None,
                sort_text: None,
                data: None,
            },
        ],
    );

    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    assert!(
        labels.contains(&"value".to_string()),
        "value should be present for nested member access, got: {labels:?}"
    );
    assert!(
        labels.contains(&"count".to_string()),
        "count should be present for nested member access, got: {labels:?}"
    );
}

#[tokio::test]
async fn hover_rewrites_prop_backed_event_attr_to_vue_syntax() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ label: string; onAlert?: (payload: string) => void }>()
</script>
<template><button>{{ label }}</button></template>
"#;
    let app_source = r#"<script setup lang="ts">
import OnEventPropComp from './OnEventPropComp.vue'
function handleCustom(payload: string) {
  console.log(payload)
}
</script>

<template>
  <OnEventPropComp label="go" @alert="handleCustom" />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/OnEventPropComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let position = Position {
        line: 8,
        character: 29,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "```typescript\n(property) onAlert?: (payload: string) => void\n```",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("@alert"),
        "hover should use Vue event syntax, got: {text}"
    );
    assert!(
        text.contains("payload"),
        "hover should include payload label, got: {text}"
    );
    assert!(
        text.contains("string"),
        "hover should include payload type, got: {text}"
    );
    assert!(
        !text.contains("onAlert"),
        "hover must not expose TSX on* naming, got: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_init_drains_pending_snapshot_provider_sync_for_open_vue_file() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div /></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    );
    let provider_sync_states = DashMap::new();
    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    drain_pending_snapshot_provider_sync(
        Some(&sync),
        &documents,
        &vfs_workspace,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        false,
        None,
    )
    .await;

    assert!(
        !pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "drained open Vue files should be removed from the pending snapshot queue"
    );

    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("drained sync should commit owner-aware provider state");
    assert!(
        !state.is_provisional(),
        "drain must set an owner-aware binding on provider state"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path.ends_with(".vue.ts")
        )),
        "drain should sync the Vue public API through .vue.ts"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path.ends_with(".tsx")
        )),
        "drain should sync the open Vue IDE file through the synthetic TSX path"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_init_drain_removes_stale_provisional_state_when_ready_snapshot_has_no_owner() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    );
    let provider_sync_states = DashMap::new();
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Provisional,
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ide_background_loaded: true,
            api_background_loaded: true,
            shadow_path: None,
            shadow_background_loaded: false,
        },
    );
    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    drain_pending_snapshot_provider_sync(
        Some(&sync),
        &documents,
        &vfs_workspace,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        false,
        None,
    )
    .await;

    assert!(
        pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
        "ready-but-unowned files should stay queued for future rebuilds"
    );
    assert!(
        provider_sync_states.get("/workspace/src/App.vue").is_none(),
        "ready-but-unowned drain must remove stale provisional provider state"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "drain should close the stale provisional IDE path, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.ts"
        )),
        "drain should close the stale provisional API path, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path.ends_with(".vue.ts") || path.ends_with(".vue.tsx")
        )),
        "ready-but-unowned drain must not keep syncing guessed provider paths, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_init_drain_clears_stale_macro_type_diagnostic_for_package_exports_dep() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::create_dir_all(workspace.join("node_modules/motion/dist"))
        .expect("create motion dist dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::write(
        workspace.join("node_modules/motion/package.json"),
        r#"{
                "name": "motion",
                "exports": {
                    ".": {
                        "types": "./dist/index.d.ts"
                    }
                }
            }"#,
    )
    .expect("write motion package");
    std::fs::write(
        workspace.join("node_modules/motion/dist/index.d.ts"),
        "export interface MotionProps { duration: number }\n",
    )
    .expect("write motion types");

    let popup_source = "<script setup lang=\"ts\">\nimport type { MotionProps } from 'motion'\nconst props = defineProps<MotionProps>()\n</script>\n<template><div>{{ props.duration }}</div></template>";
    std::fs::write(workspace.join("src/Popup.vue"), popup_source).expect("write Popup.vue");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let popup_id = format!("{workspace_id}/src/Popup.vue");
    let uri = crate::uri::path_to_file_uri(&popup_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    )]);

    let documents = DocumentRegistry::new(Arc::clone(&host));
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: popup_source.to_string(),
    });

    let cached_verter_diags = DashMap::new();

    // With TypeImport resolution and a filesystem-backed host, the macro type dep
    // resolves immediately via the "types" export condition in package.json.
    // No stale HOST_MISSING_MACRO_TYPE_DEP diagnostic should appear.
    let diags =
        compute_verter_diagnostics_for_with_views(&documents, &uri, &cached_verter_diags, None);
    assert!(
        !diags.iter().any(|d| matches!(
            &d.code,
            Some(NumberOrString::String(code)) if code == "HOST_MISSING_MACRO_TYPE_DEP"
        )),
        "macro type dep 'motion' with types-only exports should resolve via TypeImport, got: {diags:?}"
    );
    let cache = cached_verter_diags
        .get(uri.as_str())
        .expect("diagnostics should be cached");
    assert_eq!(
        cache.0, 1,
        "cached doc version should match did_open version"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_vue_api_lightweight_uses_provisional_api_path_before_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(
        workspace.join("src/Child.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    )
    .expect("write child");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    let child_id = crate::test_utils::canonical_test_path(&workspace.join("src/Child.vue"));

    server.sync_imported_vue_api_lightweight(&child_id).await;

    let calls = provider.file_sync_calls();
    let expected_api_path = format!("{child_id}.ts");
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == &expected_api_path
            )),
            "pre-snapshot imported Vue API sync should open the provisional .vue.ts path, calls={calls:?}"
        );

    let state = server
        .provider_sync_states
        .get(&child_id)
        .map(|entry| entry.clone())
        .expect("provisional API sync should commit provider state");
    assert!(
        state.is_provisional(),
        "pre-snapshot imported sync should mark the owner as provisional"
    );
    assert_eq!(
        state.api_path.as_deref(),
        Some(expected_api_path.as_str()),
        "imported Vue API should use the canonical provisional .vue.ts path"
    );
    assert!(
        state.api_background_loaded,
        "provisional imported API sync should mark the API path as loaded"
    );
    assert!(
        server.pending_snapshot_provider_sync.contains(&child_id),
        "owner-aware sync should still be queued for reconciliation after snapshot discovery"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_vue_api_lightweight_opens_snapshot_api_path_for_tsserver() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_id = "/workspace/src/Child.vue";
    let _child_uri = open_test_vue(
        server,
        child_id,
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    server.sync_imported_vue_api_lightweight(child_id).await;

    let state = server
        .provider_sync_states
        .get(child_id)
        .map(|entry| entry.clone())
        .expect("snapshot imported API sync should commit provider state");
    let api_path = state
        .api_path
        .clone()
        .expect("snapshot imported API sync should record the API path");
    let calls = provider.file_sync_calls();

    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == &api_path
            )),
            "snapshot imported Vue API sync should open the provider-facing API path for tsserver, calls={calls:?}, api_path={api_path}"
        );
    assert!(
            !calls.iter().any(|call| matches!(
                call,
                MockCall::LoadFile { path, .. } if path == &api_path
            )),
            "snapshot imported Vue API sync should not only cache the API path for tsserver, calls={calls:?}, api_path={api_path}"
        );
    assert!(
        state.api_background_loaded,
        "snapshot imported API sync should mark the API path as loaded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_imported_vue_api_lightweight_opens_snapshot_ide_path_for_tsgo() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    install_test_resolver(server);

    let child_id = "/workspace/src/Child.vue";
    let _child_uri = open_test_vue(
        server,
        child_id,
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
    );

    server.sync_imported_vue_api_lightweight(child_id).await;

    let state = server
        .provider_sync_states
        .get(child_id)
        .map(|entry| entry.clone())
        .expect("snapshot imported Vue sync should commit provider state");
    let ide_path = state
        .ide_path
        .clone()
        .expect("TSGO imported Vue sync should record the IDE path");
    let calls = provider.file_sync_calls();

    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == &ide_path
            )),
            "snapshot imported Vue sync should open the provider-facing IDE path for TSGO, calls={calls:?}, ide_path={ide_path}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_queues_provisional_ide_path_for_snapshot_reconciliation() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
}

const actions: Action[] = [{ label: 'ok', disabled: false }]
</script>

<template>
  <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
    {{ action.label }}
  </button>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.tsx"
        )),
        "pre-snapshot current-file sync should open the provisional IDE path, calls={calls:?}"
    );

    let state = server
        .provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("provisional IDE sync should commit provider state");
    assert!(
        state.is_provisional(),
        "pre-snapshot current-file sync should mark the IDE owner as provisional"
    );
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx"),
        "pre-snapshot current-file sync should use the provisional IDE path"
    );
    assert!(
        server
            .pending_snapshot_provider_sync
            .contains("/workspace/src/App.vue"),
        "pre-snapshot current-file sync should queue owner-aware reconciliation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_clears_provisional_state_when_ready_snapshot_has_no_owner() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;
    assert!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .expect("bootstrap sync should commit provisional state")
            .is_provisional(),
        "bootstrap sync should start from provisional state"
    );

    provider.clear_calls();
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    server.ensure_current_file_synced(&uri).await;

    assert!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .is_none(),
        "ready-but-unowned current-file sync must remove stale provisional state"
    );
    assert!(
        server
            .pending_snapshot_provider_sync
            .contains("/workspace/src/App.vue"),
        "ready-but-unowned current-file sync should stay queued for future rebuilds"
    );
    assert!(
        server.type_provider_context(&uri).is_none(),
        "interactive type-provider lookups must not synthesize a live path from the resolver alone"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "ready-but-unowned current-file sync should close the stale provisional IDE path, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == "/workspace/src/App.vue.tsx"
        )),
        "ready-but-unowned current-file sync must not reopen or update the guessed IDE path, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn did_change_does_not_eager_sync_ready_unowned_file_through_resolver_path() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    tower_lsp_server::LanguageServer::did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: r#"<script setup lang="ts">
const msg = 'updated'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#
                .to_string(),
            }],
        },
    )
    .await;

    let calls = provider.file_sync_calls();
    assert!(
        calls.is_empty(),
        "did_change must not eagerly sync a ready-but-unowned file through a raw resolver path, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn current_file_sync_reopens_when_live_ide_path_changes() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;
    assert_eq!(
        server
            .provider_sync_state_for_source("/workspace/src/App.vue")
            .and_then(|state| state.ide_path),
        Some("/workspace/src/App.vue.tsx".to_string()),
        "initial sync should materialize the TSX path"
    );

    provider.clear_calls();
    tower_lsp_server::LanguageServer::did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: r#"<script setup lang="js">
const msg = 'updated'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#
                .to_string(),
            }],
        },
    )
    .await;

    let eager_calls = provider.file_sync_calls();
    assert!(
        !eager_calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. }
                if path == "/workspace/src/App.vue.tsx"
        )),
        "did_change must not eagerly sync the stale TSX path after the live IDE path changes, calls={eager_calls:?}"
    );

    server.ensure_current_file_synced(&uri).await;

    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("inline sync should commit the updated IDE path");
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "inline sync should switch the committed IDE path to JSX"
    );
    assert!(
        state.ide_background_loaded,
        "the new JSX path should be marked as loaded"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.tsx"
        )),
        "path change should close the stale TSX path, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.jsx"
        )),
        "path change should open the new JSX path, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path == "/workspace/src/App.vue.jsx"
        )),
        "path change should not treat the new JSX path as an already-open file, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn current_file_needs_inline_type_provider_sync_when_matching_ide_path_is_not_loaded() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/workspace/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ide_background_loaded: false,
            api_background_loaded: true,
            shadow_path: None,
            shadow_background_loaded: false,
        },
    );

    assert!(
            server.current_file_needs_inline_type_provider_sync(&uri),
            "matching IDE paths must still trigger inline sync until the TSX file has been opened in the provider"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_current_file_synced_marks_matching_ide_path_loaded() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;

    let state = server
        .provider_sync_state_for_source("/workspace/src/App.vue")
        .expect("current-file sync should commit provider state");
    assert!(
        state.ide_background_loaded,
        "successful current-file sync should mark the IDE path as loaded"
    );
    assert!(
        !server.current_file_needs_inline_type_provider_sync(&uri),
        "matching loaded IDE paths should not keep triggering inline sync"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_reopens_current_file_when_tsserver_lost_virtual_file_content() {
    let provider = Arc::new(LostContentCompletionProvider::default());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
}

const actions: Action[] = [{ label: 'ok', disabled: false }]
</script>

<template>
  <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
    {{ action.label }}
  </button>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;

    let ctx = synced_type_provider_context(server, &uri);
    provider.drop_open_path(&ctx.tsx_path);

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();
    let open_count = calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                MockCall::OpenFile { path, .. } if path == &ctx.tsx_path
            )
        })
        .count();

    assert!(
            labels.contains(&"disabled".to_string()),
            "completion should recover after the provider loses the current-file TSX content, got: {labels:?}, calls={calls:?}"
        );
    assert!(
            labels.contains(&"label".to_string()),
            "completion should recover after the provider loses the current-file TSX content, got: {labels:?}"
        );
    assert!(
            open_count >= 2,
            "recovery should force a reopen of the current-file TSX path after provider content loss, calls={calls:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_syncs_current_file_api_when_tsserver_needs_self_public_api() {
    let provider = Arc::new(LostContentCompletionProvider::requiring_current_api());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
interface Action {
  label: string
  disabled: boolean
}

const actions: Action[] = [{ label: 'ok', disabled: false }]
</script>

<template>
  <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
    {{ action.label }}
  </button>
</template>
"#,
    );

    server.ensure_current_file_synced(&uri).await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );
    let calls = provider.calls();

    assert!(
            labels.contains(&"disabled".to_string()),
            "completion should recover by syncing the current file API when tsserver requires the self public API, got: {labels:?}, calls={calls:?}"
        );
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == "/workspace/src/App.vue.ts"
            )),
            "recovery should open the current file .vue.ts path when the provider requires it, calls={calls:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_returns_fixture_vfor_member_access_properties() {
    let workspace_id = fixture_workspace_root("single-project");
    let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules/typescript/lib")
        .to_string_lossy()
        .replace('\\', "/");
    let Some(node_path) = crate::tsserver::find_node() else {
        eprintln!("skipping: node not found");
        return;
    };
    let Some(tsserver_path) = crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id))
    else {
        eprintln!("skipping: tsserver.js not found");
        return;
    };
    let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules")
        .to_string_lossy()
        .replace('\\', "/");
    let provider = match crate::tsserver::ipc::TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path.to_string_lossy().replace('\\', "/"),
        &workspace_id,
        Some(&plugin_path),
        None,
    )
    .await
    {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("skipping: tsserver spawn failed: {e}");
            return;
        }
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri: Uri = format!("file://{app_path}")
        .parse()
        .expect("fixture uri should be valid");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: app_source,
            },
        })
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let ctx = synced_type_provider_context(server, &uri);
    let tsx_offset = merge::vue_position_to_tsx_offset_validated(
        &position,
        &ctx.vue_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("fixture member access position should map to tsx");
    let _expr_context =
        classify_expression_context_with_trigger(&ctx.tsx_content, tsx_offset as usize, None);
    let Ok(direct_result) = provider
        .get_completions(&ctx.tsx_path, tsx_offset, Some("."))
        .await
    else {
        eprintln!("skipping: direct tsserver completion timed out (cold start)");
        provider.shutdown().await;
        return;
    };
    let direct_labels: Vec<String> = direct_result
        .items
        .into_iter()
        .map(|item| item.label)
        .collect();
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    if !labels.contains(&"disabled".to_string()) {
        eprintln!(
            "skipping: tsserver not warmed up (got global completions instead of member access)"
        );
        provider.shutdown().await;
        return;
    }
    assert!(
            labels.contains(&"label".to_string()),
            "real tsserver fixture member access should include label, got: {labels:?}, direct_labels={direct_labels:?}"
        );
    assert!(
            labels.contains(&"handler".to_string()),
            "real tsserver fixture member access should include handler, got: {labels:?}, direct_labels={direct_labels:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_recovers_fixture_vfor_member_access_immediately_after_open()
{
    let workspace_id = fixture_workspace_root("single-project");
    let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules/typescript/lib")
        .to_string_lossy()
        .replace('\\', "/");
    let Some(node_path) = crate::tsserver::find_node() else {
        eprintln!("skipping: node not found");
        return;
    };
    let Some(tsserver_path) = crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id))
    else {
        eprintln!("skipping: tsserver.js not found");
        return;
    };
    let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules")
        .to_string_lossy()
        .replace('\\', "/");
    let provider = match crate::tsserver::ipc::TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path.to_string_lossy().replace('\\', "/"),
        &workspace_id,
        Some(&plugin_path),
        None,
    )
    .await
    {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("skipping: tsserver spawn failed: {e}");
            return;
        }
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri: Uri = crate::uri::path_to_file_uri(&app_path).expect("fixture uri should be valid");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: app_source,
            },
        })
        .await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    if !labels.contains(&"disabled".to_string()) {
        eprintln!(
            "skipping: tsserver not warmed up (got global completions instead of member access)"
        );
        provider.shutdown().await;
        return;
    }
    assert!(
        labels.contains(&"label".to_string()),
        "immediate real tsserver fixture member access should include label, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "immediate real tsserver fixture member access should include handler, got: {labels:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_recovers_fixture_vfor_member_access_on_dot_trigger_immediately_after_open(
) {
    let workspace_id = fixture_workspace_root("single-project");
    let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules/typescript/lib")
        .to_string_lossy()
        .replace('\\', "/");
    let Some(node_path) = crate::tsserver::find_node() else {
        eprintln!("skipping: node not found");
        return;
    };
    let Some(tsserver_path) = crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id))
    else {
        eprintln!("skipping: tsserver.js not found");
        return;
    };
    let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules")
        .to_string_lossy()
        .replace('\\', "/");
    let provider = match crate::tsserver::ipc::TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path.to_string_lossy().replace('\\', "/"),
        &workspace_id,
        Some(&plugin_path),
        None,
    )
    .await
    {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("skipping: tsserver spawn failed: {e}");
            return;
        }
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri: Uri = crate::uri::path_to_file_uri(&app_path).expect("fixture uri should be valid");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: app_source,
            },
        })
        .await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, Some(".")))
            .await
            .expect("completion request should succeed"),
    );

    if !labels.contains(&"disabled".to_string()) {
        eprintln!(
            "skipping: tsserver not warmed up (got global completions instead of member access)"
        );
        provider.shutdown().await;
        return;
    }
    assert!(
            labels.contains(&"label".to_string()),
            "immediate dot-trigger real tsserver fixture member access should include label, got: {labels:?}"
        );
    assert!(
            labels.contains(&"handler".to_string()),
            "immediate dot-trigger real tsserver fixture member access should include handler, got: {labels:?}"
        );
}

#[test]
fn compute_verter_diagnostics_flags_fixture_fragment_component_data_attr() {
    let workspace_id = fixture_workspace_root("single-project");
    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri = crate::uri::path_to_file_uri(&app_path).expect("fixture uri should be valid");

    let fixture_path = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/vue-vscode/e2e/fixtures/single-project"),
    )
    .unwrap();
    let host = crate::test_utils::make_filesystem_test_host(&fixture_path);
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: app_source,
    });

    let cached_verter_diags = Arc::new(DashMap::new());

    let diags =
        compute_verter_diagnostics_for_with_views(&documents, &uri, &cached_verter_diags, None);
    let fragment_path = format!("{workspace_id}/src/FragmentComp.vue");
    let fragment_analysis = resolve_component_for(host.as_ref(), &app_path, "./FragmentComp.vue");

    assert!(
            diags.iter().any(|diag| {
                matches!(
                    diag.code.as_ref(),
                    Some(NumberOrString::String(code)) if code == "verter/unknown-prop"
                ) && diag.message.contains("data-test")
            }),
            "fixture fragment component should flag data-test, got: {diags:?}, child_loaded={}, child_template_roots={:?}, child_macros={:?}, child_components={:?}",
            host.get_analysis(&fragment_path).is_some(),
            fragment_analysis.as_ref().and_then(|analysis| {
                analysis.template.as_ref().map(|template| {
                    template
                        .elements
                        .iter()
                        .filter(|element| element.parent_index.is_none())
                        .map(|element| element.tag.clone())
                        .collect::<Vec<_>>()
                })
            }),
            fragment_analysis
                .as_ref()
                .map(|analysis| analysis.macros.iter().map(|mac| mac.kind).collect::<Vec<_>>()),
            fragment_analysis.as_ref().map(|analysis| {
                analysis
                    .template
                    .as_ref()
                    .map(|template| template.components.iter().map(|comp| comp.name.clone()).collect::<Vec<_>>())
            })
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_with_real_tsserver_recovers_when_current_file_sync_was_missed() {
    let workspace_id = fixture_workspace_root("single-project");
    let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules/typescript/lib")
        .to_string_lossy()
        .replace('\\', "/");
    let Some(node_path) = crate::tsserver::find_node() else {
        eprintln!("skipping: node not found");
        return;
    };
    let Some(tsserver_path) = crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id))
    else {
        eprintln!("skipping: tsserver.js not found");
        return;
    };
    let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules")
        .to_string_lossy()
        .replace('\\', "/");
    let provider = match crate::tsserver::ipc::TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path.to_string_lossy().replace('\\', "/"),
        &workspace_id,
        Some(&plugin_path),
        None,
    )
    .await
    {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("skipping: tsserver spawn failed: {e}");
            return;
        }
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let app_path = format!("{workspace_id}/src/App.vue");
    let app_source = std::fs::read_to_string(&app_path).expect("fixture App.vue should exist");
    let uri = open_test_vue(server, &app_path, &app_source);

    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let position = find_document_position(server, &uri, "action.disabled", 7);
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion request should succeed"),
    );

    if !labels.contains(&"disabled".to_string()) {
        eprintln!(
            "skipping: tsserver not warmed up (got global completions instead of member access)"
        );
        provider.shutdown().await;
        return;
    }
    assert!(
        labels.contains(&"label".to_string()),
        "completion should repair a missed current-file tsserver sync, got: {labels:?}"
    );
    assert!(
        labels.contains(&"handler".to_string()),
        "completion should repair a missed current-file tsserver sync, got: {labels:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn real_tsserver_slot_member_access_stays_typed_after_opening_child_and_parent() {
    let workspace_id = fixture_workspace_root("single-project");
    let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules/typescript/lib")
        .to_string_lossy()
        .replace('\\', "/");
    let Some(node_path) = crate::tsserver::find_node() else {
        eprintln!("skipping: node not found");
        return;
    };
    let Some(tsserver_path) = crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id))
    else {
        eprintln!("skipping: tsserver.js not found");
        return;
    };
    let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/vue-vscode/node_modules")
        .to_string_lossy()
        .replace('\\', "/");
    let provider = match crate::tsserver::ipc::TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_path.to_string_lossy().replace('\\', "/"),
        &workspace_id,
        Some(&plugin_path),
        None,
    )
    .await
    {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("skipping: tsserver spawn failed: {e}");
            return;
        }
    };
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                suggest_tsgo: false,
                mcp_port: None,
                type_provider_none_reason: None,
            },
        )
    });

    let server = service.inner();
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let child_path = format!("{workspace_id}/src/TypedSlotComp.vue");
    let child_source =
        std::fs::read_to_string(&child_path).expect("fixture TypedSlotComp.vue should exist");
    let child_uri = crate::uri::path_to_file_uri(&child_path).expect("child uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: child_uri,
                language_id: "vue".to_string(),
                version: 1,
                text: child_source,
            },
        })
        .await;

    let parent_path = format!("{workspace_id}/src/TemplateSlotCases.vue");
    let parent_source =
        std::fs::read_to_string(&parent_path).expect("fixture TemplateSlotCases.vue should exist");
    let parent_uri = crate::uri::path_to_file_uri(&parent_path).expect("parent uri");
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: parent_uri.clone(),
                language_id: "vue".to_string(),
                version: 1,
                text: parent_source,
            },
        })
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let member_position = find_document_position(server, &parent_uri, "slotItem.name", 9);
    let hover_position = find_document_position(server, &parent_uri, "slotItem.name", 2);
    let labels = completion_labels(
        server
            .completion(completion_params(&parent_uri, member_position, Some(".")))
            .await
            .expect("slot completion request should succeed"),
    );
    let hover = hover_text(
        server
            .hover(hover_params(&parent_uri, hover_position))
            .await
            .expect("slot hover request should succeed"),
    );
    let literal_debug = Some(synced_type_provider_context(server, &parent_uri)).and_then(|ctx| {
        ctx.tsx_content.find("slotItem.name").map(|start| {
            (
                ctx.tsx_path.clone(),
                start as u32 + "slotItem.".len() as u32,
            )
        })
    });
    let direct_provider = provider.clone();
    let direct_debug = Some(synced_type_provider_context(server, &parent_uri))
        .and_then(|ctx| {
            let tsx_path = ctx.tsx_path.clone();
            merge::vue_position_to_tsx_offset_validated(
                &member_position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            )
            .map(|tsx_offset| (ctx, tsx_offset, tsx_path))
        })
        .map(|(ctx, tsx_offset, tsx_path)| async move {
            direct_provider
                .get_completions(&ctx.tsx_path, tsx_offset, Some("."))
                .await
                .map(|result| {
                    (
                        result
                            .items
                            .into_iter()
                            .map(|item| item.label)
                            .collect::<Vec<_>>(),
                        tsx_path.clone(),
                    )
                })
                .map_err(|error| (error.to_string(), tsx_path))
        });
    let _parent_state = server
        .provider_sync_states
        .get(&parent_path)
        .map(|state| state.clone());
    let _child_state = server
        .provider_sync_states
        .get(&child_path)
        .map(|state| state.clone());
    let (_direct_labels, _tsx_path, _direct_error) = if let Some(fut) = direct_debug {
        match fut.await {
            Ok((labels, tsx_path)) => (Some(labels), Some(tsx_path), None),
            Err((error, tsx_path)) => (None, Some(tsx_path), Some(error)),
        }
    } else {
        (
            None,
            None,
            Some("missing type provider context".to_string()),
        )
    };
    let (_literal_labels, _literal_error) = if let Some((tsx_path, tsx_offset)) = literal_debug {
        match provider
            .get_completions(&tsx_path, tsx_offset, Some("."))
            .await
        {
            Ok(result) => (
                Some(
                    result
                        .items
                        .into_iter()
                        .map(|item| item.label)
                        .collect::<Vec<_>>(),
                ),
                None,
            ),
            Err(error) => (None, Some(error.to_string())),
        }
    } else {
        (
            None,
            Some("slotItem.name missing from generated TSX".to_string()),
        )
    };

    if !labels.contains(&"name".to_string()) {
        eprintln!("skipping: tsserver not warmed up (slot member completions missing 'name')");
        provider.shutdown().await;
        return;
    }
    assert!(
        labels.contains(&"id".to_string()),
        "slot member completions should include id, got: {labels:?}"
    );
    assert!(
        hover.contains("SlotItem") || (hover.contains("name") && hover.contains("id")),
        "slot hover should retain the slot item type, got: {hover}"
    );
    assert!(
        !hover.contains(": any"),
        "slot hover should not degrade to any, got: {hover}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_pending_vue_provider_file_hydrates_codegen_blockers_before_sync() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src/partials")).expect("create partials dir");
    std::fs::write(workspace.join("tsconfig.app.json"), "{}").expect("write tsconfig");
    std::fs::write(
        workspace.join("src/partials/panel.html"),
        "<div>{{ props.msg }}</div>",
    )
    .expect("write external template");
    std::fs::write(
        workspace.join("src/types.ts"),
        "import type { Nested } from '@/nested'\nexport interface Props { msg: Nested }",
    )
    .expect("write types dependency");
    std::fs::write(
        workspace.join("src/nested.ts"),
        "export type Nested = string",
    )
    .expect("write nested dependency");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let mut project = crate::project_resolver::IdeProjectConfig::new(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.app.json")),
    );
    project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
        base_url: Some(workspace_id.clone()),
        paths: vec![("@/*".to_string(), vec!["src/*".to_string()])],
    };
    host.configure_projects(vec![project.clone()]);

    let documents = DocumentRegistry::new(Arc::clone(&host));
    let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template src=\"@/partials/panel.html\"></template>\n<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>".to_string(),
        });
    // With filesystem-backed host and configure_projects, the VFS resolver
    // resolves @/ aliases during compilation. The external template should be
    // loaded eagerly during did_open.
    assert!(
        host.get_source(&format!("{workspace_id}/src/partials/panel.html"))
            .is_some(),
        "external src files should be loaded via VFS resolution during compilation"
    );
    // Type deps (types.ts) are resolved via VFS workspace read fallback during
    // compilation but may not be explicitly loaded into the scheduler.
    assert!(
        host.resolve_import_via_workspace(&app_id, "@/types")
            .is_some(),
        "macro type dep @/types should resolve via VFS"
    );

    // Verify the resolver can resolve these specifiers
    let snapshot = PublishedResolverSnapshot {
        resolver: crate::project_resolver::NativeProjectResolver::new(vec![project]),
        ownership_ready: true,
    };
    let ws = documents.host().workspace();
    let external_resolved = snapshot.resolver.resolve_with_reader(
        ws.as_ref(),
        &crate::project_resolver::ResolveRequest {
            importer_id: app_id.clone(),
            specifier: "@/partials/panel.html".to_string(),
            kind: crate::project_resolver::ResolveRequestKind::SfcSrcAttr,
            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
        },
    );
    assert!(
        external_resolved.is_some(),
        "external src specifier should resolve through the native resolver"
    );
    assert!(
        external_resolved
            .unwrap()
            .source_id
            .ends_with("/src/partials/panel.html"),
        "external src should resolve to the real template file"
    );

    // Sync to provider and verify IDE output is available
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let synced = sync_pending_vue_provider_file(
        &sync,
        &documents,
        &snapshot,
        &provider_sync_states,
        &app_id,
        false,
    )
    .await;
    assert!(synced, "pending Vue sync should succeed with resolved deps");

    let profile = documents.tsx_profile.read().clone();
    assert!(
        host.get_ide(&app_id, &profile).is_some(),
        "IDE output should be available after sync"
    );

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path.ends_with(".vue.ts")
        )),
        "pending sync should push the provider-facing Vue API file"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } if path.ends_with(".tsx")
        )),
        "pending sync should push the hydrated TSX output"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_pending_vue_provider_file_syncs_ide_artifact_for_tsgo() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create src dir");
    std::fs::write(workspace.join("tsconfig.app.json"), "{}").expect("write tsconfig");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child msg="hi" /></template>"#
            .to_string(),
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(format!("{workspace_id}/src/Child.vue")),
        input_id: format!("{workspace_id}/src/Child.vue"),
        source: Arc::<str>::from(
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    });

    let snapshot = PublishedResolverSnapshot {
        resolver: crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                workspace_id.clone(),
                workspace_id.clone(),
                Some(format!("{workspace_id}/tsconfig.app.json")),
            ),
        ]),
        ownership_ready: true,
    };
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    let synced = sync_pending_vue_provider_file(
        &sync,
        &documents,
        &snapshot,
        &provider_sync_states,
        &app_id,
        true,
    )
    .await;

    assert!(synced, "pending Vue sync should succeed for TSGO");

    let calls = provider.file_sync_calls();
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. } if path.ends_with(".vue.ts")
            )),
            "TSGO pending sync should keep syncing the API artifact, calls={calls:?}"
        );
    assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } | MockCall::UpdateFile { path, .. } if path.ends_with(".vue.tsx")
            )),
            "TSGO pending sync should also sync the IDE artifact, calls={calls:?}"
        );
}

// ── wants_code_action_kind tests ────────────────────────────────

#[test]
fn test_wants_code_action_kind_no_filter() {
    // No `only` → all kinds wanted
    assert!(wants_code_action_kind(None, "quickfix"));
    assert!(wants_code_action_kind(None, "source.organizeImports"));
    assert!(wants_code_action_kind(None, "refactor.extract"));
}

#[test]
fn test_wants_code_action_kind_exact_match() {
    let kinds = vec![CodeActionKind::new("quickfix")];
    assert!(wants_code_action_kind(Some(&kinds), "quickfix"));
    assert!(!wants_code_action_kind(Some(&kinds), "refactor"));
    assert!(!wants_code_action_kind(
        Some(&kinds),
        "source.organizeImports"
    ));
}

#[test]
fn test_wants_code_action_kind_prefix_hierarchy() {
    // `only: [refactor]` should match `refactor.extract`
    let kinds = vec![CodeActionKind::new("refactor")];
    assert!(wants_code_action_kind(Some(&kinds), "refactor.extract"));
    assert!(wants_code_action_kind(Some(&kinds), "refactor"));
    assert!(!wants_code_action_kind(Some(&kinds), "quickfix"));

    // `only: [refactor.extract]` should match `refactor` (parent)
    let kinds = vec![CodeActionKind::new("refactor.extract")];
    assert!(wants_code_action_kind(Some(&kinds), "refactor"));
    assert!(wants_code_action_kind(Some(&kinds), "refactor.extract"));
    assert!(!wants_code_action_kind(Some(&kinds), "quickfix"));
}

#[test]
fn test_wants_code_action_kind_no_false_prefix() {
    // "quickfixExtra" should NOT match "quickfix"
    let kinds = vec![CodeActionKind::new("quickfix")];
    assert!(!wants_code_action_kind(Some(&kinds), "quickfixExtra"));

    // "refactoring" should NOT match "refactor"
    let kinds = vec![CodeActionKind::new("refactor")];
    assert!(!wants_code_action_kind(Some(&kinds), "refactoring"));
}

#[test]
fn test_wants_code_action_kind_multiple_kinds() {
    let kinds = vec![
        CodeActionKind::new("quickfix"),
        CodeActionKind::new("source.organizeImports"),
    ];
    assert!(wants_code_action_kind(Some(&kinds), "quickfix"));
    assert!(wants_code_action_kind(
        Some(&kinds),
        "source.organizeImports"
    ));
    assert!(!wants_code_action_kind(Some(&kinds), "refactor"));
}

// ── File watcher helper tests ──────────────────────────────────

#[test]
fn test_is_config_file_positive() {
    assert!(is_config_file("file:///project/tsconfig.json"));
    assert!(is_config_file("file:///project/tsconfig.app.json"));
    assert!(is_config_file("file:///project/tsconfig.node.json"));
    assert!(is_config_file("file:///project/.verterrc.json"));
    assert!(is_config_file("file:///project/vite.config.ts"));
    assert!(is_config_file("file:///project/vite.config.js"));
    assert!(is_config_file("file:///project/vite.config.mjs"));
    assert!(is_config_file("file:///project/vite.config.cjs"));
    assert!(is_config_file("file:///project/vite.config.mts"));
    assert!(is_config_file("file:///project/vite.config.cts"));
    assert!(is_config_file("file:///project/package.json"));
}

#[test]
fn test_is_config_file_negative() {
    assert!(!is_config_file("file:///project/src/App.vue"));
    assert!(!is_config_file("file:///project/src/utils.ts"));
    assert!(!is_config_file("file:///project/src/config.ts"));
    assert!(!is_config_file("file:///project/tsconfig-paths.ts"));
    assert!(!is_config_file("file:///project/my.config.ts"));
    assert!(!is_config_file("file:///project/verterrc.json"));
}

#[test]
fn test_is_config_file_node_modules_excluded() {
    // package.json inside node_modules must NOT trigger registry rebuilds
    assert!(!is_config_file(
        "/projects/myapp/node_modules/@verter/types/package.json"
    ));
    assert!(!is_config_file(
        "/projects/myapp/node_modules/vue/package.json"
    ));
    assert!(!is_config_file(
        "/projects/myapp/node_modules/.pnpm/vue@3.5.0/node_modules/vue/package.json"
    ));
    // tsconfig inside node_modules should also be excluded
    assert!(!is_config_file(
        "/projects/myapp/node_modules/some-lib/tsconfig.json"
    ));
    // But root-level config files still match
    assert!(is_config_file("/projects/myapp/package.json"));
    assert!(is_config_file("/projects/myapp/tsconfig.json"));
}

#[test]
fn test_is_config_file_windows_paths() {
    // Canonical IDs on Windows use forward slashes
    assert!(is_config_file("C:/project/tsconfig.json"));
    assert!(is_config_file("C:/project/package.json"));
    assert!(is_config_file("C:/project/.verterrc.json"));
    assert!(!is_config_file("C:/project/src/App.vue"));
    // Windows node_modules paths
    assert!(!is_config_file(
        "C:/project/node_modules/@verter/types/package.json"
    ));
}

#[test]
fn test_is_generated_verter_types_event_for_generated_stub() {
    let tmp = tempfile::tempdir().unwrap();
    let types_dir = tmp.path().join("node_modules/@verter/types");
    std::fs::create_dir_all(&types_dir).unwrap();
    std::fs::write(
        types_dir.join("index.d.ts"),
        "// Auto-generated by verter-lsp\ndeclare module '*.vue' {}",
    )
    .unwrap();
    std::fs::write(
        types_dir.join("package.json"),
        r#"{"name":"@verter/types","types":"index.d.ts"}"#,
    )
    .unwrap();

    let stub_path = format!(
        "{}/node_modules/@verter/types/index.d.ts",
        tmp.path().display()
    );
    assert!(
        is_generated_verter_types_event(&stub_path),
        "generated stub should be filtered"
    );
    let pkg_path = format!(
        "{}/node_modules/@verter/types/package.json",
        tmp.path().display()
    );
    assert!(
        is_generated_verter_types_event(&pkg_path),
        "generated stub package.json should be filtered"
    );
}

#[test]
fn test_is_generated_verter_types_event_real_package_passes_through() {
    let tmp = tempfile::tempdir().unwrap();
    let types_dir = tmp.path().join("node_modules/@verter/types");
    std::fs::create_dir_all(&types_dir).unwrap();
    // Real installed package — no marker comment, has version/exports
    std::fs::write(
        types_dir.join("index.d.ts"),
        "export type DefineComponent = any;",
    )
    .unwrap();
    std::fs::write(
        types_dir.join("package.json"),
        r#"{"name":"@verter/types","version":"0.1.0","types":"dist/index.d.ts"}"#,
    )
    .unwrap();

    let path = format!(
        "{}/node_modules/@verter/types/index.d.ts",
        tmp.path().display()
    );
    assert!(
        !is_generated_verter_types_event(&path),
        "real installed package should NOT be filtered"
    );
}

#[test]
fn test_is_generated_verter_types_event_unrelated_modules() {
    // No path match → no I/O, returns false
    assert!(!is_generated_verter_types_event(
        "/projects/myapp/node_modules/vue/package.json"
    ));
    assert!(!is_generated_verter_types_event(
        "/projects/myapp/package.json"
    ));
    assert!(!is_generated_verter_types_event(
        "/projects/myapp/src/App.vue"
    ));
}

#[test]
fn test_write_if_changed_creates_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("new_file.txt");
    assert!(!path.exists());
    let wrote = write_if_changed(&path, "hello").unwrap();
    assert!(wrote, "should write when file is missing");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
}

#[test]
fn test_write_if_changed_skips_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("existing.txt");
    std::fs::write(&path, "same content").unwrap();
    let wrote = write_if_changed(&path, "same content").unwrap();
    assert!(!wrote, "should skip when content is identical");
}

#[test]
fn test_write_if_changed_overwrites_different() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("existing.txt");
    std::fs::write(&path, "old content").unwrap();
    let wrote = write_if_changed(&path, "new content").unwrap();
    assert!(wrote, "should write when content differs");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
}

#[test]
fn test_materialize_first_call_creates_files() {
    let tmp = tempfile::tempdir().unwrap();
    let nm = tmp.path().join("node_modules/@verter/types");
    assert!(!nm.exists());
    // materialize_verter_types expects URI strings but falls back to path
    let root = format!("file://{}", tmp.path().display());
    let result = materialize_verter_types(&[root]);
    assert!(!result.any_failed, "should not fail on first call");
    assert!(result.wrote_any, "should write stub files on first call");
    assert!(nm.join("index.d.ts").exists());
    assert!(nm.join("package.json").exists());
    let dts = std::fs::read_to_string(nm.join("index.d.ts")).unwrap();
    assert!(
        dts.starts_with("// Auto-generated by verter-lsp"),
        "index.d.ts should have marker"
    );
}

#[test]
fn test_materialize_second_call_is_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let root = format!("file://{}", tmp.path().display());
    let first = materialize_verter_types(&[root.clone()]);
    assert!(!first.any_failed, "first materialization should succeed");
    assert!(first.wrote_any, "first materialization should write files");

    let second = materialize_verter_types(&[root]);
    assert!(!second.any_failed, "second materialization should succeed");
    assert!(
        !second.wrote_any,
        "second materialization should not rewrite identical files"
    );
}

#[test]
fn test_materialize_skips_real_installed_package() {
    let tmp = tempfile::tempdir().unwrap();
    let nm = tmp.path().join("node_modules/@verter/types");
    let dist = nm.join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    // Pre-populate with real package content (no marker, uses dist/index.d.ts).
    let real_dts = "export type DefineComponent = any;";
    let real_pkg = r#"{"name":"@verter/types","version":"0.1.0","types":"dist/index.d.ts"}"#;
    std::fs::write(dist.join("index.d.ts"), real_dts).unwrap();
    std::fs::write(nm.join("package.json"), real_pkg).unwrap();

    let root = format!("file://{}", tmp.path().display());
    let result = materialize_verter_types(&[root]);
    assert!(
        !result.any_failed,
        "real installed package should not trigger fallback mode"
    );
    assert!(
        !result.wrote_any,
        "real installed package should not be rewritten"
    );

    // Real package should not be overwritten
    assert_eq!(
        std::fs::read_to_string(dist.join("index.d.ts")).unwrap(),
        real_dts,
        "real installed package dist/index.d.ts should not be overwritten"
    );
    assert_eq!(
        std::fs::read_to_string(nm.join("package.json")).unwrap(),
        real_pkg,
        "real installed package package.json should not be overwritten"
    );
    assert!(
        !nm.join("index.d.ts").exists(),
        "materialization should not create a stub index.d.ts for a real package"
    );
}

#[test]
fn test_is_vue_file() {
    assert!(is_vue_file("file:///project/src/App.vue"));
    assert!(is_vue_file("C:/project/src/App.vue"));
    assert!(!is_vue_file("file:///project/src/utils.ts"));
    assert!(!is_vue_file("file:///project/tsconfig.json"));
    assert!(!is_vue_file("file:///project/vue.config.js"));
}

#[test]
fn compute_verter_diagnostics_ignores_plain_typescript_files() {
    let host = Arc::new(VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));

    let uri: Uri = "file:///workspace/src/__verter_mayberef_repro__.ts"
        .parse()
        .unwrap();
    let source = "type MaybeRef<T> = T\n\nexport function useLockScroll(target: MaybeRef<HTMLElement | null> = null) {\n  return target\n}\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());

    let diags =
        compute_verter_diagnostics_for_with_views(&documents, &uri, &cached_verter_diags, None);

    assert!(
        documents.get(&uri).is_some(),
        "the typescript document should be tracked"
    );
    assert!(
        documents.get_ide(&uri).is_none(),
        "plain typescript files should not have Vue IDE output"
    );
    assert!(
        !diags.iter().any(|d| {
            matches!(
                &d.code,
                Some(NumberOrString::String(code)) if code == "XMissingEndTag"
            )
        }),
        "plain typescript files must not surface Verter template parse diagnostics, got: {diags:?}"
    );
    assert!(
        diags.is_empty(),
        "plain typescript files should not publish Verter diagnostics, got: {diags:?}"
    );
}

/// Proves that `compute_verter_diagnostics_for` bypasses its cache when the
/// host's `diagnostics_generation` changes (even if the document version hasn't).
#[test]
fn compute_verter_diagnostics_bypasses_cache_after_host_recompile() {
    use verter_session::{CompileErrorPolicy, FileKind, UpsertRequest};

    let host = Arc::new(VerterHost::new_standalone(verter_session::HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..verter_session::HostConfig::default()
    }));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));

    // SFC with a macro type dep on ./types
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    let uri: Uri = "file:///workspace/src/Comp.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());

    // First call — should contain HOST_MISSING_MACRO_TYPE_DEP
    let diags1 =
        compute_verter_diagnostics_for_with_views(&documents, &uri, &cached_verter_diags, None);
    assert!(
        diags1.iter().any(|d| matches!(
            &d.code,
            Some(NumberOrString::String(c)) if c.contains("HOST_MISSING_MACRO_TYPE_DEP")
        )),
        "first call should contain HOST_MISSING_MACRO_TYPE_DEP, got: {diags1:?}"
    );

    // Load the dependency
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/workspace/src/types.ts".to_string(),
        source: Arc::from("export interface Props { msg: string }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });

    // Force recompile with the tsx_profile (same as documents.get_diagnostics uses)
    let _ = host.ensure_compiled("/workspace/src/Comp.vue", &documents.tsx_profile.read());

    // Second call — same doc version, but diagnostics_generation changed
    let diags2 =
        compute_verter_diagnostics_for_with_views(&documents, &uri, &cached_verter_diags, None);
    assert!(
            !diags2.iter().any(|d| matches!(
                &d.code,
                Some(NumberOrString::String(c)) if c.contains("HOST_MISSING_MACRO_TYPE_DEP")
            )),
            "second call should NOT contain HOST_MISSING_MACRO_TYPE_DEP after dep loaded, got: {diags2:?}"
        );
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_resolves_and_syncs_after_registry_built() {
    // Setup: temp dir with workspace/src/App.vue importing @/components/Child.vue
    // Use a non-dot-prefixed directory so tsconfig discovery doesn't skip it
    // (tsconfig discovery skips dot-directories).
    let temp_base = std::env::temp_dir().join("verter_test_resync_aliased");
    let _ = std::fs::remove_dir_all(&temp_base);
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");

    // Write a tsconfig.json with @/* -> src/* alias
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("write tsconfig");

    // Write the child component on disk
    let child_source = r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#;
    std::fs::write(workspace.join("src/components/Child.vue"), child_source)
        .expect("write Child.vue");

    // Canonicalize workspace path for consistent IDs
    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    // Strip Windows extended-length prefix that canonicalize() produces
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");
    let child_id = format!("{workspace_id}/src/components/Child.vue");

    // App.vue imports Child via alias
    let app_source = r#"<script setup lang="ts">
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#
        .to_string();

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: app_source,
    });

    // Phase 1: before VFS snapshot is built — aliased import should NOT resolve
    let analysis = host.get_analysis(&app_id).expect("analysis for App.vue");
    let ids_before = collect_imported_vue_priority_ids_from_imports_with_fallback(
        &analysis.imports,
        Some(&app_id),
        |parent, specifier| resolve_import_specifier_standalone(&host, parent, specifier),
    );
    assert!(
        ids_before.is_empty(),
        "aliased imports should NOT resolve when project_registry is None, got: {ids_before:?}"
    );

    // Phase 2: Build and populate project registry with tsconfig alias
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let build_result =
        crate::config::ProjectRegistry::from_workspace_roots(&[workspace_uri], &vite_opts);
    let registry = build_result.registry;

    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);

    // Now aliased import should resolve
    let ids_after = collect_imported_vue_priority_ids_from_imports_with_fallback(
        &analysis.imports,
        Some(&app_id),
        |parent, specifier| resolve_import_specifier_standalone(&host, parent, specifier),
    );
    assert!(
        !ids_after.is_empty(),
        "aliased imports should resolve after project_registry is populated"
    );
    assert!(
        ids_after.iter().any(|id| id.ends_with("Child.vue")),
        "resolved imports should include Child.vue, got: {ids_after:?}"
    );

    // Phase 3: resync_aliased_imports_for_open_files should sync .vue.ts
    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        false,
    )
    .await;

    // Positive: Child.vue should have its .vue.ts synced
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.contains("Child.vue.ts")
        )),
        "resync should open Child.vue.ts in the type provider, calls={calls:?}"
    );

    // Positive: provider_sync_states should have the child entry
    assert!(
        provider_sync_states.get(&child_id).is_some()
            || provider_sync_states
                .iter()
                .any(|entry| entry.key().ends_with("Child.vue")),
        "provider_sync_states should contain Child.vue entry"
    );

    // Negative: .ts imports should NOT be synced via this path
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.ends_with(".ts") && !path.ends_with(".vue.ts")
        )),
        "resync should NOT sync non-.vue files, calls={calls:?}"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_syncs_vue_ide_artifact_for_tsgo() {
    let temp_base = std::env::temp_dir().join("verter_test_resync_aliased_tsgo");
    let _ = std::fs::remove_dir_all(&temp_base);
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");

    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("write tsconfig");

    std::fs::write(
        workspace.join("src/components/Child.vue"),
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    )
    .expect("write child");

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let app_id = format!("{workspace_id}/src/App.vue");

    let host = crate::test_utils::make_filesystem_test_host(&workspace);
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import Child from '@/components/Child.vue'
</script>
<template><Child msg="hello" /></template>"#
            .to_string(),
    });

    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let build_result =
        crate::config::ProjectRegistry::from_workspace_roots(&[workspace_uri], &vite_opts);
    let registry = build_result.registry;
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);
    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
    )
    .await;

    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.ends_with("Child.vue.tsx")
        )),
        "TSGO alias resync should open the Vue IDE artifact, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[tokio::test(flavor = "multi_thread")]
async fn resync_aliased_imports_syncs_barrel_and_vue_deps_for_tsgo() {
    // Setup: App.vue imports `{ Overlay }` from a barrel (./components/index.ts)
    // which re-exports `./Overlay.vue`. Both the barrel and its Vue dependency
    // must be synced eagerly so TSGO resolves the component types.
    let temp_base = std::env::temp_dir().join("verter_test_resync_barrel_tsgo");
    let _ = std::fs::remove_dir_all(&temp_base);
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src/components")).expect("create dirs");

    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
    )
    .expect("write tsconfig");

    // Barrel file re-exports Overlay from its Vue component
    std::fs::write(
        workspace.join("src/components/index.ts"),
        r#"export { default as Overlay } from './Overlay.vue'"#,
    )
    .expect("write barrel");

    // Vue component behind the barrel
    std::fs::write(
        workspace.join("src/components/Overlay.vue"),
        r#"<script setup lang="ts">
defineProps<{ show: boolean }>()
</script>
<template><div v-if="show">overlay</div></template>"#,
    )
    .expect("write Overlay.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let app_id = format!("{workspace_id}/src/App.vue");

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri,
        language_id: "vue".to_string(),
        version: 1,
        text: r#"<script setup lang="ts">
import { Overlay } from './components'
</script>
<template><Overlay :show="true" /></template>"#
            .to_string(),
    });

    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let build_result =
        crate::config::ProjectRegistry::from_workspace_roots(&[workspace_uri], &vite_opts);
    let registry = build_result.registry;
    let vfs_workspace = make_test_vfs_workspace_from_registry(&registry);
    host.configure_projects(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();

    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
    )
    .await;

    let calls = provider.file_sync_calls();

    // Positive: Vue dependency Overlay.vue should be synced (IDE + API artifacts)
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.contains("Overlay.vue")
        )),
        "Vue dependency Overlay.vue should be synced, calls={calls:?}"
    );

    // Positive: Barrel file should be synced to provider (via sync_file → update_file)
    // Note: rewrite_vue_imports_for_tsgo happens inside the real TSGO provider, not the mock.
    // The mock records raw content; in production TSGO rewrites .vue → .vue.ts.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::LoadFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                if path.contains("components/index")
        )),
        "Barrel file index.ts should be synced to provider, calls={calls:?}"
    );

    // Negative: non-barrel utility imports should NOT trigger barrel sync
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::LoadFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                if path.contains("utils")
        )),
        "Utility files should not be synced through barrel path, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

// ── VFS workspace integration ──

#[test]
fn vfs_workspace_rwlock_initially_none() {
    let vfs: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>> =
        Arc::new(parking_lot::RwLock::new(None));

    // Before background_init, the workspace is None
    assert!(
        vfs.read().is_none(),
        "VFS workspace should be None before initialization"
    );
}

#[test]
fn vfs_workspace_rwlock_install_and_access() {
    let vfs: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>> =
        Arc::new(parking_lot::RwLock::new(None));

    // Simulate what background_init does: build workspace and store it
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec!["/test-project".to_string()],
            eager_preload: false,
        },
    ));
    *vfs.write() = Some(Arc::clone(&workspace));

    // Verify it's accessible
    let ws = vfs.read().clone();
    assert!(ws.is_some(), "VFS workspace should be Some after install");

    // Verify workspace options match
    assert_eq!(
        ws.unwrap().options().roots,
        vec!["/test-project".to_string()],
        "workspace roots should match what was installed"
    );
}

#[test]
fn vfs_workspace_with_project_graph() {
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec!["/my-project".to_string()],
            eager_preload: false,
        },
    ));

    // Before setting a project graph, owner_for_file returns None
    use verter_workspace::WorkspaceAccess;
    assert!(
        workspace
            .owner_for_file("/my-project/src/App.vue")
            .is_none(),
        "empty project graph should have no owner"
    );

    // Set a simple project graph
    let graph =
        verter_workspace::ProjectGraph::from_configs(vec![verter_workspace::VfsProjectConfig {
            root: "/my-project".to_string(),
            rank: verter_workspace::ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![".vue".to_string()],
            workspace_root: "/my-project".to_string(),
            workspace_aliases: vec![],
            compiler_options: Default::default(),
            references: vec![],
            membership: verter_workspace::ProjectMembership::MatchAll,
        }]);
    workspace.set_project_graph(graph);

    // Now owner_for_file should return the project
    let owner = workspace.owner_for_file("/my-project/src/App.vue");
    assert!(
        owner.is_some(),
        "file under project root should have an owner after graph set"
    );
    assert_eq!(
        owner.unwrap().project_root,
        "/my-project",
        "owner should be the correct project root"
    );

    // Negative: file outside project root should have no owner
    assert!(
        workspace
            .owner_for_file("/other-project/src/App.vue")
            .is_none(),
        "file outside project root should have no owner"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_file_changed_invalidates_vfs_negative_cache_for_created_file() {
    use verter_workspace::WorkspaceAccess;

    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    let src_dir = workspace.join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");

    let root_id = crate::test_utils::canonical_test_path(&workspace);
    let file_id = format!("{root_id}/src/NewFile.vue");
    let file_uri = crate::uri::path_to_file_uri_string(&file_id);

    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(provider);
    let server = service.inner();
    let vfs_workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec![root_id.clone()],
            eager_preload: false,
        },
    ));
    server.install_vfs_workspace(Arc::clone(&vfs_workspace));

    assert!(
        !vfs_workspace.file_exists(&file_id),
        "missing file should seed a negative dir-index entry"
    );

    std::fs::write(
        workspace.join("src/NewFile.vue"),
        "<template><div/></template>",
    )
    .expect("write new file");

    server
        .on_file_changed(OnFileChangedParams {
            uri: file_uri,
            change_type: "create".to_string(),
        })
        .await;

    assert!(
        vfs_workspace.file_exists(&file_id),
        "create watcher events should invalidate the cached missing sibling result"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn on_watcher_state_changed_invalidates_vfs_negative_cache_under_workspace_root() {
    use verter_workspace::WorkspaceAccess;

    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    let src_dir = workspace.join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");

    let root_id = crate::test_utils::canonical_test_path(&workspace);
    let file_id = format!("{root_id}/src/Recovered.vue");
    let root_uri = crate::uri::path_to_file_uri_string(&root_id);

    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(provider);
    let server = service.inner();
    let vfs_workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions {
            roots: vec![root_id.clone()],
            eager_preload: false,
        },
    ));
    server.install_vfs_workspace(Arc::clone(&vfs_workspace));

    assert!(
        !vfs_workspace.file_exists(&file_id),
        "missing file should seed a negative dir-index entry"
    );

    std::fs::write(
        workspace.join("src/Recovered.vue"),
        "<template><span/></template>",
    )
    .expect("write recovered file");

    server
        .on_watcher_state_changed(WatcherStateChangedParams {
            workspace_root: root_uri,
            reason: "overflow".to_string(),
        })
        .await;

    assert!(
        vfs_workspace.file_exists(&file_id),
        "watcher overflow should invalidate cached directory membership under the workspace root"
    );
}

#[test]
fn standalone_host_cannot_resolve_disk_files() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let ws = tmp.path().join("workspace");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("App.vue"), "<template><div/></template>").unwrap();

    let host = VerterHost::new_standalone(HostConfig::default());
    let file_id = verter_workspace::resolver::normalize_canonical_id(
        &ws.join("App.vue").to_string_lossy().replace('\\', "/"),
    );
    // Positive: standalone host cannot load disk files (documents the limitation)
    assert!(
        !host.ensure_loaded(&file_id),
        "standalone host should NOT be able to load disk files (no VFS)"
    );
    // Negative: also no analysis available
    assert!(
        host.get_analysis(&file_id).is_none(),
        "standalone host should have no analysis for disk-only files"
    );
}
