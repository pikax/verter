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
use std::sync::Arc;

use tokio::sync::Mutex;

use verter_type_runtime::codec::SourceIndex;
use verter_type_runtime::provider_query::{ConflictKind, ProviderQueryConflict};

use crate::server::TsQueryParams;
use crate::tsserver::ipc::{
    assemble_signature_label, build_completion_entry_details_request, build_entry_names_entry,
    byte_offset_to_tsserver_absolute_offset, byte_offset_to_tsserver_pos, combined_code_fix_args,
    completion_entry_details_to_resolve_result, concat_display_parts, dedup_error_codes,
    enrich_completion_with_entry_details, format_quickinfo_hover, merge_diagnostic_sets,
    parse_tsserver_code_action, parse_tsserver_combined_code_fix, parse_tsserver_completion,
    parse_tsserver_diagnostic, parse_tsserver_highlight_span, parse_tsserver_inlay_hint,
    parse_tsserver_locations, parse_tsserver_rename_spans, quickinfo_wire_pos_to_byte_offset,
    stamp_tsserver_completion_offset,
};
use crate::type_provider::protocol::*;
use crate::type_provider::traits::{
    ConfiguredOwnerAuthority, ProviderFuture, ProviderQuery, TypeProvider,
};

#[path = "extension_provider_binding.rs"]
mod binding;
#[path = "extension_provider_transport.rs"]
mod transport;

pub use transport::{LspTsQueryTransport, TsQueryTransport};

use binding::FileProjectBinding;

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
    /// The extension language service's application receipts: per file, the
    /// bytes it acknowledged and the newest delivery issued to it. The contents
    /// cache above runs ahead of the extension (it is written before the
    /// request is sent, and a `load_file` never sends anything), so it is not
    /// evidence of what the service holds.
    applied: Arc<parking_lot::Mutex<DeliveryLedger>>,
    /// Workspace root path (forward slashes).
    workspace_root: String,
    /// Per-project roots for per-file `projectRootPath` matching.
    project_roots: Arc<parking_lot::RwLock<Vec<String>>>,
    /// The configured-project ownership authority, installed by init once the
    /// exact workspace snapshot is built. Until then only the editor's
    /// workspace folders are known.
    ownership: Arc<parking_lot::RwLock<Option<Arc<dyn ConfiguredOwnerAuthority>>>>,
}

/// The extension language service's application receipts.
///
/// Every content delivery replaces the service's whole buffer and is issued a
/// ticket from one monotonic sequence. Only the settlement of a file's NEWEST
/// issued delivery touches its receipt: an older acknowledgement landing after
/// a newer write was issued certifies nothing, even when its bytes equal the
/// newest ones, and an older failure withdraws nothing — the newer write
/// replaces whatever the older one left and settles the receipt itself. Issuing
/// different bytes withdraws the receipt at once — from the moment the write
/// may reach the service its bytes are unknown — so a delivery whose future is
/// dropped before it settles leaves no receipt behind. The newest-delivery
/// check and the receipt publication are one critical section.
#[derive(Default)]
struct DeliveryLedger {
    /// The last ticket issued, across every file.
    issued: u64,
    files: HashMap<String, FileReceipt>,
}

/// One file's application receipt.
struct FileReceipt {
    /// The newest delivery issued for the file.
    newest: u64,
    /// The bytes the service acknowledged, while no delivery of other bytes is
    /// unsettled.
    applied: Option<Arc<str>>,
}

/// One issued content delivery, settled once the extension answers it.
struct DeliveryTicket {
    file: String,
    seq: u64,
    content: Arc<str>,
}

impl DeliveryLedger {
    /// Issue a delivery of `content` for `file`. The receipt survives only when
    /// it already certifies exactly these bytes: the service holds them whether
    /// or not this delivery lands.
    fn issue(&mut self, file: &str, content: &Arc<str>) -> DeliveryTicket {
        self.issued += 1;
        let seq = self.issued;
        let receipt = self.files.entry(file.to_string()).or_insert(FileReceipt {
            newest: seq,
            applied: None,
        });
        receipt.newest = seq;
        if receipt
            .applied
            .as_ref()
            .is_some_and(|applied| applied != content)
        {
            receipt.applied = None;
        }
        DeliveryTicket {
            file: file.to_string(),
            seq,
            content: Arc::clone(content),
        }
    }

