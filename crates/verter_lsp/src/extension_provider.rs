//! TypeScript type provider via the VS Code extension's in-process
//! `ts.createLanguageService()`.
//!
//! Instead of spawning a child process, this provider sends `$/verter/tsQuery`
//! requests back to the extension host over the existing LSP stdio pipe.
//! The extension handles each query synchronously in-process, avoiding TCP,
//! stdio, and process spawn overhead.
//!
//! Uses tsserver command format so all existing response parsers work unchanged.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use tokio::sync::{Mutex, OnceCell};

use crate::server::{TsQuery, TsQueryParams};
use crate::tsserver::ipc::{
    build_completion_entry_details_request, build_entry_names_entry, byte_offset_to_tsserver_pos,
    combined_code_fix_args, completion_entry_details_to_resolve_result, concat_display_parts,
    dedup_error_codes, enrich_completion_with_entry_details, format_quickinfo_hover,
    merge_diagnostic_sets, parse_tsserver_code_action, parse_tsserver_combined_code_fix,
    parse_tsserver_completion, parse_tsserver_diagnostic, parse_tsserver_location,
    parse_tsserver_rename_span, stamp_tsserver_completion_offset,
};
use crate::type_provider::protocol::*;
use crate::type_provider::traits::{ProviderFuture, TypeProvider};

/// Transport seam for the `$/verter/tsQuery` server→client request.
///
/// `ExtensionTypeProvider` talks to the VS Code extension host over a single
/// raw `command + arguments → JSON body` choke point. In production that body
/// is delivered over a concrete `tower_lsp_server::Client`
/// ([`LspTsQueryTransport`]); tests inject a scripted in-memory transport so the
/// provider's completion / resolve / diagnostics request envelopes can be
/// driven headlessly, without a live extension-host `Client`.
///
/// This is a TRANSPORT abstraction only — it carries the typed `command`/
/// `arguments` envelope and returns the raw response body. It does NOT resolve
/// types, parse responses, or duplicate any of the provider's
/// tsserver-family mapping (`parse_tsserver_completion`,
/// `completion_entry_details_to_resolve_result`, `merge_diagnostic_sets`, …)
/// which remain the single shared owner in `verter_type_runtime::tsserver::ipc`.
///
/// The trait is statically dispatched (generic injection, no `dyn`,
/// no `async_trait`, no boxed future) so the production path stays
/// zero-overhead.
pub trait TsQueryTransport: Send + Sync {
    /// Send one `$/verter/tsQuery` request and return its raw JSON response body.
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<serde_json::Value, TypeProviderError>> + Send + '_;
}

/// Production [`TsQueryTransport`] — forwards each `$/verter/tsQuery` over the
/// deferred extension-host LSP `Client`.
pub struct LspTsQueryTransport {
    /// Deferred LSP client — populated during `LspService::build()`.
    client: Arc<OnceCell<tower_lsp_server::Client>>,
}

impl TsQueryTransport for LspTsQueryTransport {
    // The trait declares an explicit `+ Send` return bound (load-bearing: the
    // future is awaited from `Send` provider methods and downstream
    // `ProviderFuture`s). `async fn` in a trait impl cannot express that bound,
    // so the explicit `impl Future + Send` form stays — clippy's `async fn`
    // suggestion would drop the requirement.
    #[allow(clippy::manual_async_fn)]
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<serde_json::Value, TypeProviderError>> + Send + '_ {
        async move {
            let client = self
                .client
                .get()
                .ok_or_else(|| TypeProviderError::new("LSP client not yet initialized"))?;
            client
                .send_request::<TsQuery>(params)
                .await
                .map_err(|e| TypeProviderError::new(format!("tsQuery failed: {e}")))
        }
    }
}

/// A `TypeProvider` that delegates to the VS Code extension's in-process
/// TypeScript language service via `$/verter/tsQuery` server→client requests.
///
/// Generic over the [`TsQueryTransport`] so production binds the concrete
/// [`LspTsQueryTransport`] (the default) while tests bind a scripted mock — the
/// completion / resolve / diagnostics request shaping is identical across both.
pub struct ExtensionTypeProvider<T = LspTsQueryTransport> {
    /// Transport for the `$/verter/tsQuery` request envelope.
    transport: T,
    /// Cached file contents for position conversion (byte offset ↔ line/col).
    contents: Arc<Mutex<HashMap<String, Arc<str>>>>,
    /// Files that have been sent to the extension via `open` command.
    opened_files: Arc<Mutex<HashSet<String>>>,
    /// Workspace root path (forward slashes).
    workspace_root: String,
    /// Per-project roots for per-file `projectRootPath` matching.
    project_roots: Arc<parking_lot::RwLock<Vec<String>>>,
}

