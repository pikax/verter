#![allow(clippy::cloned_ref_to_slice_refs, clippy::type_complexity)]
//! Shared test harness for server-level integration tests with real type providers.
//!
//! Provides `TestSessionBuilder` (fluent builder), `RealProviderTestSession` (convenience
//! methods for completions, hover, go-to-definition), and the `real_provider_test!` macro
//! that generates both tsserver and TSGO test variants from a single test body.
//!
//! **Fully virtual filesystem**: No temp dirs or file writes. The E2E fixtures provide
//! the project scaffold (tsconfig.json, node_modules/vue) already on disk. Test file
//! content is fed entirely through in-memory APIs (`host.upsert()` + `did_open()`).

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use tower_lsp_server::LanguageServer;
use verter_session::{HostConfig, VerterHost};

use crate::server::VerterLanguageServer;
use crate::type_provider::traits::TypeProvider;
use crate::LspConfig;

// ---------------------------------------------------------------------------
// Provider kind
// ---------------------------------------------------------------------------

/// Which real type provider to spawn.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TestProviderKind {
    Tsserver,
    Tsgo,
}

// ---------------------------------------------------------------------------
// Session builder
// ---------------------------------------------------------------------------

/// Fluent builder for `RealProviderTestSession`.
pub(crate) struct TestSessionBuilder {
    kind: TestProviderKind,
    fixture: Option<String>,
    fixture_files: Vec<String>,
    virtual_files: Vec<(String, String)>,
}

impl TestSessionBuilder {
    pub(crate) fn new(kind: TestProviderKind) -> Self {
        Self {
            kind,
            fixture: None,
            fixture_files: Vec::new(),
            virtual_files: Vec::new(),
        }
    }

    /// Use an E2E fixture workspace root for the project scaffold.
    pub(crate) fn fixture(mut self, name: &str) -> Self {
        self.fixture = Some(name.to_string());
        self
    }

    /// Queue a fixture file to be opened after build (reads from disk, writes nothing).
    #[allow(dead_code)]
    pub(crate) fn open_fixture_file(mut self, relative_path: &str) -> Self {
        self.fixture_files.push(relative_path.to_string());
        self
    }

    /// Queue a virtual file with inline content (no disk I/O).
    #[allow(dead_code)]
    pub(crate) fn open_virtual(mut self, relative_path: &str, content: &str) -> Self {
        self.virtual_files
            .push((relative_path.to_string(), content.to_string()));
        self
    }