    /// Settle `ticket`. Only the file's newest delivery settles anything — a
    /// ticket issued before a newer delivery, or before a close, is inert. An
    /// acknowledgement certifies its bytes; a failure leaves the service's bytes
    /// unknown, so it withdraws the receipt.
    fn settle(&mut self, ticket: DeliveryTicket, acknowledged: bool) {
        let Some(receipt) = self.files.get_mut(&ticket.file) else {
            return;
        };
        if receipt.newest != ticket.seq {
            return;
        }
        receipt.applied = acknowledged.then_some(ticket.content);
    }

    /// Forget `file`: a closed file holds nothing, and no delivery issued
    /// before the close can certify bytes after it.
    fn close(&mut self, file: &str) {
        self.files.remove(file);
    }

    fn applied(&self, file: &str) -> Option<&Arc<str>> {
        self.files.get(file)?.applied.as_ref()
    }

    /// Every file's receipt now: the newest delivery issued for it and the
    /// bytes the service acknowledged, for each file that has them.
    fn receipts(&self) -> HashMap<String, (u64, Arc<str>)> {
        self.files
            .iter()
            .filter_map(|(file, receipt)| {
                let applied = receipt.applied.as_ref()?;
                Some((file.clone(), (receipt.newest, Arc::clone(applied))))
            })
            .collect()
    }

    /// Whether `file`'s receipt is still exactly `bytes` under the delivery
    /// `newest`: no delivery of it was issued, failed or closed since.
    fn unchanged(&self, file: &str, newest: u64, bytes: &Arc<str>) -> bool {
        self.files.get(file).is_some_and(|receipt| {
            receipt.newest == newest
                && receipt
                    .applied
                    .as_ref()
                    .is_some_and(|applied| Arc::ptr_eq(applied, bytes))
        })
    }
}

/// The application receipts one query was sent under: every file the
/// extension had acknowledged, with the newest delivery issued for it and the
/// bytes it acknowledged. The request position converts against these bytes
/// and every range in the answer decodes through them; the answer stands only
/// while no file it decoded through has been issued another delivery since.
/// A file with no receipt — never delivered, mid-delivery, or only loaded into
/// the local cache — is read by the service itself, so a query on it, or an
/// answer locating anything in it, is a typed conflict.
struct ReceiptBinding {
    query: ProviderQuery,
    file: String,
    receipts: HashMap<String, (u64, Arc<str>)>,
}

impl ReceiptBinding {
    /// Bind `query` on `file` to the receipts held now; the bytes its request
    /// position converts against.
    fn bind(
        ledger: &parking_lot::Mutex<DeliveryLedger>,
        query: &ProviderQuery,
        file: &str,
    ) -> Result<(Self, Arc<str>), ProviderQueryConflict> {
        let receipts = ledger.lock().receipts();
        let Some((_, requested)) = receipts.get(file) else {
            return Err(ProviderQueryConflict::new(file, ConflictKind::Undelivered));
        };
        let requested = Arc::clone(requested);
        query.check_intended(file, Some(&requested))?;
        Ok((
            Self {
                query: query.clone(),
                file: file.to_string(),
                receipts,
            },
            requested,
        ))
    }

    /// The bytes the request file and each of `targets` decode through, once
    /// every one of their receipts is confirmed unchanged since the query was
    /// sent. A target the requester captured a surface for must decode through
    /// exactly that surface.
    fn settle(
        &self,
        ledger: &parking_lot::Mutex<DeliveryLedger>,
        targets: impl IntoIterator<Item = String>,
    ) -> Result<HashMap<String, Arc<str>>, ProviderQueryConflict> {
        let mut decoded = HashMap::new();
        for path in std::iter::once(self.file.clone()).chain(targets) {
            if decoded.contains_key(&path) {
                continue;
            }
            let Some((_, bytes)) = self.receipts.get(&path) else {
                return Err(ProviderQueryConflict::new(&path, ConflictKind::Undelivered));
            };
            if path != self.file {
                self.query.check_intended_target(&path, Some(bytes))?;
            }
            decoded.insert(path, Arc::clone(bytes));
        }
        let ledger = ledger.lock();
        for path in decoded.keys() {
            let (newest, bytes) = &self.receipts[path];
            if !ledger.unchanged(path, *newest, bytes) {
                return Err(ProviderQueryConflict::new(path, ConflictKind::Moved));
            }
        }
        Ok(decoded)
    }
}