impl ExtensionTypeProvider<LspTsQueryTransport> {
    pub fn new(client: Arc<OnceCell<tower_lsp_server::Client>>, workspace_root: &str) -> Self {
        Self::with_transport(LspTsQueryTransport { client }, workspace_root)
    }
}

impl<T: TsQueryTransport> ExtensionTypeProvider<T> {
    /// Construct a provider over an arbitrary [`TsQueryTransport`]. Production
    /// uses [`ExtensionTypeProvider::new`] (the concrete `Client`-backed
    /// transport); tests inject a scripted mock.
    pub fn with_transport(transport: T, workspace_root: &str) -> Self {
        Self {
            transport,
            contents: Arc::new(Mutex::new(HashMap::new())),
            opened_files: Arc::new(Mutex::new(HashSet::new())),
            workspace_root: verter_span::path::canonicalize_path(workspace_root),
            project_roots: Arc::new(parking_lot::RwLock::new(Vec::new())),
        }
    }

    /// Send a tsserver-format command to the extension and return the response body.
    async fn query(
        &self,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, TypeProviderError> {
        self.transport
            .ts_query(TsQueryParams {
                command: command.into(),
                arguments,
            })
            .await
    }

    fn normalize_path(path: &str) -> String {
        verter_span::path::canonicalize_path(path)
    }

    /// Share the contents-cache handle so a scripted transport can simulate a
    /// concurrent `update_file` landing mid-request, exercising the fresh
    /// per-response snapshot the edit paths take.
    #[cfg(test)]
    pub(crate) fn contents_handle_for_test(
        &self,
    ) -> Arc<Mutex<HashMap<String, Arc<str>>>> {
        Arc::clone(&self.contents)
    }

    fn project_root_for(&self, file: &str) -> String {
        let roots = self.project_roots.read();
        verter_span::path::longest_project_root(file, &roots, &self.workspace_root).to_string()
    }
}

impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T> {
    fn provider_id(&self) -> &'static str {
        "extension"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let project_root = self.project_root_for(&file);
        Box::pin(async move {
            contents_cache
                .lock()
                .await
                .insert(file.clone(), Arc::from(content.as_str()));
            opened_files.lock().await.insert(file.clone());
            self.query(
                "open",
                serde_json::json!({
                    "file": file,
                    "fileContent": content,
                    "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                        else if file.ends_with(".jsx") { "JSX" }
                        else if file.ends_with(".js") { "JS" }
                        else { "TS" },
                    "projectRootPath": project_root,
                }),
            )
            .await?;
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            contents_cache.lock().await.insert(file, content.into());
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let project_root = self.project_root_for(&file);
        Box::pin(async move {
            let old_line_count = {
                let cache = contents_cache.lock().await;
                cache.get(&file).map(|c| c.lines().count() as u32 + 1)
            };

            contents_cache
                .lock()
                .await
                .insert(file.clone(), Arc::from(content.as_str()));

            let mut opened = opened_files.lock().await;
            if opened.contains(&file) {
                drop(opened);
                if let Some(end_line) = old_line_count {
                    self.query(
                        "updateOpen",
                        serde_json::json!({
                            "changedFiles": [{
                                "fileName": file,
                                "textChanges": [{
                                    "start": { "line": 1, "offset": 1 },
                                    "end": { "line": end_line, "offset": 1 },
                                    "newText": content,
                                }]
                            }]
                        }),
                    )
                    .await?;
                } else {
                    self.query(
                        "updateOpen",
                        serde_json::json!({
                            "closedFiles": [&file],
                            "openFiles": [{
                                "file": file,
                                "fileContent": content,
                                "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                                    else if file.ends_with(".jsx") { "JSX" }
                                    else if file.ends_with(".js") { "JS" }
                                    else { "TS" },
                                "projectRootPath": project_root,
                            }]
                        }),
                    )
                    .await?;
                }
            } else {
                opened.insert(file.clone());
                drop(opened);
                self.query(
                    "open",
                    serde_json::json!({
                        "file": file,
                        "fileContent": content,
                        "scriptKindName": if file.ends_with(".tsx") { "TSX" }
                            else if file.ends_with(".jsx") { "JSX" }
                            else if file.ends_with(".js") { "JS" }
                            else { "TS" },
                        "projectRootPath": project_root,
                    }),
                )
                .await?;
            }
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        Box::pin(async move {
            contents_cache.lock().await.remove(&file);
            opened_files.lock().await.remove(&file);
            self.query("close", serde_json::json!({ "file": file }))
                .await?;
            Ok(())
        })
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let file = Self::normalize_path(path);
        let trigger = trigger_character.map(|s| s.to_string());
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let mut args = serde_json::json!({
                "file": file,
                "line": line,
                "offset": col,
                "includeExternalModuleExports": true,
                "includeInsertTextCompletions": true,
            });

            if let Some(ref t) = trigger {
                args["triggerCharacter"] = serde_json::Value::String(t.clone());
            }

            let result = self.query("completionInfo", args).await?;

            let is_incomplete = result
                .get("isMemberCompletion")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let items = result
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(parse_tsserver_completion)
                        .map(|item| stamp_tsserver_completion_offset(item, offset))
                        .collect()
                })
                .unwrap_or_default();

            Ok(CompletionResult {
                items,
                is_incomplete,
            })
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let file = Self::normalize_path(path);
        Box::pin(async move {
            if items.is_empty() {
                return Ok(Vec::new());
            }

            let (line, col) = {
                let cache = self.contents.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            // tsserver-family `completionEntryDetails` keys on the entry name plus
            // the `source`/`data` recovered from the entry's resolve handle (an
            // auto-import entry resolves against a different module than a local
            // member). The shared builder forwards the typed handle's fields so an
            // external-module entry resolves to the right symbol — identical to
            // the tsserver provider's request (review finding H4).
            let entry_names: Vec<_> = items
                .iter()
                .map(build_completion_entry_details_request)
                .collect();

            let result = self
                .query(
                    "completionEntryDetails",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "entryNames": entry_names,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let detail_map: HashMap<String, &serde_json::Value> = body
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|detail| {
                            detail
                                .get("name")
                                .and_then(|value| value.as_str())
                                .map(|name| (name.to_string(), detail))
                        })
                        .collect();
                    let enriched = items
                        .iter()
                        .map(|item| {
                            detail_map
                                .get(&item.label)
                                .map(|detail| enrich_completion_with_entry_details(item, detail))
                                .unwrap_or_else(|| item.clone())
                        })
                        .collect::<Vec<_>>();
                    Ok(enriched)
                }
                Err(_) => Ok(items.to_vec()),
            }
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "quickinfo",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let display = body
                        .get("displayString")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let docs = body
                        .get("documentation")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let kind = body
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();

                    if display.is_empty() {
                        return Ok(None);
                    }

                    let contents = format_quickinfo_hover(kind, display, docs);

                    Ok(Some(HoverInfo {
                        contents,
                        range_start: None,
                        range_end: None,
                    }))
                }
                Err(e) => {
                    tracing::warn!("extension quickinfo error for {file}: {e}");
                    Ok(None)
                }
            }
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let content = {
                let cache = contents_cache.lock().await;
                cache.get(&file).cloned()
            };

            // Pull all three tsserver-family diagnostic passes and union them:
            // SEMANTIC (type errors) + SYNTACTIC (parse errors) + SUGGESTION
            // (unused-symbol / hint findings) — the tsserver-family parity gap
            // (GAP-2). The semantic pass gates success; syntactic/suggestion
            // failures degrade that category to empty rather than failing the
            // whole pull. The union/dedup is the shared `merge_diagnostic_sets`
            // owner (one merge point, not a per-provider fork).
            let parse_body = |body: serde_json::Value| -> Vec<TypeDiagnostic> {
                body.as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| parse_tsserver_diagnostic(d, content.as_deref()))
                            .collect()
                    })
                    .unwrap_or_default()
            };

            match self
                .query(
                    "semanticDiagnosticsSync",
                    serde_json::json!({ "file": file }),
                )
                .await
            {
                Ok(semantic_body) => {
                    let semantic = parse_body(semantic_body);
                    let syntactic = self
                        .query(
                            "syntacticDiagnosticsSync",
                            serde_json::json!({ "file": file }),
                        )
                        .await
                        .ok()
                        .map(parse_body)
                        .unwrap_or_default();
                    let suggestion = self
                        .query(
                            "suggestionDiagnosticsSync",
                            serde_json::json!({ "file": file }),
                        )
                        .await
                        .ok()
                        .map(parse_body)
                        .unwrap_or_default();
                    Ok(merge_diagnostic_sets(semantic, syntactic, suggestion))
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "definition",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await?;

            // Snapshot then release before parsing: `parse_tsserver_location` can fall back to a
            // blocking disk read on a cache miss for a cross-file target. The snapshot is a cheap
            // pointer-map clone (`Arc<str>` values).
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                guard.clone()
            };
            let locs = result
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|loc| parse_tsserver_location(loc, &cache_snapshot))
                        .collect()
                })
                .unwrap_or_default();

            Ok(locs)
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "typeDefinition",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await?;

            // Snapshot then release before parsing: `parse_tsserver_location` can fall back to a
            // blocking disk read on a cache miss for a cross-file target. The snapshot is a cheap
            // pointer-map clone (`Arc<str>` values).
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                guard.clone()
            };
            let locs = result
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|loc| parse_tsserver_location(loc, &cache_snapshot))
                        .collect()
                })
                .unwrap_or_default();

            Ok(locs)
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "references",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await?;

            // Snapshot then release before parsing: `parse_tsserver_location` can fall back to a
            // blocking disk read on a cache miss for a cross-file target. The snapshot is a cheap
            // pointer-map clone (`Arc<str>` values).
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                guard.clone()
            };
            let locs = result
                .get("refs")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|loc| parse_tsserver_location(loc, &cache_snapshot))
                        .collect()
                })
                .unwrap_or_default();

            Ok(locs)
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "rename",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "findInComments": false,
                        "findInStrings": false,
                    }),
                )
                .await?;

            // Snapshot ONLY this response's target files and release the lock BEFORE parsing: a
            // rename span resolves its target through `parse_tsserver_rename_span`, which can fall
            // back to a blocking disk read on a cache miss. Holding the async mutex across that read
            // would block every other task contending for the cache. Scanning the response bounds
            // the snapshot to the files it touches.
            let target_paths =
                verter_type_runtime::contents_snapshot::tsserver_rename_target_paths(&result);
            let cache_snapshot = {
                let guard = contents_cache.lock().await;
                verter_type_runtime::contents_snapshot::targeted_contents_snapshot(
                    &guard,
                    &target_paths,
                )
            };
            let locs = {
                // Bind a `Copy` `&HashMap` so each per-target closure can capture the cache by
                // shared reference.
                let cache: &HashMap<String, Arc<str>> = &cache_snapshot;
                result
                    .get("locs")
                    .and_then(|v| v.as_array())
                    .map(|groups| {
                        groups
                            .iter()
                            .flat_map(|group| {
                                let file_path = verter_span::path::canonicalize_path(
                                    group
                                        .get("file")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default(),
                                );
                                group
                                    .get("locs")
                                    .and_then(|v| v.as_array())
                                    .into_iter()
                                    .flat_map(move |spans| {
                                        let fp = file_path.clone();
                                        spans.iter().filter_map(move |span| {
                                            parse_tsserver_rename_span(span, &fp, cache)
                                        })
                                    })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };

            Ok(locs)
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "signatureHelp",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let items = body.get("items").and_then(|v| v.as_array());
                    let Some(items) = items else {
                        return Ok(None);
                    };

                    let signatures: Vec<SignatureInfo> = items
                        .iter()
                        .map(|item| {
                            let prefix = item
                                .get("prefixDisplayParts")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts))
                                .unwrap_or_default();
                            let suffix = item
                                .get("suffixDisplayParts")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts))
                                .unwrap_or_default();
                            let separator = item
                                .get("separatorDisplayParts")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts))
                                .unwrap_or_else(|| ", ".to_string());

                            let params: Vec<ParameterInfo> = item
                                .get("parameters")
                                .and_then(|v| v.as_array())
                                .map(|ps| {
                                    ps.iter()
                                        .map(|p| {
                                            let label = p
                                                .get("displayParts")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts))
                                                .unwrap_or_default();
                                            let doc = p
                                                .get("documentation")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts));
                                            ParameterInfo {
                                                label,
                                                documentation: doc,
                                            }
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            let param_labels: Vec<String> =
                                params.iter().map(|p| p.label.clone()).collect();
                            let label =
                                format!("{prefix}{}{suffix}", param_labels.join(&separator));
                            let doc = item
                                .get("documentation")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts));

                            SignatureInfo {
                                label,
                                documentation: doc,
                                parameters: params,
                            }
                        })
                        .collect();

                    if signatures.is_empty() {
                        return Ok(None);
                    }

                    let active_sig = body
                        .get("selectedItemIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    let active_param = body
                        .get("argumentIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);

                    Ok(Some(SignatureHelp {
                        signatures,
                        active_signature: active_sig,
                        active_parameter: active_param,
                    }))
                }
                Err(_) => Ok(None),
            }
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        // Mirror the out-of-process tsserver path: key the fixes off the
        // diagnostic error codes, short-circuiting when none are numeric.
        let error_codes = dedup_error_codes(diagnostics);
        Box::pin(async move {
            if error_codes.is_empty() {
                return Ok(vec![]);
            }
            let (sl, sc, el, ec) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => {
                        let (sl, sc) = byte_offset_to_tsserver_pos(c, start_offset);
                        let (el, ec) = byte_offset_to_tsserver_pos(c, end_offset);
                        (sl, sc, el, ec)
                    }
                    None => (1, start_offset + 1, 1, end_offset + 1),
                }
            };

            let result = self
                .query(
                    "getCodeFixes",
                    serde_json::json!({
                        "file": file,
                        "startLine": sl,
                        "startOffset": sc,
                        "endLine": el,
                        "endOffset": ec,
                        "errorCodes": error_codes,
                    }),
                )
                .await;

            let raw_fixes = match result {
                Ok(body) => body.as_array().cloned().unwrap_or_default(),
                Err(_) => return Ok(vec![]),
            };

            // Snapshot ONLY the files these single-fix actions target, then release the lock BEFORE
            // parsing: `parse_tsserver_code_action` can fall back to a blocking disk read on a cache
            // miss. Holding the async mutex across those reads would block every other task
            // contending for the cache. Scanning the responses bounds the snapshot to touched files.
            let mut single_fix_paths: HashSet<String> = HashSet::new();
            for fix in &raw_fixes {
                single_fix_paths.extend(
                    verter_type_runtime::contents_snapshot::tsserver_code_action_target_paths(fix),
                );
            }
            let single_fix_snapshot = {
                let guard = contents_cache.lock().await;
                verter_type_runtime::contents_snapshot::targeted_contents_snapshot(
                    &guard,
                    &single_fix_paths,
                )
            };

            // Single-fix actions first, then their combined "fix all" companions.
            let mut actions: Vec<TypeCodeAction> = raw_fixes
                .iter()
                .filter_map(|a| parse_tsserver_code_action(a, &single_fix_snapshot))
                .collect();

            // Follow each DISTINCT combinable `fixId` once (e.g. "Delete all unused
            // declarations"), titled from the fix's typed `fixAllDescription` —
            // never a title-string match.
            let mut combined: Vec<TypeCodeAction> = Vec::new();
            let mut seen_fix_ids: HashSet<String> = HashSet::new();
            for fix in &raw_fixes {
                let Some(fix_id) = fix.get("fixId").and_then(|v| v.as_str()) else {
                    continue;
                };
                if fix_id.is_empty() || !seen_fix_ids.insert(fix_id.to_string()) {
                    continue;
                }
                let fix_all_title = fix
                    .get("fixAllDescription")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                if let Ok(body) = self
                    .query("getCombinedCodeFix", combined_code_fix_args(&file, fix_id))
                    .await
                {
                    // Snapshot ONLY this combined response's target files, taken FRESH after the
                    // await so it reflects content current as of this response (a concurrent
                    // `update_file` during the await must not convert offsets against stale text).
                    let target_paths =
                        verter_type_runtime::contents_snapshot::tsserver_combined_code_fix_target_paths(
                            &body,
                        );
                    let combined_snapshot = {
                        let guard = contents_cache.lock().await;
                        verter_type_runtime::contents_snapshot::targeted_contents_snapshot(
                            &guard,
                            &target_paths,
                        )
                    };
                    if let Some(action) = parse_tsserver_combined_code_fix(
                        &body,
                        fix_all_title.as_deref(),
                        &combined_snapshot,
                    ) {
                        combined.push(action);
                    }
                }
            }

            actions.extend(combined);
            Ok(actions)
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let content = {
                let cache = contents_cache.lock().await;
                cache.get(&file).cloned()
            };
            let Some(content) = content else {
                return Ok(vec![]);
            };
            let end_line = content.lines().count() as u32 + 1;

            let result = self
                .query(
                    "encodedSemanticClassifications-full",
                    serde_json::json!({
                        "file": file,
                        "start": { "line": 1, "offset": 1 },
                        "end": { "line": end_line, "offset": 1 },
                        "format": "2020",
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let spans = body
                        .get("spans")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    let mut tokens = Vec::new();
                    let mut i = 0;
                    while i + 2 < spans.len() {
                        let start = spans[i].as_u64().unwrap_or(0) as u32;
                        let length = spans[i + 1].as_u64().unwrap_or(0) as u32;
                        let classification = spans[i + 2].as_u64().unwrap_or(0) as u32;
                        let token_type = classification & 0xFF;
                        let token_modifiers = (classification >> 8) & 0xFF;
                        tokens.push(SemanticToken {
                            start,
                            length,
                            token_type,
                            token_modifiers,
                        });
                        i += 3;
                    }

                    Ok(tokens)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (line, col) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let result = self
                .query(
                    "documentHighlights",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "filesToSearch": [file],
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let highlights = body
                        .as_array()
                        .into_iter()
                        .flat_map(|groups| {
                            groups.iter().flat_map(|group| {
                                group
                                    .get("highlightSpans")
                                    .and_then(|v| v.as_array())
                                    .into_iter()
                                    .flat_map(|spans| {
                                        spans.iter().filter_map(|span| {
                                            let start = span.get("start")?;
                                            let end = span.get("end")?;
                                            let sl = start.get("line")?.as_u64()? as u32;
                                            let so = start.get("offset")?.as_u64()? as u32;
                                            let el = end.get("line")?.as_u64()? as u32;
                                            let eo = end.get("offset")?.as_u64()? as u32;
                                            let kind = span
                                                .get("kind")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("none");
                                            let hl_kind = match kind {
                                                "writtenReference" => {
                                                    TypeDocumentHighlightKind::Write
                                                }
                                                _ => TypeDocumentHighlightKind::Read,
                                            };
                                            let s = ((sl.saturating_sub(1)) << 16)
                                                | ((so.saturating_sub(1)) & 0xFFFF);
                                            let e = ((el.saturating_sub(1)) << 16)
                                                | ((eo.saturating_sub(1)) & 0xFFFF);
                                            Some(TypeDocumentHighlight {
                                                start: s,
                                                end: e,
                                                kind: hl_kind,
                                            })
                                        })
                                    })
                            })
                        })
                        .collect();

                    Ok(highlights)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        Box::pin(async move {
            let (sl, _sc, el, _ec) = {
                let cache = contents_cache.lock().await;
                match cache.get(&file) {
                    Some(c) => {
                        let (sl, sc) = byte_offset_to_tsserver_pos(c, start_offset);
                        let (el, ec) = byte_offset_to_tsserver_pos(c, end_offset);
                        (sl, sc, el, ec)
                    }
                    None => (1, start_offset + 1, 1, end_offset + 1),
                }
            };

            let result = self
                .query(
                    "provideInlayHints",
                    serde_json::json!({
                        "file": file,
                        "start": sl,
                        "length": (el.saturating_sub(sl) + 1) * 200,
                    }),
                )
                .await;

            match result {
                Ok(body) => {
                    let hints = body
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|hint| {
                                    let text = hint.get("text")?.as_str()?.to_string();
                                    let pos = hint.get("position")?;
                                    let hl = pos.get("line")?.as_u64()? as u32;
                                    let ho = pos.get("offset")?.as_u64()? as u32;

                                    let kind_str =
                                        hint.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                                    let kind = match kind_str {
                                        "Type" => Some(InlayHintKind::Type),
                                        "Parameter" => Some(InlayHintKind::Parameter),
                                        _ => None,
                                    };

                                    let position = ((hl.saturating_sub(1)) << 16)
                                        | ((ho.saturating_sub(1)) & 0xFFFF);

                                    Some(InlayHint {
                                        position,
                                        label: text,
                                        kind,
                                        padding_left: hint
                                            .get("whitespaceBefore")
                                            .and_then(|v| v.as_bool()),
                                        padding_right: hint
                                            .get("whitespaceAfter")
                                            .and_then(|v| v.as_bool()),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    Ok(hints)
                }
                Err(_) => Ok(vec![]),
            }
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let file = Self::normalize_path(path);
        Box::pin(async move {
            // The extension is a tsserver-family provider — it resolves through
            // `completionEntryDetails`. A non-tsserver resolve key cannot have
            // come from this provider, so fail closed.
            let CompletionResolveData::TsserverEntry {
                name,
                source,
                data,
                offset,
            } = data
            else {
                return Ok(None);
            };

            // Re-issue at the SAME completion-site position the entry came from;
            // tsserver keys the entry's auto-import `codeActions` on
            // (position, name, source/data).
            let (line, col) = {
                let cache = self.contents.lock().await;
                match cache.get(&file) {
                    Some(c) => byte_offset_to_tsserver_pos(c, offset),
                    None => (1, offset + 1),
                }
            };

            let entry = build_entry_names_entry(&name, source.as_deref(), data.as_ref());

            let result = self
                .query(
                    "completionEntryDetails",
                    serde_json::json!({
                        "file": file,
                        "line": line,
                        "offset": col,
                        "entryNames": [entry],
                    }),
                )
                .await?;

            let Some(detail) = result.as_array().and_then(|arr| arr.first()) else {
                return Ok(None);
            };
            let contents_cache = self.contents.lock().await.clone();
            Ok(completion_entry_details_to_resolve_result(
                detail,
                &file,
                &contents_cache,
            ))
        })
    }

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        let base_url = base_url.to_string();
        // The Svelte IDE-projection assets (the `@verter/svelte-jsx` shim +
        // transitive `svelte` rows) are injected at the COMMON
        // per-owner-project path-config call site in `background_init` (so
        // EVERY provider — extension / TSGO / tsserver — receives them, keyed
        // to the owner project root), NOT here in a single provider.
        Box::pin(async move {
            let mut options = serde_json::json!({
                "module": "esnext",
                "target": "esnext",
                "moduleResolution": "bundler",
                "jsx": "preserve",
                "jsxImportSource": "vue",
                "allowImportingTsExtensions": true,
                "allowJs": true,
                "checkJs": true,
                "strict": true,
                "allowArbitraryExtensions": true,
                "baseUrl": base_url,
                "paths": paths,
            });
            if options.get("paths").is_some_and(|v| v.is_null()) {
                if let Some(obj) = options.as_object_mut() {
                    obj.remove("paths");
                }
            }
            let _ = self
                .query(
                    "compilerOptionsForInferredProjects",
                    serde_json::json!({ "options": options }),
                )
                .await;
            Ok(())
        })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let project_roots = Arc::clone(&self.project_roots);
        Box::pin(async move {
            let mut roots = project_roots.write();

            for folder in &removed {
                if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                    // `uri_to_canonical_id_from_str` already routes through the
                    // canonical owner — no second canonicalization needed.
                    let canonical = crate::documents::uri_to_canonical_id_from_str(uri);
                    roots.retain(|r| r != &canonical);
                }
            }

            for folder in &added {
                if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                    let canonical = crate::documents::uri_to_canonical_id_from_str(uri);
                    if !roots.contains(&canonical) {
                        roots.push(canonical);
                    }
                }
            }

            roots.sort_by_key(|r| std::cmp::Reverse(r.len()));

            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "extension_provider_tests.rs"]
mod tests;