    /// Build the session. Returns `None` (and prints skip message) when binaries are
    /// not found, so tests degrade gracefully on machines without tsserver/TSGO.
    pub(crate) async fn build(self) -> Option<RealProviderTestSession> {
        let fixture_name = self.fixture.as_deref().unwrap_or("single-project");
        let workspace_id = fixture_workspace_root(fixture_name);

        let provider: Arc<dyn TypeProvider> = match self.kind {
            TestProviderKind::Tsserver => {
                let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../packages/vue-vscode/node_modules/typescript/lib")
                    .to_string_lossy()
                    .replace('\\', "/");
                let node_path = match crate::tsserver::find_node() {
                    Some(p) => p,
                    None => {
                        eprintln!("skipping: node not found");
                        return None;
                    }
                };
                let tsserver_path =
                    match crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id)) {
                        Some(p) => p,
                        None => {
                            eprintln!("skipping: tsserver.js not found");
                            return None;
                        }
                    };
                let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../packages/vue-vscode/node_modules")
                    .to_string_lossy()
                    .replace('\\', "/");
                match crate::tsserver::ipc::TsserverTypeProvider::spawn(
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
                        return None;
                    }
                }
            }
            TestProviderKind::Tsgo => {
                // Prefer the repo-local tsgo (installed as a workspace dev
                // dependency) so the parity tests run against the SAME tsgo the
                // project pins, regardless of PATH / npm-cache state. Falls back
                // to the system discovery (PATH + npm/npx cache).
                let repo_node_modules =
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node_modules");
                let tsgo_bin: String =
                    match crate::tsgo::ipc::find_tsgo_binary_under_node_modules(&repo_node_modules)
                    {
                        Some(bin) => bin,
                        None => match crate::tsgo::ipc::find_tsgo_binary() {
                            Ok(bin) => bin,
                            Err(err) => {
                                eprintln!("skipping: tsgo binary not found: {err}");
                                return None;
                            }
                        },
                    };
                let root_uri = crate::uri::path_to_file_uri_string(&workspace_id);
                match crate::tsgo::ipc::TsgoTypeProvider::spawn(&tsgo_bin, &root_uri).await {
                    Ok(p) => Arc::new(p),
                    Err(e) => {
                        eprintln!("skipping: tsgo spawn failed: {e}");
                        return None;
                    }
                }
            }
        };

        let provider_kind = match self.kind {
            TestProviderKind::Tsserver => crate::TypeProviderKind::Tsserver,
            TestProviderKind::Tsgo => crate::TypeProviderKind::Tsgo,
        };

        let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> =
            Arc::new(verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            ));
        let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
        let host_for_server = Arc::clone(&host);
        let type_provider_for_server = Arc::clone(&provider);
        let (service, socket) = tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host_for_server),
                    type_provider: Some(Arc::clone(&type_provider_for_server)),
                    project_sync_mode: crate::ProjectSyncMode::FullProject,
                    type_provider_kind: provider_kind,
                    suggest_tsgo: false,
                    mcp_port: None,
                    type_provider_none_reason: None,
                },
            )
        });

        // Drain the client socket to prevent backpressure
        let drain_handle = tokio::spawn(async move {
            use futures_util::StreamExt;
            let mut socket = socket;
            while socket.next().await.is_some() {}
        });

        let server = service.inner();

        // Build a project registry from the workspace root so verter-internal
        // definition handlers can resolve path aliases (e.g. "@/*" → "./src/*").
        let root_uri = crate::uri::path_to_file_uri_string(&workspace_id);
        let tsconfig_path_str = format!("{workspace_id}/tsconfig.json");
        let vite_opts = verter_workspace::ViteConfigOptions::default();
        let registry_ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let build_result = crate::config::ProjectRegistry::from_workspace_roots(
            &registry_ws,
            &[root_uri.clone()],
            &vite_opts,
        );
        // Sync resolver to host's VFS so resolve_import_via_workspace works
        host.configure_projects(
            build_result
                .registry
                .projects()
                .iter()
                .map(|p| p.to_ide_project_config())
                .collect(),
        );

        // Build and install VFS workspace with published snapshot (replaces old resolver_snapshot + project_registry)
        {
            let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            ));
            let vfs_vite_opts = verter_workspace::ViteConfigOptions {
                enabled: false,
                trusted_files: Vec::new(),
                node_path: None,
            };
            let vfs_build = verter_workspace::ProjectGraph::from_workspace_roots(
                &*vfs_ws,
                &[workspace_id.clone()],
                &vfs_vite_opts,
            );
            vfs_ws.set_project_graph(vfs_build.graph);
            if let Some(published) = vfs_ws.load_published() {
                let snapshot_arc = Arc::clone(&published.snapshot);
                let views =
                    crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot_arc, vec![]);
                vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
                    snapshot_arc,
                    Box::new(views),
                ));
            }
            server.install_vfs_workspace(vfs_ws);
        }

        // Replicate the lifecycle from `initialized()`:
        // 1. Notify the type provider about workspace folders
        let added = vec![serde_json::json!({
            "uri": root_uri,
            "name": workspace_id.rsplit('/').next().unwrap_or(&workspace_id)
        })];
        let _ = provider.update_workspace_folders(added, vec![]).await;

        // 2. Configure tsconfig paths (e.g. "@/*" → "./src/*") so the provider
        //    can resolve path aliases in go-to-definition and completions.
        let tsconfig_path = std::path::PathBuf::from(&tsconfig_path_str);
        if tsconfig_path.exists() {
            let ws = verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            );
            if let Some((base_url, paths)) =
                verter_workspace::config::raw_paths_json(&ws, &tsconfig_path_str)
            {
                let _ = provider.configure_paths(&base_url, paths).await;
            }
        }

        let session = RealProviderTestSession {
            service,
            provider,
            workspace_id,
            kind: self.kind,
            _drain_handle: drain_handle,
        };

        // Open queued fixture files
        for relative_path in &self.fixture_files {
            session.open_fixture_file(relative_path).await;
        }

        // Open queued virtual files
        for (relative_path, content) in &self.virtual_files {
            session.open_virtual(relative_path, content).await;
        }

        Some(session)
    }
}

// ---------------------------------------------------------------------------
// Test session
// ---------------------------------------------------------------------------