/// The `open` / `updateOpen`-`openFiles` entry that sets `file`'s whole
/// buffer to `content` under its declared project.
fn open_entry(
    file: &str,
    content: &str,
    project_root: &str,
    project_config: Option<&str>,
) -> serde_json::Value {
    let script_kind = if file.ends_with(".tsx") {
        "TSX"
    } else if file.ends_with(".jsx") {
        "JSX"
    } else if file.ends_with(".js") {
        "JS"
    } else {
        "TS"
    };
    serde_json::json!({
        "file": file,
        "fileContent": content,
        "scriptKindName": script_kind,
        "projectRootPath": project_root,
        "projectConfigPath": project_config,
    })
}

impl ExtensionTypeProvider<LspTsQueryTransport> {
    pub fn new(client: crate::outbound::Outbound, workspace_root: &str) -> Self {
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
            applied: Arc::new(parking_lot::Mutex::new(DeliveryLedger::default())),
            workspace_root: verter_span::path::canonicalize_path(workspace_root),
            project_roots: Arc::new(parking_lot::RwLock::new(Vec::new())),
            ownership: Arc::new(parking_lot::RwLock::new(None)),
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

    /// Bind `query` on `file` to the extension's receipts now.
    fn bind(
        &self,
        query: &ProviderQuery,
        file: &str,
    ) -> Result<(ReceiptBinding, Arc<str>), ProviderQueryConflict> {
        ReceiptBinding::bind(&self.applied, query, file)
    }

    /// Issue one content delivery of `file` to the extension; settle it with
    /// [`DeliveryLedger::settle`] once the extension answers.
    fn issue_delivery(&self, file: &str, content: &Arc<str>) -> DeliveryTicket {
        self.applied.lock().issue(file, content)
    }

    /// Share the contents-cache handle so a scripted transport can simulate a
    /// concurrent `update_file` landing mid-request, exercising the fresh
    /// per-response snapshot the edit paths take.
    #[cfg(test)]
    pub(crate) fn contents_handle_for_test(&self) -> Arc<Mutex<HashMap<String, Arc<str>>>> {
        Arc::clone(&self.contents)
    }
}

impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T> {
    fn provider_id(&self) -> &'static str {
        "extension"
    }

    /// The bytes the extension's language service acknowledged for `path`.
    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        use verter_type_runtime::traits::AppliedContent;
        match self.applied.lock().applied(&Self::normalize_path(path)) {
            Some(bytes) => AppliedContent::Applied(Arc::clone(bytes)),
            None => AppliedContent::NotApplied,
        }
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let content = content.to_string();
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let declared = self.declared_project_for(&file);
        Box::pin(async move {
            // Fail closed BEFORE recording the file as open: an unowned file has
            // no project, so nothing may be declared for it and no later query
            // may believe it is live in one.
            let (project_root, project_config) = declared?;
            let issued: Arc<str> = Arc::from(content.as_str());
            contents_cache
                .lock()
                .await
                .insert(file.clone(), Arc::clone(&issued));
            opened_files.lock().await.insert(file.clone());
            let ticket = self.issue_delivery(&file, &issued);
            let delivered = self
                .query(
                    "open",
                    open_entry(&file, &content, &project_root, project_config.as_deref()),
                )
                .await;
            self.applied.lock().settle(ticket, delivered.is_ok());
            delivered?;
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
        let declared = self.declared_project_for(&file);
        Box::pin(async move {
            let (project_root, project_config) = declared?;
            let issued: Arc<str> = Arc::from(content.as_str());
            contents_cache
                .lock()
                .await
                .insert(file.clone(), Arc::clone(&issued));

            // Both arms replace the service's WHOLE buffer with `content`. A
            // ranged edit would have to name the end of the buffer the service
            // holds, which no local state knows: the contents cache runs ahead of
            // the service (`load_file` and refused deliveries write it), and the
            // receipt is withdrawn whenever an unsettled write may have landed.
            let entry = open_entry(&file, &content, &project_root, project_config.as_deref());
            let mut opened = opened_files.lock().await;
            let ticket = self.issue_delivery(&file, &issued);
            let delivered = if opened.contains(&file) {
                drop(opened);
                self.query("updateOpen", serde_json::json!({ "openFiles": [entry] }))
                    .await
            } else {
                opened.insert(file.clone());
                drop(opened);
                self.query("open", entry).await
            };
            self.applied.lock().settle(ticket, delivered.is_ok());
            delivered?;
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let file = Self::normalize_path(path);
        let contents_cache = Arc::clone(&self.contents);
        let opened_files = Arc::clone(&self.opened_files);
        let applied = Arc::clone(&self.applied);
        Box::pin(async move {
            contents_cache.lock().await.remove(&file);
            opened_files.lock().await.remove(&file);
            applied.lock().close(&file);
            self.query("close", serde_json::json!({ "file": file }))
                .await?;
            Ok(())
        })
    }

    fn get_completions(
        &self,
        query: &ProviderQuery,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        let trigger = trigger_character.map(|s| s.to_string());
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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
            binding.settle(&self.applied, [])?;

            Ok(CompletionResult {
                items,
                is_incomplete,
            })
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        query: &'a ProviderQuery,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            if items.is_empty() {
                return Ok(Vec::new());
            }

            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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
                    binding.settle(&self.applied, [])?;
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
                // A failed enrichment is NOT "these items have no details". The
                // list was produced by whichever project owned the file when
                // `completionInfo` ran, and this request can land after the file
                // has been re-declared to another project (an ownership
                // authority arriving mid-session, a config change, a resync).
                // Returning the original items then serves the FORMER owner's
                // answer under the current binding — a cross-project stale
                // result, and the refusal that should have disabled the feature
                // disappears. Propagate it.
                Err(e) => Err(e),
            }
        })
    }

    fn get_hover(
        &self,
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Option<HoverInfo>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        let witness = self.provider_wire_witness();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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

                    binding.settle(&self.applied, [])?;
                    if display.is_empty() {
                        return Ok(None);
                    }

                    let contents = format_quickinfo_hover(kind, display, docs);

                    // The quickinfo body's `start`/`end` are lines and columns
                    // in the bytes the request was converted against.
                    let index = SourceIndex::new_utf16(&requested);
                    let range_start = quickinfo_wire_pos_to_byte_offset(&index, body.get("start"));
                    let range_end = quickinfo_wire_pos_to_byte_offset(&index, body.get("end"));

                    Ok(Some(HoverInfo {
                        contents,
                        range_start,
                        range_end,
                        display_signature: Some(DisplaySignature::from_provider_wire(
                            witness, display,
                        )),
                        kind: QuickInfoKind::from_tsserver_wire(kind),
                        documentation: (!docs.is_empty()).then(|| docs.to_string()),
                    }))
                }
                // A refused project is not "no hover here". The extension host
                // throws when it cannot serve the file's project (no workspace
                // TypeScript, or a library-less one), and that refusal is the
                // provider's fail-closed contract — swallowing it into `None`
                // makes a disabled provider indistinguishable from a position
                // with nothing to say.
                Err(e) => Err(e),
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
            // One index for all three passes: they resolve against the same
            // content snapshot, so the document is scanned once for the whole
            // pull rather than twice per diagnostic per pass.
            let index = content.as_deref().map(SourceIndex::new_utf16);
            let parse_body = |body: serde_json::Value| -> Vec<TypeDiagnostic> {
                body.as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| {
                                parse_tsserver_diagnostic(d, index.as_ref(), Some(file.as_str()))
                            })
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
                // The SEMANTIC pass gates the pull (the syntactic/suggestion
                // passes above degrade independently). Its failure is a refusal
                // to serve, not a clean bill of health — reporting zero
                // diagnostics for a project the host refused is the worst
                // possible silent wrong answer.
                Err(e) => Err(e),
            }
        })
    }

    fn get_definition(
        &self,
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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

            // Every location decodes through its file's receipt as the query was
            // sent under it.
            let targets = result
                .as_array()
                .or_else(|| result.get("refs").and_then(|v| v.as_array()))
                .map(|arr| {
                    verter_type_runtime::contents_snapshot::tsserver_location_target_paths(arr)
                })
                .unwrap_or_default();
            let cache_snapshot = binding.settle(&self.applied, targets)?;
            let locs = result
                .as_array()
                .map(|arr| parse_tsserver_locations(arr, &cache_snapshot))
                .unwrap_or_default();

            Ok(locs)
        })
    }

    fn get_type_definition(
        &self,
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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

            // Every location decodes through its file's receipt as the query was
            // sent under it.
            let targets = result
                .as_array()
                .or_else(|| result.get("refs").and_then(|v| v.as_array()))
                .map(|arr| {
                    verter_type_runtime::contents_snapshot::tsserver_location_target_paths(arr)
                })
                .unwrap_or_default();
            let cache_snapshot = binding.settle(&self.applied, targets)?;
            let locs = result
                .as_array()
                .map(|arr| parse_tsserver_locations(arr, &cache_snapshot))
                .unwrap_or_default();

            Ok(locs)
        })
    }

    fn get_references(
        &self,
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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

            // Every location decodes through its file's receipt as the query was
            // sent under it.
            let targets = result
                .as_array()
                .or_else(|| result.get("refs").and_then(|v| v.as_array()))
                .map(|arr| {
                    verter_type_runtime::contents_snapshot::tsserver_location_target_paths(arr)
                })
                .unwrap_or_default();
            let cache_snapshot = binding.settle(&self.applied, targets)?;
            let locs = result
                .get("refs")
                .and_then(|v| v.as_array())
                .map(|arr| parse_tsserver_locations(arr, &cache_snapshot))
                .unwrap_or_default();

            Ok(locs)
        })
    }

    fn get_rename_locations(
        &self,
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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

            // Every edit decodes through its target's receipt as the query was sent
            // under it — only the files the response touches.
            let target_paths =
                verter_type_runtime::contents_snapshot::tsserver_rename_target_paths(&result);
            let cache_snapshot = binding.settle(&self.applied, target_paths)?;
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
                                let spans = group
                                    .get("locs")
                                    .and_then(|v| v.as_array())
                                    .map_or(&[][..], Vec::as_slice);
                                parse_tsserver_rename_spans(spans, &file_path, cache)
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
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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
                    binding.settle(&self.applied, [])?;
                    let items = body.get("items").and_then(|v| v.as_array());
                    let Some(items) = items else {
                        return Ok(None);
                    };

                    // tsserver gives a single top-level active param
                    // (`argumentIndex`) + active signature (`selectedItemIndex`),
                    // not per-overload values; read both up front so each signature
                    // can stamp the active param onto the SELECTED overload only.
                    let active_sig = body
                        .get("selectedItemIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    let active_param = body
                        .get("argumentIndex")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);

                    let signatures: Vec<SignatureInfo> = items
                        .iter()
                        .enumerate()
                        .map(|(sig_idx, item)| {
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

                            // Collect each parameter's display text + docs first; the
                            // text is exactly what occupies the param's slot in the
                            // assembled label, so offsets computed from it are exact.
                            let param_parts: Vec<(String, Option<String>)> = item
                                .get("parameters")
                                .and_then(|v| v.as_array())
                                .map(|ps| {
                                    ps.iter()
                                        .map(|p| {
                                            let text = p
                                                .get("displayParts")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts))
                                                .unwrap_or_default();
                                            let doc = p
                                                .get("documentation")
                                                .and_then(|v| v.as_array())
                                                .map(|parts| concat_display_parts(parts));
                                            (text, doc)
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            // Borrow each param's text in place (no clone); the
                            // assembler builds label + per-param UTF-16 offset spans
                            // in one pass. LSP offsets are UTF-16 code units; offsets
                            // let the client bold the exact active-parameter span.
                            let param_labels: Vec<&str> =
                                param_parts.iter().map(|(t, _)| t.as_str()).collect();
                            let assembled = assemble_signature_label(
                                &prefix,
                                &param_labels,
                                &separator,
                                &suffix,
                            );
                            let params: Vec<ParameterInfo> = param_parts
                                .into_iter()
                                .zip(assembled.param_offsets.iter())
                                .map(|((_, doc), &(start, end))| ParameterInfo {
                                    label: ParameterLabelKind::Offsets(start, end),
                                    documentation: doc,
                                })
                                .collect();
                            let doc = item
                                .get("documentation")
                                .and_then(|v| v.as_array())
                                .map(|parts| concat_display_parts(parts));

                            // Stamp top-level active param onto the selected overload
                            // only; tsserver gives no per-overload active params.
                            let sig_active_param = if active_sig == Some(sig_idx as u32) {
                                active_param
                            } else {
                                None
                            };

                            SignatureInfo {
                                label: assembled.label,
                                documentation: doc,
                                parameters: params,
                                active_parameter: sig_active_param,
                            }
                        })
                        .collect();

                    if signatures.is_empty() {
                        return Ok(None);
                    }

                    Ok(Some(SignatureHelp {
                        signatures,
                        active_signature: active_sig,
                        active_parameter: active_param,
                    }))
                }
                Err(e) => Err(e),
            }
        })
    }

    fn get_code_actions(
        &self,
        query: &ProviderQuery,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        // Mirror the out-of-process tsserver path: key the fixes off the
        // diagnostic error codes, short-circuiting when none are numeric.
        let error_codes = dedup_error_codes(diagnostics);
        Box::pin(async move {
            if error_codes.is_empty() {
                return Ok(vec![]);
            }
            let (binding, requested) = self.bind(&query, &file)?;
            let (sl, sc) = byte_offset_to_tsserver_pos(&requested, start_offset);
            let (el, ec) = byte_offset_to_tsserver_pos(&requested, end_offset);

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
                // `getCodeFixes` is the PRIMARY query for this feature (the
                // per-`fixId` combined follow-ups below are the secondary pass and
                // degrade on their own). A refused project answered as an empty
                // fix list is an empty lightbulb where the user's quick fix should
                // be — the refusal reads as "nothing is available here".
                Err(e) => return Err(e),
            };

            // Every single-fix edit decodes through its target's receipt as the query was sent
            // under it — only the files these actions touch.
            let mut single_fix_paths: HashSet<String> = HashSet::new();
            for fix in &raw_fixes {
                single_fix_paths.extend(
                    verter_type_runtime::contents_snapshot::tsserver_code_action_target_paths(fix),
                );
            }
            let single_fix_snapshot = binding.settle(&self.applied, single_fix_paths)?;

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
                // Each combined request is its own query: its edits name the
                // bytes held when it is sent, and a follow-up whose files are
                // not bound to the receipts it was sent under is skipped.
                let Ok((combined_binding, _)) = self.bind(&query, &file) else {
                    continue;
                };
                if let Ok(body) = self
                    .query("getCombinedCodeFix", combined_code_fix_args(&file, fix_id))
                    .await
                {
                    let target_paths =
                        verter_type_runtime::contents_snapshot::tsserver_combined_code_fix_target_paths(
                            &body,
                        );
                    let Ok(combined_snapshot) =
                        combined_binding.settle(&self.applied, target_paths)
                    else {
                        continue;
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

    fn get_semantic_tokens(&self, query: &ProviderQuery) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, content) = self.bind(&query, &file)?;
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
                    binding.settle(&self.applied, [])?;
                    let spans = body
                        .get("spans")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    // Same span payload as the managed tsserver lane: `"2020"`
                    // packed classification triplets (the extension host's
                    // bridge accepts the line/offset REQUEST shape and calls
                    // `getEncodedSemanticClassifications` itself, but the
                    // RESPONSE spans are the language service's raw UTF-16
                    // offsets either way). The shared owner
                    // (`verter_type_runtime::semantic_tokens`) decodes, remaps
                    // into Verter's published legend space (unmappable
                    // classifications drop their span, fail closed), and
                    // converts the offsets to bytes through the cached text.
                    Ok(
                        verter_type_runtime::semantic_tokens::map_classified_spans_2020(
                            &spans,
                            Some(&content),
                        ),
                    )
                }
                Err(e) => Err(e),
            }
        })
    }

    fn get_document_highlights(
        &self,
        query: &ProviderQuery,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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
                    // Spans are lines and columns in the bytes the request was
                    // converted against.
                    binding.settle(&self.applied, [])?;
                    let index = SourceIndex::new_utf16(&requested);
                    Ok(body
                        .as_array()
                        .into_iter()
                        .flatten()
                        .flat_map(|group| {
                            group
                                .get("highlightSpans")
                                .and_then(|v| v.as_array())
                                .into_iter()
                                .flatten()
                        })
                        .filter_map(|span| parse_tsserver_highlight_span(span, &index))
                        .collect())
                }
                Err(e) => Err(e),
            }
        })
    }

    fn get_inlay_hints(
        &self,
        query: &ProviderQuery,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
        Box::pin(async move {
            let (binding, requested) = self.bind(&query, &file)?;
            let Some(start) = byte_offset_to_tsserver_absolute_offset(&requested, start_offset)
            else {
                return Ok(vec![]);
            };
            let Some(end) = byte_offset_to_tsserver_absolute_offset(&requested, end_offset) else {
                return Ok(vec![]);
            };
            let Some(length) = end.checked_sub(start) else {
                return Ok(vec![]);
            };

            let result = self
                .query(
                    "provideInlayHints",
                    serde_json::json!({
                        "file": file,
                        "start": start,
                        "length": length,
                    }),
                )
                .await;

            // One index for the whole hint batch.
            let index = Some(SourceIndex::new_utf16(&requested));
            match result {
                Ok(body) => {
                    binding.settle(&self.applied, [])?;
                    let hints = body
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|hint| parse_tsserver_inlay_hint(hint, index.as_ref()))
                                .collect()
                        })
                        .unwrap_or_default();

                    Ok(hints)
                }
                Err(e) => Err(e),
            }
        })
    }

    fn resolve_completion(
        &self,
        query: &ProviderQuery,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let file = Self::normalize_path(query.path());
        let query = query.clone();
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
            let (binding, requested) = self.bind(&query, &file)?;
            let (line, col) = byte_offset_to_tsserver_pos(&requested, offset);

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
                binding.settle(&self.applied, [])?;
                return Ok(None);
            };
            // The entry's auto-import `codeActions` parse into `additionalTextEdits`:
            // each edit decodes through its target's receipt as the query was sent
            // under it — only the files those code actions target.
            let target_paths =
                verter_type_runtime::contents_snapshot::tsserver_completion_entry_details_target_paths(
                    detail,
                );
            let cache_snapshot = binding.settle(&self.applied, target_paths)?;
            Ok(completion_entry_details_to_resolve_result(
                detail,
                &file,
                &cache_snapshot,
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

    fn set_project_ownership(&self, authority: Arc<dyn ConfiguredOwnerAuthority>) {
        *self.ownership.write() = Some(authority);
    }

    /// Re-declare every live file against the CURRENT ownership authority.
    ///
    /// Init opens files as soon as the editor does, but publishes the exact
    /// workspace snapshot — and with it the configured-owner authority — later.
    /// Everything opened in between carries the bootstrap FOLDER identity, which
    /// for a nested package names the wrong project entirely: the extension host
    /// then resolves that package's TypeScript from the workspace folder and
    /// reports the package's own install absent.
    ///
    /// Nothing else repairs that binding. An ordinary `update_file` on an
    /// already-open file sends `changedFiles`, which carries no root and no
    /// config, so it cannot move a file between projects however many edits
    /// follow. Inheriting the trait's no-op therefore leaves every
    /// bootstrap-opened file mis-bound for the life of the window — which is why
    /// `background_init` calls this immediately after installing the authority.
    ///
    /// A file the authority now places in NO configured project is CLOSED and
    /// left closed: `NoProject` is terminal, and re-opening it would re-assert a
    /// project that does not exist. Per-file failures do not abort the sweep —
    /// one refused project must not leave every remaining file bound to the
    /// bootstrap folder — but the first error is returned once the sweep is done.
    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            // Deterministic order, and no lock held across an await.
            let mut files: Vec<String> = self.opened_files.lock().await.iter().cloned().collect();
            files.sort();

            let mut first_error: Option<TypeProviderError> = None;
            let mut record = |result: Result<serde_json::Value, TypeProviderError>| {
                if let Err(e) = result {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            };

            for file in files {
                let content = self.contents.lock().await.get(&file).cloned();
                let Some(content) = content else {
                    // Open with no cached buffer: nothing authoritative to
                    // re-declare, and a re-open would have to invent content.
                    continue;
                };
                let binding = self.project_binding_for(&file);

                // The close and re-open are deliveries like any other: the
                // service's bytes for the file are no longer the receipt's.
                self.applied.lock().close(&file);
                record(
                    self.query("close", serde_json::json!({ "file": file }))
                        .await,
                );

                let (project_root, project_config) = match binding {
                    FileProjectBinding::Configured { root, config } => (root, Some(config)),
                    FileProjectBinding::Bootstrap { root } => (root, None),
                    FileProjectBinding::Unowned => {
                        self.opened_files.lock().await.remove(&file);
                        self.contents.lock().await.remove(&file);
                        continue;
                    }
                };

                let ticket = self.issue_delivery(&file, &content);
                let delivered = self
                    .query(
                        "open",
                        open_entry(&file, &content, &project_root, project_config.as_deref()),
                    )
                    .await;
                self.applied.lock().settle(ticket, delivered.is_ok());
                record(delivered);
            }

            match first_error {
                Some(e) => Err(e),
                None => Ok(()),
            }
        })
    }
}

#[cfg(test)]
#[path = "extension_provider_tests.rs"]
mod tests;