/// A live LSP server session backed by a real type provider process.
pub(crate) struct RealProviderTestSession {
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    provider: Arc<dyn TypeProvider>,
    workspace_id: String,
    kind: TestProviderKind,
    _drain_handle: tokio::task::JoinHandle<()>,
}

impl RealProviderTestSession {
    /// Access the underlying server.
    pub(crate) fn server(&self) -> &VerterLanguageServer {
        self.service.inner()
    }

    /// Which provider backend this session uses.
    #[allow(dead_code)]
    pub(crate) fn provider_kind(&self) -> TestProviderKind {
        self.kind
    }

    /// Returns `true` when this session uses TSGO.
    pub(crate) fn is_tsgo(&self) -> bool {
        matches!(self.kind, TestProviderKind::Tsgo)
    }

    /// Direct access to the underlying real type provider.
    ///
    /// For provider-level integration tests (diagnostics / completion-detail
    /// parity) that exercise the provider contract directly rather than the full
    /// LSP carrier-mapping path.
    pub(crate) fn provider(&self) -> &Arc<dyn TypeProvider> {
        &self.provider
    }

    /// Open a generated `.tsx`/`.ts` file DIRECTLY in the type provider (as an
    /// editor-open buffer that triggers diagnostics) and return its provider
    /// path. Used by provider-level integration tests to drive the real backend's
    /// own diagnostics / completion paths without the Vue carrier indirection.
    ///
    /// The path is rooted under the fixture workspace so the provider resolves it
    /// against the fixture's `tsconfig` + `node_modules`.
    pub(crate) async fn open_in_provider(&self, relative_path: &str, content: &str) -> String {
        let abs_path = format!("{}/{relative_path}", self.workspace_id);
        self.provider
            .open_file(&abs_path, content)
            .await
            .expect("provider open_file should succeed");
        abs_path
    }

    /// Build a `file://` URI from a fixture-relative path.
    #[allow(dead_code)]
    pub(crate) fn workspace_uri(&self, relative_path: &str) -> Uri {
        crate::uri::path_to_file_uri(&format!("{}/{relative_path}", self.workspace_id))
            .expect("workspace uri")
    }

    /// Read a file from the fixture on disk and open it in the server (no disk writes).
    pub(crate) async fn open_fixture_file(&self, relative_path: &str) -> Uri {
        let abs_path = format!("{}/{relative_path}", self.workspace_id);
        let source = std::fs::read_to_string(&abs_path)
            .unwrap_or_else(|e| panic!("fixture file should exist: {abs_path}: {e}"));
        let uri = crate::uri::path_to_file_uri(&abs_path).expect("fixture file uri");
        self.server()
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language_id_for(relative_path),
                    version: 1,
                    text: source,
                },
            })
            .await;
        uri
    }

    /// Open inline content under a virtual path within the fixture root (no disk I/O).
    pub(crate) async fn open_virtual(&self, relative_path: &str, content: &str) -> Uri {
        let abs_path = format!("{}/{relative_path}", self.workspace_id);
        let uri = crate::uri::path_to_file_uri(&abs_path).expect("virtual file uri");
        self.server()
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language_id_for(relative_path),
                    version: 1,
                    text: content.to_string(),
                },
            })
            .await;
        uri
    }

    /// Find a position within an open document by searching for `needle` and adding `delta`.
    pub(crate) fn find_position(&self, uri: &Uri, needle: &str, delta: usize) -> Position {
        let doc = self
            .server()
            .test_documents()
            .get(uri)
            .expect("document should be open");
        let offset = doc
            .source
            .find(needle)
            .unwrap_or_else(|| panic!("needle `{needle}` should exist in document"))
            + delta;
        doc.line_index
            .offset_to_position(offset as u32)
            .expect("valid position")
    }

    /// Find the Nth (0-indexed) occurrence of `needle` and add `delta`.
    pub(crate) fn find_nth_position(
        &self,
        uri: &Uri,
        needle: &str,
        n: usize,
        delta: usize,
    ) -> Position {
        let doc = self
            .server()
            .test_documents()
            .get(uri)
            .expect("document should be open");
        let mut start = 0;
        let mut count = 0;
        loop {
            match doc.source[start..].find(needle) {
                Some(pos) => {
                    let abs_pos = start + pos;
                    if count == n {
                        return doc
                            .line_index
                            .offset_to_position((abs_pos + delta) as u32)
                            .expect("valid position");
                    }
                    count += 1;
                    start = abs_pos + 1;
                }
                None => {
                    panic!("needle `{needle}` occurrence {n} not found (only {count} occurrences)")
                }
            }
        }
    }

    /// Ensure the current file is synced to the type provider.
    pub(crate) async fn ensure_synced(&self, uri: &Uri) {
        self.server().test_ensure_synced(uri).await;
    }

    /// Get completion labels at a position.
    pub(crate) async fn completion_labels(
        &self,
        uri: &Uri,
        position: Position,
        trigger: Option<&str>,
    ) -> Vec<String> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: trigger
                    .map(|_| CompletionTriggerKind::TRIGGER_CHARACTER)
                    .unwrap_or(CompletionTriggerKind::INVOKED),
                trigger_character: trigger.map(str::to_string),
            }),
        };
        match self.server().completion(params).await {
            Ok(Some(CompletionResponse::Array(items))) => {
                items.into_iter().map(|item| item.label).collect()
            }
            Ok(Some(CompletionResponse::List(list))) => {
                list.items.into_iter().map(|item| item.label).collect()
            }
            Ok(None) => Vec::new(),
            Err(e) => {
                eprintln!("completion error: {e}");
                Vec::new()
            }
        }
    }

    /// Get hover text at a position.
    pub(crate) async fn hover_text(&self, uri: &Uri, position: Position) -> Option<String> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        match self.server().hover(params).await {
            Ok(Some(hover)) => match hover.contents {
                HoverContents::Markup(m) => Some(m.value),
                HoverContents::Scalar(MarkedString::String(s)) => Some(s),
                HoverContents::Scalar(MarkedString::LanguageString(ls)) => Some(ls.value),
                HoverContents::Array(items) => Some(
                    items
                        .into_iter()
                        .map(|item| match item {
                            MarkedString::String(s) => s,
                            MarkedString::LanguageString(ls) => ls.value,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            },
            Ok(None) => None,
            Err(e) => {
                eprintln!("hover error: {e}");
                None
            }
        }
    }

    /// Get go-to-definition locations at a position.
    pub(crate) async fn definitions(
        &self,
        uri: &Uri,
        position: Position,
    ) -> Option<GotoDefinitionResponse> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        match self.server().goto_definition(params).await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("goto_definition error: {e}");
                None
            }
        }
    }

    /// Get go-to-definition locations flattened to `Vec<Location>`.
    pub(crate) async fn definition_locations(
        &self,
        uri: &Uri,
        position: Position,
    ) -> Vec<Location> {
        match self.definitions(uri, position).await {
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
        }
    }

    /// Get references at a position (includes declaration).
    pub(crate) async fn references(&self, uri: &Uri, position: Position) -> Vec<Location> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };
        match self.server().references(params).await {
            Ok(Some(locs)) => locs,
            Ok(None) => Vec::new(),
            Err(e) => {
                eprintln!("references error: {e}");
                Vec::new()
            }
        }
    }

    /// Call prepare_rename at a position.
    pub(crate) async fn prepare_rename(
        &self,
        uri: &Uri,
        position: Position,
    ) -> Option<PrepareRenameResponse> {
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        };
        match self.server().prepare_rename(params).await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("prepare_rename error: {e}");
                None
            }
        }
    }

    /// Call rename at a position with a new name.
    pub(crate) async fn rename_edits(
        &self,
        uri: &Uri,
        position: Position,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        };
        match self.server().rename(params).await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("rename error: {e}");
                None
            }
        }
    }

    /// Get document symbols flattened to a list of names.
    pub(crate) async fn document_symbols(&self, uri: &Uri) -> Vec<String> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        match self.server().document_symbol(params).await {
            Ok(Some(DocumentSymbolResponse::Flat(syms))) => {
                syms.into_iter().map(|s| s.name).collect()
            }
            Ok(Some(DocumentSymbolResponse::Nested(syms))) => {
                fn collect_names(syms: Vec<DocumentSymbol>, out: &mut Vec<String>) {
                    for s in syms {
                        out.push(s.name);
                        if let Some(children) = s.children {
                            collect_names(children, out);
                        }
                    }
                }
                let mut names = Vec::new();
                collect_names(syms, &mut names);
                names
            }
            Ok(None) => Vec::new(),
            Err(e) => {
                eprintln!("document_symbol error: {e}");
                Vec::new()
            }
        }
    }

    /// Extract a filesystem path from a URI (for assertions).
    /// Returns a forward-slash path without the `file://` scheme.
    pub(crate) fn uri_to_path(uri: &Uri) -> String {
        uri.to_string()
            .strip_prefix("file:///")
            .unwrap_or_else(|| uri.as_str().strip_prefix("file://").unwrap_or(uri.as_str()))
            .replace("%3A", ":")
            .replace("%20", " ")
    }

    /// Retry-loop waiting for the provider to warm up.
    ///
    /// Probes completion at `needle + delta` and checks if `expected_label` appears.
    /// Returns `true` if the provider warms up within the retry budget, `false` on timeout.
    pub(crate) async fn wait_until_ready(
        &self,
        uri: &Uri,
        needle: &str,
        delta: usize,
        expected_label: &str,
    ) -> bool {
        let position = self.find_position(uri, needle, delta);
        for attempt in 0..5 {
            self.ensure_synced(uri).await;
            let labels = self.completion_labels(uri, position, None).await;
            if labels.contains(&expected_label.to_string()) {
                return true;
            }
            if attempt < 4 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        false
    }

    /// Shut down the type provider process.
    pub(crate) async fn shutdown(self) {
        let _ = self.provider.shutdown().await;
        self._drain_handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve an E2E fixture workspace root as a canonical path.
pub(crate) fn fixture_workspace_root(name: &str) -> String {
    let path = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packages/vue-vscode/e2e/fixtures/{name}")),
    )
    .expect("fixture workspace path should canonicalize");
    crate::test_utils::canonical_test_path(&path)
}

/// Infer a language ID from a file extension.
fn language_id_for(path: &str) -> String {
    if path.ends_with(".vue") {
        "vue".to_string()
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript".to_string()
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        "javascript".to_string()
    } else {
        "plaintext".to_string()
    }
}

// ---------------------------------------------------------------------------
// Macro
// ---------------------------------------------------------------------------

/// Generate two `#[tokio::test]` functions (one per provider) from a single async test body.
///
/// The test body is an `async fn` taking `session: &RealProviderTestSession`.
///
/// Usage:
/// ```ignore
/// real_provider_test!(test_name, fixture = "single-project", async fn run(session) {
///     let uri = session.open_fixture_file("src/App.vue").await;
///     // ...assertions...
/// });
/// ```
macro_rules! real_provider_test {
    ($name:ident, fixture = $fixture:expr, async fn $fn_name:ident ($session:ident) $body:block) => {
        paste::paste! {
            #[tokio::test(flavor = "multi_thread")]
            async fn [<$name _tsserver>]() {
                let Some(session) = $crate::test_harness::TestSessionBuilder::new(
                    $crate::test_harness::TestProviderKind::Tsserver,
                )
                .fixture($fixture)
                .build()
                .await
                else {
                    return;
                };
                async fn $fn_name($session: &$crate::test_harness::RealProviderTestSession)
                    $body
                $fn_name(&session).await;
                session.shutdown().await;
            }

            #[tokio::test(flavor = "multi_thread")]
            async fn [<$name _tsgo>]() {
                let Some(session) = $crate::test_harness::TestSessionBuilder::new(
                    $crate::test_harness::TestProviderKind::Tsgo,
                )
                .fixture($fixture)
                .build()
                .await
                else {
                    return;
                };
                async fn $fn_name($session: &$crate::test_harness::RealProviderTestSession)
                    $body
                $fn_name(&session).await;
                session.shutdown().await;
            }
        }
    };
}

pub(crate) use real_provider_test;

/// Canary assertion for known provider/harness limitations.
///
/// Asserts that the **known-broken behavior still holds**. When the limitation is fixed
/// (the condition becomes false), the canary panics — signaling the fix should be
/// promoted to a real `assert!`.
///
/// Usage: `canary_assert_known_limitation!(broken_condition, "description of limitation");`
///
/// - If `broken_condition` is true → the limitation still exists → test passes (logs a note)
/// - If `broken_condition` is false → the limitation was fixed → test **fails** with a
///   message to promote the canary to a real assertion
macro_rules! canary_assert_known_limitation {
    ($broken_cond:expr, $($arg:tt)+) => {
        if $broken_cond {
            eprintln!("  CANARY (known limitation still present): {}", format_args!($($arg)+));
        } else {
            panic!(
                "CANARY RESOLVED — limitation no longer present, promote to real assert!: {}",
                format_args!($($arg)+)
            );
        }
    };
}

pub(crate) use canary_assert_known_limitation;
