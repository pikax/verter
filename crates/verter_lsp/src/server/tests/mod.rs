use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

use crate::server::PublishedResolverSnapshot;
use crate::test_utils::make_test_vfs_workspace_from_registry;
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use crate::type_provider::protocol::{
    CompletionResolveResult, CompletionResult, HoverInfo, InlayHint, ProviderDiagnosticContext,
    RenameLocation, SemanticToken, SignatureHelp, TypeCodeAction, TypeDiagnostic,
    TypeDocumentHighlight, TypeLocation,
};
use crate::type_provider::traits::{ProviderFuture, TypeProvider};
use crate::ProjectSyncMode;

// ── synthetic store-backed workspace roots ────────────────────────────────
//
// Several tests below drive the real `CarrierPublishCoordinator` over the on-disk
// carrier store, whose dir is `temp/verter-carrier-store/<host>/blake3(<ws_root>)`.
// That dir is PROCESS-EXTERNAL, so a synthetic root shared by two concurrent test
// processes aliases them onto ONE `manifest.json`. These helpers are the single seam
// those roots come from.

/// The synthetic workspace root for a store-backed server test, from the
/// disambiguators a concurrent test run varies over.
///
/// `pid` is the load-bearing one: these tests run one per PROCESS, so the per-process
/// counter in [`unique_server_ws_root`] reads 0 in every process, and
/// `SystemTime::now()` is only MICROSECOND-resolution on macOS. Without the process
/// identity the root collides whenever two test processes reach it inside the same
/// microsecond.
fn server_ws_root_for(tag: &str, pid: u32, nanos: u128, n: u64) -> String {
    format!("/verter_{tag}_{pid}_{nanos}_{n}/ws")
}

/// A synthetic workspace root unique across concurrent PROCESSES — see
/// [`server_ws_root_for`].
fn unique_server_ws_root(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    server_ws_root_for(tag, std::process::id(), nanos, n)
}

/// Read the carrier-store manifest for `ws_root` STRICTLY: `None` ONLY when the manifest
/// genuinely does not exist, and a PANIC naming the cause for every other failure.
///
/// These tests must not read through `CarrierPublishStore::current_manifest`, which by
/// design reports a fresh EMPTY manifest for an unreadable or unparseable one. That
/// fail-open is right for a read-only diagnostics view and wrong here in two distinct
/// ways: a presence `.expect(...)` would blame the PUBLISH for a STORE failure, and an
/// ABSENCE assertion ("owner loss must retract the carrier") would pass VACUOUSLY
/// because an empty manifest trivially satisfies "not owned".
fn carrier_manifest_strict(ws_root: &str) -> Option<crate::external_ts::Manifest> {
    use crate::external_ts::{default_carrier_store_host_version, CarrierPublishStore};
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), ws_root);
    let path = store.manifest_path();
    match std::fs::read(&path) {
        Ok(bytes) => Some(
            serde_json::from_slice::<crate::external_ts::Manifest>(&bytes).unwrap_or_else(|e| {
                panic!(
                    "the carrier-store oracle must surface a store failure rather than \
                     report nothing published: manifest at {} is present but unparseable: {e}",
                    path.display()
                )
            }),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => panic!(
            "the carrier-store oracle must surface a store failure rather than report \
             nothing published: manifest at {} is unreadable: {e} (kind={:?}, errno={:?})",
            path.display(),
            e.kind(),
            e.raw_os_error()
        ),
    }
}

#[derive(Default)]
struct SlowConfigurePathsProvider {
    configure_paths_started: AtomicUsize,
    /// Fires when `configure_paths` is entered, so a test waits on the
    /// call itself instead of polling `configure_paths_started`.
    configure_paths_started_notify: tokio::sync::Notify,
    /// Never notified — `configure_paths` parks on it forever. A test that
    /// wants to prove `initialized()` does not await background path
    /// configuration blocks this indefinitely rather than racing a fixed
    /// sleep against a wall-clock ceiling: if `initialized()` incorrectly
    /// awaited this future it would hang forever (any wrapping timeout
    /// discriminates), never merely run slow under load.
    configure_paths_release: tokio::sync::Notify,
}

impl TypeProvider for SlowConfigurePathsProvider {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// This double does not distinguish a background load from an editor open.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
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
        _diagnostics: &[ProviderDiagnosticContext],
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
        self.configure_paths_started_notify.notify_waiters();
        Box::pin(async {
            self.configure_paths_release.notified().await;
            Ok(())
        })
    }
}

#[derive(Default)]
struct TriggerSensitiveCompletionProvider;

impl TypeProvider for TriggerSensitiveCompletionProvider {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// This double does not distinguish a background load from an editor open.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
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
                    crate::type_provider::protocol::Completion {
                        label: "name".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                        detail: Some("(property) name: string".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
                        data: None,
                    },
                    crate::type_provider::protocol::Completion {
                        label: "id".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                        detail: Some("(property) id: number".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
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
        _diagnostics: &[ProviderDiagnosticContext],
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
struct DotTriggerRequiredCompletionProvider {
    applied: std::sync::Mutex<HashMap<String, Arc<str>>>,
}

impl TypeProvider for DotTriggerRequiredCompletionProvider {
    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        use verter_type_runtime::traits::AppliedContent;
        match self.applied.lock().unwrap().get(path) {
            Some(bytes) => AppliedContent::Applied(Arc::clone(bytes)),
            None => AppliedContent::NotApplied,
        }
    }

    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = Arc::<str>::from(content);
        Box::pin(async move {
            self.applied
                .lock()
                .unwrap()
                .insert(path, Arc::clone(&content));
            Ok(())
        })
    }

    /// This double does not distinguish a background load from an editor open.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.applied.lock().unwrap().remove(&path);
            Ok(())
        })
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
                    crate::type_provider::protocol::Completion {
                        label: "disabled".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                        detail: Some("(property) disabled: boolean".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
                        data: None,
                    },
                    crate::type_provider::protocol::Completion {
                        label: "label".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                        detail: Some("(property) label: string".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
                        data: None,
                    },
                    crate::type_provider::protocol::Completion {
                        label: "handler".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Method),
                        detail: Some("(method) handler(): void".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
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
        _diagnostics: &[ProviderDiagnosticContext],
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

/// A FAITHFUL type provider for the declaration-overlay close-supersession race:
/// it applies the open/close EFFECT (mutating `open_paths`) at await-COMPLETION
/// inside the returned future — exactly as the real `ExtensionTypeProvider` does
/// (it inserts/removes the path inside the `async move` body), NOT at call-ENTRY
/// like `MockTypeProvider` (which records the call before its `.await` returns and
/// therefore MASKS the stale-close race). `open_paths` is the provider's ground
/// truth: a `.d.<ext>.ts` overlay is resolvable iff it is in this set, so a test
/// can assert the provider and the refcount agree (no TS2307 stranding).
///
/// A one-shot close gate (`block_close_path` / `arrived` / `release`) pauses the
/// `close_file` future of ONE exact path AFTER it has been entered but BEFORE the
/// open-set removal effect applies — modelling a slow provider close, so the test
/// can run a concurrent reopen pass inside the destructive-close window
/// deterministically (no timing race).
#[derive(Default)]
struct GatedDeclOverlayProvider {
    open_paths: std::sync::Mutex<HashSet<String>>,
    calls: std::sync::Mutex<Vec<MockCall>>,
    /// `Some((path, arrived, release))`: a `close_file` against `path` SIGNALS
    /// `arrived` (the close future has been entered, before the open-set removal),
    /// then AWAITS `release` before applying the removal effect and returning. The
    /// gate is taken (one-shot) so subsequent closes of the same path do not block.
    #[allow(clippy::type_complexity)]
    close_gate: std::sync::Mutex<
        Option<(
            String,
            std::sync::Arc<tokio::sync::Notify>,
            std::sync::Arc<tokio::sync::Notify>,
        )>,
    >,
    /// `Some((path, opened))`: an `open_file`/`update_file` against `path` SIGNALS
    /// `opened` AFTER its open-set insertion effect has applied (the overlay is now
    /// resolvable). One-shot. Lets a test learn the EXACT moment a specific overlay
    /// open EFFECT lands — used to release a gated close strictly after a concurrent
    /// reopen has re-established the overlay, so the stale-close strand is
    /// deterministic in a tree WITHOUT the owner's per-path serialization.
    #[allow(clippy::type_complexity)]
    open_signal: std::sync::Mutex<Option<(String, std::sync::Arc<tokio::sync::Notify>)>>,
}

impl GatedDeclOverlayProvider {
    fn open_paths_snapshot(&self) -> HashSet<String> {
        self.open_paths.lock().unwrap().clone()
    }

    fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Take (one-shot) the open signal armed for `path`, if any.
    fn take_open_signal(&self, path: &str) -> Option<std::sync::Arc<tokio::sync::Notify>> {
        let mut guard = self.open_signal.lock().unwrap();
        match guard.as_ref() {
            Some((armed, _)) if armed == path => guard.take().map(|(_, opened)| opened),
            _ => None,
        }
    }

    /// Arm the one-shot close gate for `path`. Returns `(arrived, release)`: the
    /// test awaits `arrived` to learn the close future has been entered (the
    /// closing task is now paused INSIDE the destructive close, before the open-set
    /// removal applies), runs whatever concurrent reopen it needs, then signals
    /// `release` (`notify_one`, which stores a permit so there is no
    /// signal-before-await race) to let the close apply its removal and return.
    fn block_close_path(
        &self,
        path: &str,
    ) -> (
        std::sync::Arc<tokio::sync::Notify>,
        std::sync::Arc<tokio::sync::Notify>,
    ) {
        let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        *self.close_gate.lock().unwrap() =
            Some((path.to_string(), arrived.clone(), release.clone()));
        (arrived, release)
    }

    /// Arm a one-shot OPEN signal for `path`. Returns `opened`: a future open (or
    /// in-place update) of `path` fires it AFTER applying its open-set insertion, so
    /// the test learns the open EFFECT has landed (the overlay is now resolvable).
    fn signal_open_path(&self, path: &str) -> std::sync::Arc<tokio::sync::Notify> {
        let opened = std::sync::Arc::new(tokio::sync::Notify::new());
        *self.open_signal.lock().unwrap() = Some((path.to_string(), opened.clone()));
        opened
    }
}

impl TypeProvider for GatedDeclOverlayProvider {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        // Take the one-shot open signal (if armed for this exact path) WITHOUT
        // holding the open_paths lock across the await.
        let opened_signal = self.take_open_signal(&path);
        Box::pin(async move {
            // EFFECT at await-completion: the path becomes resolvable only once the
            // open future actually runs to its end (faithful to the real provider).
            self.open_paths.lock().unwrap().insert(path.clone());
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::OpenFile { path, content });
            // Signal AFTER the open-set insertion: the overlay is now resolvable.
            if let Some(opened) = opened_signal {
                opened.notify_one();
            }
            Ok(())
        })
    }

    /// This double does not distinguish a background load from an editor open.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        let opened_signal = self.take_open_signal(&path);
        Box::pin(async move {
            // An update keeps the path open (sync_dts of an already-live overlay).
            self.open_paths.lock().unwrap().insert(path.clone());
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::UpdateFile { path, content });
            if let Some(opened) = opened_signal {
                opened.notify_one();
            }
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        // Capture the one-shot gate (if armed for this exact path) WITHOUT holding
        // the open_paths lock across the await.
        let gate = {
            let mut guard = self.close_gate.lock().unwrap();
            match guard.as_ref() {
                Some((armed, _, _)) if armed == &path => {
                    guard.take().map(|(_, arrived, release)| (arrived, release))
                }
                _ => None,
            }
        };
        Box::pin(async move {
            if let Some((arrived, release)) = gate {
                // Signal the test that the destructive close has been ENTERED (the
                // open-set removal has NOT applied yet), then await the release.
                arrived.notify_one();
                release.notified().await;
            }
            // EFFECT at await-completion: only now is the path no longer resolvable.
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
        _diagnostics: &[ProviderDiagnosticContext],
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

#[derive(Default)]
struct LostContentCompletionProvider {
    open_paths: std::sync::Mutex<HashSet<String>>,
    applied: std::sync::Mutex<HashMap<String, std::sync::Arc<str>>>,
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
    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        use verter_type_runtime::traits::AppliedContent;
        match self.applied.lock().unwrap().get(path) {
            Some(bytes) => AppliedContent::Applied(std::sync::Arc::clone(bytes)),
            None => AppliedContent::NotApplied,
        }
    }

    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.open_paths.lock().unwrap().insert(path.clone());
            self.applied
                .lock()
                .unwrap()
                .insert(path.clone(), std::sync::Arc::from(content.as_str()));
            self.calls
                .lock()
                .unwrap()
                .push(MockCall::OpenFile { path, content });
            Ok(())
        })
    }

    /// This double does not distinguish a background load from an editor open.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.applied
                .lock()
                .unwrap()
                .insert(path.clone(), std::sync::Arc::from(content.as_str()));
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
            self.applied.lock().unwrap().remove(&path);
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
                return Err(crate::type_provider::protocol::TypeProviderError::new(
                    "No content available.",
                ));
            }
            if self.require_current_api {
                let current_api_path = path
                    .strip_suffix(".tsx")
                    .map(|prefix| format!("{prefix}.verter.ts"))
                    .unwrap_or_else(|| path.clone());
                if !self.open_paths.lock().unwrap().contains(&current_api_path) {
                    return Err(crate::type_provider::protocol::TypeProviderError::new(
                        "No content available.",
                    ));
                }
            }
            Ok(CompletionResult {
                items: vec![
                    crate::type_provider::protocol::Completion {
                        label: "disabled".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                        detail: Some("(property) disabled: boolean".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
                        data: None,
                    },
                    crate::type_provider::protocol::Completion {
                        label: "label".to_string(),
                        kind: Some(crate::type_provider::protocol::CompletionKind::Property),
                        detail: Some("(property) label: string".to_string()),
                        documentation: None,
                        edit_range_start: None,
                        edit_range_end: None,
                        text_edit_new_text: None,
                        insert_text: None,
                        sort_text: None,
                        insert_text_format: None,
                        commit_characters: None,
                        filter_text: None,
                        preselect: None,
                        label_details: None,
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
        _diagnostics: &[ProviderDiagnosticContext],
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
    make_hover_test_service_with_kind(type_provider, crate::TypeProviderKind::Tsserver)
}

/// Build a hover/sync test service whose engine exercises the inferred
/// carrier-open path (tsgo). Used by the tests that characterize the
/// ENGINE-AGNOSTIC IDE-path commit/close-after-success state machine
/// (open-new-then-close-old, retain-prior-on-failed-sync, jsx↔tsx flip):
/// under tsserver the carrier-companion content verbs are no-ops (publish-only),
/// so the open/fail path that discipline guards is observable only on tsgo. The
/// tsserver publish-only contract for those same methods is covered separately
/// (the `project_sync` suppression tests + the imported-carrier publish-only
/// tests + the real-provider carrier baselines).
fn make_hover_test_service_tsgo(
    type_provider: Arc<dyn TypeProvider>,
) -> tower_lsp_server::LspService<VerterLanguageServer> {
    make_hover_test_service_with_kind(type_provider, crate::TypeProviderKind::Tsgo)
}

fn make_hover_test_service_with_kind(
    type_provider: Arc<dyn TypeProvider>,
    kind: crate::TypeProviderKind,
) -> tower_lsp_server::LspService<VerterLanguageServer> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: kind,
                type_provider_topology: crate::TypeProviderTopology::implied_by(kind),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    // These legacy contract tests intentionally exercise Verter's native hover
    // enrichment. Production now defaults that optional lane off; opt the shared
    // hover fixture in explicitly so the tests describe the feature they assert
    // instead of depending on the production default.
    service
        .inner()
        .hover_native_semantics_enabled
        .store(true, std::sync::atomic::Ordering::Release);
    // Cross-file native completion enrichment is also opt-in and cache-only.
    // These contract fixtures deliberately exercise that optional lane; real
    // provider tests below keep the production default and assert TypeScript-
    // owned completion behavior.
    service
        .inner()
        .documents
        .set_semantic_analysis_enabled(true);
    service
}

fn install_test_resolver(server: &VerterLanguageServer) {
    install_test_resolver_for_root(server, "/workspace", Some("/workspace/tsconfig.json"));
}

pub(super) fn install_test_resolver_for_root(
    server: &VerterLanguageServer,
    root: &str,
    tsconfig: Option<&str>,
) {
    install_test_resolver_for_root_with_options(
        server,
        root,
        tsconfig,
        verter_session_query::resolution::IdeProjectCompilerOptions::default(),
    );
}

fn install_test_resolver_for_root_with_options(
    server: &VerterLanguageServer,
    root: &str,
    tsconfig: Option<&str>,
    compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions,
) {
    let vfs_ws = std::sync::Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));

    let root_cp = verter_workspace::CanonicalPath::new(root);

    // Mirror the production snapshot builder, which emits a tsconfig-backed
    // `Configured` project AND a `Fallback` project at the workspace root. A
    // carrier's external-TS membership resolves through the SHARED publish path
    // (`publish_carrier` → `WorkspaceProjectResolver`), and that resolver only
    // mints a `ProjectBinding` for a `Configured` owner — a `Fallback`-only
    // snapshot fails closed (`NoProject`), so the carrier is never published or
    // registered. A `tsconfig` therefore yields a `Configured` owner here so the
    // membership path exercises the same mechanism production uses. The projects
    // are pushed in precedence order (a `Configured` owner precedes the `Fallback`
    // at the same root) and re-id'd by index so IDs match position.
    let mut projects: Vec<verter_workspace::workspace_snapshot::OwnershipProject> = Vec::new();
    if let Some(tsconfig) = tsconfig {
        // Spec-bridge membership: an `include` of `{root}/**/*` with an empty
        // materialized set, so `ConfiguredMembership::contains` matches every
        // file under `root` via the static spec (the documented bridge mode for
        // a harness that does not walk the disk).
        let spec = verter_session_query::resolution::StaticMembershipSpec {
            files: Vec::new(),
            include: vec![verter_session_query::resolution::CompiledGlob::new(
                verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                    &root_cp, "**/*",
                ),
            )],
            exclude: vec![verter_session_query::resolution::CompiledGlob::new(
                verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                    &root_cp,
                    "node_modules/**",
                ),
            )]
            .into(),
        };
        projects.push(verter_workspace::workspace_snapshot::OwnershipProject {
            id: verter_session_query::resolution::ProjectId(0),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                membership: verter_session_query::resolution::ConfiguredMembership {
                    spec,
                    materialized_files: Default::default(),
                },
                compiler_options: compiler_options.clone(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        });
    }
    projects.push(verter_workspace::workspace_snapshot::OwnershipProject {
        id: verter_session_query::resolution::ProjectId(0),
        root: root_cp.clone(),
        workspace_root: root_cp.clone(),
        payload: verter_workspace::workspace_snapshot::ProjectPayload::Fallback {
            membership: verter_workspace::FallbackMembership {
                root: root_cp.clone(),
                exclude: vec![verter_session_query::resolution::CompiledGlob::new(
                    verter_session_query::resolution::NormalizedGlob::new(&format!(
                        "{}/node_modules/**",
                        root
                    )),
                )]
                .into(),
            },
        },
    });
    // IDs must match index position (the snapshot invariant
    // `build_workspace_snapshot_simple` upholds after its precedence sort).
    for (i, project) in projects.iter_mut().enumerate() {
        project.id = verter_session_query::resolution::ProjectId(i as u32);
    }

    let mut resolver_project = verter_workspace::ide_project_config(
        root.to_string(),
        root.to_string(),
        tsconfig.map(|s| s.to_string()),
    );
    resolver_project.compiler_options = compiler_options;
    let resolver = verter_resolution::ModuleResolverCore::new(vec![resolver_project]);

    let snapshot = std::sync::Arc::new(verter_workspace::WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });

    let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    // This helper models the completed production publication, so install the
    // authoritative workspace into both the server and the host. A server-only
    // handle would leave rename admission reading the host's earlier
    // non-authoritative graph.
    server.swap_vfs_workspace(vfs_ws);
}

/// A `FilesystemWorkspace` publishing ONE `Configured` project owning everything under
/// `root` via `tsconfig` (spec-bridge `include: {root}/**/*`), so a carrier under
/// `root` resolves to a `Bound` project binding through the shared
/// `WorkspaceProjectResolver` — the ownership-resolution source the carrier-sync
/// gateway reads for BOTH engines. Mirrors [`install_test_resolver_for_root`] but
/// returns the published workspace directly (for the direct-call background-task tests).
fn configured_owner_vfs(root: &str, tsconfig: &str) -> Arc<verter_workspace::FilesystemWorkspace> {
    let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let root_cp = verter_workspace::CanonicalPath::new(root);
    let spec = verter_session_query::resolution::StaticMembershipSpec {
        files: Vec::new(),
        include: vec![verter_session_query::resolution::CompiledGlob::new(
            verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                &root_cp, "**/*",
            ),
        )],
        exclude: vec![verter_session_query::resolution::CompiledGlob::new(
            verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                &root_cp,
                "node_modules/**",
            ),
        )]
        .into(),
    };
    let projects = vec![verter_workspace::workspace_snapshot::OwnershipProject {
        id: verter_session_query::resolution::ProjectId(0),
        root: root_cp.clone(),
        workspace_root: root_cp.clone(),
        payload: verter_workspace::workspace_snapshot::ProjectPayload::Configured {
            tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
            membership: verter_session_query::resolution::ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions::default(
            ),
            references: Vec::new(),
            workspace_aliases: Vec::new(),
        },
    }];
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            root.to_string(),
            root.to_string(),
            Some(tsconfig.to_string()),
        )]);
    let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });
    let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    vfs_ws
}

/// A host config for a real-provider CORRECTNESS test: every request deadline
/// raised to the long batch backstop.
///
/// The production deadlines are human-scaled to a single healthy provider
/// (hover 1.5s, definition 2.5s, ...). A correctness test that spins a real
/// tsserver runs alongside dozens of others under nextest, a CPU-starvation
/// environment where a normally-fast round-trip is scheduled out past its
/// budget. That is the canonical "slow machine" the deadlines are configurable
/// for; a correctness test validates the RESULT, not the latency, so it lifts
/// every kind to the backstop. Wedge / fail-closed repros keep their own tight
/// deadline and do not use this.
fn real_provider_correctness_config() -> HostConfig {
    let backstop = std::time::Duration::from_secs(15);
    HostConfig {
        lsp_method_timeouts: verter_session::LspMethodTimeoutsConfig {
            request_deadlines: verter_session::LspMethodBudgets {
                hover: backstop,
                goto_definition: backstop,
                completion: backstop,
                references: backstop,
                diagnostics: backstop,
                document_symbols: backstop,
                semantic_tokens: backstop,
                inlay_hints: backstop,
                code_action: backstop,
                rename: backstop,
                other: backstop,
            },
            ..HostConfig::default().lsp_method_timeouts
        },
        ..HostConfig::default()
    }
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

fn mock_completion(
    label: &str,
    kind: crate::type_provider::protocol::CompletionKind,
) -> crate::type_provider::protocol::Completion {
    crate::type_provider::protocol::Completion {
        label: label.to_string(),
        kind: Some(kind),
        detail: None,
        documentation: None,
        edit_range_start: None,
        edit_range_end: None,
        text_edit_new_text: None,
        insert_text: None,
        sort_text: None,
        insert_text_format: None,
        commit_characters: None,
        filter_text: None,
        preselect: None,
        label_details: None,
        data: None,
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

/// Build a [`TypeProviderContext`] for tests through the SAME captured-surface
/// path production uses, with the document's DependencyReady receipt settled
/// the way production settles it (background publication on open — these
/// harnesses bypass `did_open`, so the settle runs inline here). Tests drive
/// provider sync through many entry points; when a test set up its document
/// WITHOUT completing a recorded sync, seed the committed sync state + recorded
/// surface from the live artifacts (the exact data a successful sync would have
/// recorded) and capture again.
pub(super) async fn synced_type_provider_context(
    server: &VerterLanguageServer,
    uri: &Uri,
) -> TypeProviderContext {
    // Settle the background dependency publication first: navigation handlers
    // only CAPTURE the receipt (they never start the pass), so a test that
    // expects a provider-backed answer must provide the receipt like
    // production's open path does.
    server.publish_import_dependencies_settled(uri).await;
    synced_type_provider_context_surface_only(server, uri)
}

/// Publish `content` through the production open path on a helper thread.
///
/// The seeder is synchronous and is often called from a test that already
/// owns the runtime; `block_on` on that runtime deadlocks. The helper has
/// its own current-thread runtime, so the open runs the same `open_tsx`
/// receipt gate production uses.
fn publish_seed_surface(server: &VerterLanguageServer, tsx_path: &str, content: &str) {
    let Some(sync) = server.project_sync.clone() else {
        return;
    };
    let path = tsx_path.to_string();
    let content = content.to_string();
    std::thread::Builder::new()
        .name("seed-provider-open".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("seed runtime");
            runtime.block_on(sync.open_tsx(&path, &content))
        })
        .expect("seed thread")
        .join()
        .expect("seed thread panicked")
        .expect("seeded provider open");
}

/// Stamp the IDE surface a test recorded at `tsx_path` as the one the
/// committed sync state of `canonical_id` published. A successful IDE sync on
/// the membership-only topology is the receipt-gated commit, which stamps the
/// exact surface it published: the engine never reads an IDE companion whose
/// bytes no receipt fingerprints, however live its path is.
pub(super) fn stamp_seeded_ide_publication(
    server: &VerterLanguageServer,
    canonical_id: &str,
    tsx_path: &str,
) {
    let recorded = server
        .documents
        .provider_surfaces()
        .current_snapshot(tsx_path)
        .expect("the seeded IDE surface was recorded");
    let mut state = server
        .provider_sync_state_for_source(canonical_id)
        .expect("the seeded sync state was committed");
    state.committed_ide_surface = Some(crate::provider_sync::CommittedCarrierSurface {
        content_hash: recorded.stamp.content_hash.to_hash16(),
        map_hash: recorded.stamp.map_hash,
    });
    server.commit_provider_sync_state(canonical_id, state);
}

/// The surface half of [`synced_type_provider_context`], WITHOUT the
/// DependencyReady settle — for seeding helpers whose handlers are not
/// receipt-gated (hover / completion).
fn synced_type_provider_context_surface_only(
    server: &VerterLanguageServer,
    uri: &Uri,
) -> TypeProviderContext {
    if let Some(ctx) = server.type_provider_context(uri) {
        return ctx;
    }
    let canonical_id = server
        .documents
        .get_canonical_id(uri)
        .expect("canonical id should exist");
    server.documents.host().ensure_loaded(&canonical_id);
    let ide = server
        .documents
        .get_ide(uri)
        .expect("IDE output should exist");
    let tsx_path = server
        .active_ide_path_for_uri(uri)
        .or_else(|| server.target_ide_path_for_uri(uri))
        .expect("type provider path should exist");
    // Simulate the state a completed successful IDE sync commits + records.
    let mut state = server
        .provider_sync_state_for_source(&canonical_id)
        .unwrap_or_else(|| ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ..Default::default()
        });
    state.ide_path = Some(tsx_path.clone());
    state.ide_background_loaded = true;
    server.commit_provider_sync_state(&canonical_id, state);
    // Fenced with the document's OWN current identity: this seed simulates a
    // successful, non-racing IDE sync for the still-open document, so a real
    // pin captured right here (nothing can have moved between this capture
    // and the record just below) records normally.
    let seed_revision = server.documents.snapshot_identity(uri);
    publish_seed_surface(server, &tsx_path, &ide.code);
    server.record_carrier_ide_snapshot_with_pin(
        seed_revision.as_ref().map(|revision| (uri, revision)),
        &canonical_id,
        &tsx_path,
        &ide.code,
        None,
    );
    stamp_seeded_ide_publication(server, &canonical_id, &tsx_path);
    server
        .type_provider_context(uri)
        .expect("a seeded provider surface must yield a query context")
}

fn set_type_hover_at_vue_position(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    uri: &Uri,
    position: Position,
    contents: &str,
) {
    let ctx = synced_type_provider_context_surface_only(server, uri);
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("vue position should map to tsx");
    provider.set_hover(
        &ctx.tsx_path,
        tsx_offset,
        Some(HoverInfo {
            // What a real provider returns: the structured display signature
            // (the render source) plus the rendered blob for whole-blob
            // consumers.
            contents: contents.to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(contents)),
            ..Default::default()
        }),
    );
}

fn set_type_completions_at_vue_position(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    uri: &Uri,
    position: Position,
    items: Vec<crate::type_provider::protocol::Completion>,
) {
    let ctx = synced_type_provider_context_surface_only(server, uri);
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("vue position should map to tsx");
    provider.set_completions(&ctx.tsx_path, tsx_offset, items);
}

fn test_module_reference(
    raw_text: &str,
    literal_specifier: Option<&str>,
    finite_specifiers: &[&str],
    analyzability: verter_session_query::analysis::types::ModuleReferenceAnalyzability,
    expr_start: usize,
    expr_end: usize,
) -> verter_session::ScriptModuleReference {
    verter_session::ScriptModuleReference {
        syntax: verter_session_query::analysis::types::ModuleReferenceSyntax::StaticImport,
        semantics: verter_session_query::analysis::types::ModuleReferenceSemantics::Import,
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
    analyzability: verter_session_query::analysis::types::ModuleReferenceAnalyzability,
    expr_start: usize,
    expr_end: usize,
    semantics: verter_session_query::analysis::types::ModuleReferenceSemantics,
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
    analyzability: verter_session_query::analysis::types::ModuleReferenceAnalyzability,
    expr_start: usize,
    expr_end: usize,
) -> verter_session_query::analysis::types::AnalyzedModuleReference {
    verter_session_query::analysis::types::AnalyzedModuleReference {
        syntax: verter_session_query::analysis::types::ModuleReferenceSyntax::StaticImport,
        semantics: verter_session_query::analysis::types::ModuleReferenceSemantics::Import,
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

struct TestResolverReader {
    files: HashSet<String>,
    texts: HashMap<String, Arc<str>>,
    workspace: verter_workspace::MemoryWorkspace,
}

impl Default for TestResolverReader {
    fn default() -> Self {
        let workspace =
            verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
        verter_workspace::WorkspaceAccess::configure_resolver(
            &workspace,
            vec![verter_workspace::ide_project_config(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            )],
        );
        Self {
            files: HashSet::new(),
            texts: HashMap::new(),
            workspace,
        }
    }
}

impl TestResolverReader {
    fn with_files(paths: &[&str]) -> Self {
        let mut reader = Self::default();
        for path in paths {
            let normalized = path.replace('\\', "/");
            reader.files.insert(normalized.clone());
            reader
                .texts
                .insert(normalized.clone(), Arc::<str>::from("// test file"));
            reader
                .workspace
                .inject_file(normalized, Arc::<str>::from("// test file"));
        }
        reader
    }
}

impl verter_workspace::WorkspaceRead for TestResolverReader {
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

    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> Option<verter_session_query::resolution::ResolveResult> {
        verter_workspace::WorkspaceRead::resolve_import(
            &self.workspace,
            importer_id,
            specifier,
            ctx,
        )
    }

    fn resolve_import_outcome(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionOutcome {
        verter_workspace::WorkspaceRead::resolve_import_outcome(
            &self.workspace,
            importer_id,
            specifier,
            ctx,
        )
    }

    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn dependency_snapshot(
        &self,
        _canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        None
    }
}

impl verter_workspace::WorkspaceAccess for TestResolverReader {
    // Reader-only stub overrides (R6/R7). Rationale: `TestResolverReader`
    // is an LSP test fixture that only feeds the resolver with file content
    // for definition/hover/completion test plumbing; it never participates
    // in the host's dep-flow.
    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[verter_workspace::ParsedEdge]) {}
    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        verter_workspace::ExactResolutionResult::default()
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        _canonical_id: &str,
        _edges: &[verter_workspace::ParsedEdge],
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
    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

struct ReturnOnlyResolverReader;

impl verter_workspace::WorkspaceRead for ReturnOnlyResolverReader {
    fn read_file(&self, _canonical_id: &str) -> Option<Arc<str>> {
        None
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        canonical_id == "/workspace/src/dep.ts"
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.file_exists(canonical_id)
            .then(|| canonical_id.to_string())
    }

    fn resolve_import(
        &self,
        _importer_id: &str,
        specifier: &str,
        _ctx: verter_session_query::resolution::ResolutionContext,
    ) -> Option<verter_session_query::resolution::ResolveResult> {
        (specifier == "./dep").then(|| verter_session_query::resolution::ResolveResult {
            source_id: "/workspace/src/dep.ts".to_string(),
            provider_id: "/workspace/src/dep.ts".to_string(),
            provider_specifier: "./dep".to_string(),
            provider_target: verter_session_query::resolution::ProviderTarget::SourceFile,
            resolution_kind: verter_session_query::resolution::ResolutionKind::Relative,
            owner_tsconfig_path: Some("/workspace/tsconfig.json".to_string()),
        })
    }

    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    fn dependency_snapshot(
        &self,
        _canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        None
    }
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
    make_definition_test_server_with_kind(files, crate::TypeProviderKind::Tsserver).await
}

/// Build a filesystem-backed definition/sync test server bound to `kind`. The
/// barrel-sync BFS tests route through `Tsgo` so the terminal-carrier sync is an
/// observable `open_file` (under tsserver the carrier-companion verbs are
/// plugin/store-owned and no barrel walk runs).
async fn make_definition_test_server_with_kind(
    files: &[(&str, &str, &str)],
    kind: crate::TypeProviderKind,
) -> (
    tempfile::TempDir,
    tower_lsp_server::LspService<VerterLanguageServer>,
    tokio::task::JoinHandle<()>,
    Arc<MockTypeProvider>,
    String,
) {
    make_definition_test_server_with_kind_and_deadlines(
        files,
        kind,
        verter_session::LspMethodBudgets::interactive_defaults(),
    )
    .await
}

/// [`make_definition_test_server_with_kind`] with custom production request
/// deadlines, so a readiness/cancellation test can use a SHORT definition
/// deadline instead of waiting out the shipped 2500 ms budget. Production
/// deadline values themselves are pinned by the deadline tests below — this
/// harness only exercises behaviour AT a deadline, never re-tunes one.
async fn make_definition_test_server_with_kind_and_deadlines(
    files: &[(&str, &str, &str)],
    kind: crate::TypeProviderKind,
    request_deadlines: verter_session::LspMethodBudgets,
) -> (
    tempfile::TempDir,
    tower_lsp_server::LspService<VerterLanguageServer>,
    tokio::task::JoinHandle<()>,
    Arc<MockTypeProvider>,
    String,
) {
    let mut host_config = HostConfig::default();
    host_config.lsp_method_timeouts.request_deadlines = request_deadlines;
    make_definition_test_server_with_config(files, kind, host_config, true).await
}

/// Build the production default-profile definition server: the projection host
/// runs BUILD and optional semantic analysis stays off. Carrier `did_open`
/// still compiles the normal IDE + TEMPLATE_DATA projection.
async fn make_default_profile_definition_test_server(
    files: &[(&str, &str, &str)],
) -> (
    tempfile::TempDir,
    tower_lsp_server::LspService<VerterLanguageServer>,
    tokio::task::JoinHandle<()>,
    Arc<MockTypeProvider>,
    String,
) {
    make_definition_test_server_with_config(
        files,
        crate::TypeProviderKind::Tsserver,
        HostConfig {
            analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
            ..HostConfig::default()
        },
        false,
    )
    .await
}

pub(super) async fn make_definition_test_server_with_config(
    files: &[(&str, &str, &str)],
    kind: crate::TypeProviderKind,
    host_config: HostConfig,
    enable_semantic_analysis: bool,
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

    // The harness models a real workspace: Svelte fixtures reference the
    // `svelte` package (statement imports AND binding-less inline
    // `import("svelte").…` type references), and the typed package-provenance
    // chain (snippet-role classification, dispatcher validation) requires the
    // specifier to resolve to the INSTALLED svelte package — a workspace
    // without it honestly degrades to `Unresolved` roles. Vendor the same
    // hermetic minimal Svelte 5 surface the real-provider fixtures vendor.
    let svelte_dir = workspace.join("node_modules").join("svelte");
    std::fs::create_dir_all(&svelte_dir).expect("vendored svelte dir");
    for (name, contents) in crate::test_harness::VENDORED_SVELTE_PACKAGE {
        std::fs::write(svelte_dir.join(name), contents)
            .unwrap_or_else(|e| panic!("write vendored svelte {name}: {e}"));
    }

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
    let host = Arc::new(VerterHost::new(host_config, vfs_workspace));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: kind,
                type_provider_topology: crate::TypeProviderTopology::implied_by(kind),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let socket = service.inner().outbound().wire();
    if enable_semantic_analysis {
        // Definition-server fixtures also back the native
        // component/directive/slot hover contract matrix. Keep that optional
        // lane explicit in tests now that the production initialization default
        // is disabled.
        service
            .inner()
            .hover_native_semantics_enabled
            .store(true, std::sync::atomic::Ordering::Release);
    }
    let drain_handle = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let server = service.inner();
    if enable_semantic_analysis {
        server.documents.set_semantic_analysis_enabled(true);
    }
    let ide_project = verter_workspace::ide_project_config(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    );
    // Sync resolver to host's VFS so resolve_import_transient works
    host.configure_projects(vec![ide_project]);
    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );

    let mut semantic_ready = server.documents.subscribe_semantic_ready();
    let mut scheduled_semantic = 0usize;
    for (relative_path, language_id, source) in files {
        let canonical_id = format!("{workspace_id}/{relative_path}");
        let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: (*language_id).to_string(),
            version: 1,
            text: (*source).to_string(),
        });
        if enable_semantic_analysis && matches!(*language_id, "vue" | "svelte") {
            scheduled_semantic += 1;
            server.documents.schedule_semantic_analysis(&uri);
        }
    }
    for _ in 0..scheduled_semantic {
        tokio::time::timeout(std::time::Duration::from_secs(10), semantic_ready.recv())
            .await
            .expect("optional semantic fixture must finish")
            .expect("optional semantic fixture signal must remain open");
    }
    if matches!(kind, crate::TypeProviderKind::Tsserver) {
        server.test_settle_workspace_carriers().await;
    }

    (temp, service, drain_handle, provider, workspace_id)
}

use crate::test_harness::fixture_workspace_root;

pub(super) fn workspace_uri(workspace_id: &str, relative_path: &str) -> Uri {
    crate::uri::path_to_file_uri(&format!("{workspace_id}/{relative_path}")).expect("file uri")
}

async fn settle_child_contracts(
    server: &VerterLanguageServer,
    parent_uri: &Uri,
    workspace_id: &str,
    child_paths: &[&str],
) {
    server.publish_import_dependencies_settled(parent_uri).await;
    for child_path in child_paths {
        let child_id = format!("{workspace_id}/{child_path}");
        assert!(
            server.cached_child_public_contract(&child_id).is_some(),
            "background import publication must commit `{child_path}` before the request boundary"
        );
    }
}

pub(super) fn find_document_position(
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

/// The authored range of `needle` in the open document (UTF-16 line index,
/// encoding-consistent with the merge).
fn range_for_authored_snippet(server: &VerterLanguageServer, uri: &Uri, needle: &str) -> Range {
    let start = find_document_position(server, uri, needle, 0);
    let end = find_document_position(server, uri, needle, needle.len());
    Range { start, end }
}

/// Collect every (uri, range, new_text) triple from a `WorkspaceEdit`, across
/// both the `changes` map and `document_changes` shapes.
fn workspace_edit_triples(edit: &WorkspaceEdit) -> Vec<(Uri, Range, String)> {
    let mut triples = Vec::new();
    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            for e in edits {
                triples.push((uri.clone(), e.range, e.new_text.clone()));
            }
        }
    }
    if let Some(DocumentChanges::Edits(doc_edits)) = &edit.document_changes {
        for doc_edit in doc_edits {
            for e in &doc_edit.edits {
                if let tower_lsp_server::ls_types::OneOf::Left(text_edit) = e {
                    triples.push((
                        doc_edit.text_document.uri.clone(),
                        text_edit.range,
                        text_edit.new_text.clone(),
                    ));
                }
            }
        }
    }
    triples
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

/// The capabilities a serving host advertises, built from THAT host's own
/// composed classification authority — the same source the initialize
/// handler reads, never a process-global registry.
fn host_server_capabilities(
    resolve_provider: bool,
) -> tower_lsp_server::ls_types::ServerCapabilities {
    let host = VerterHost::new_standalone(HostConfig::default());
    crate::capabilities::server_capabilities(
        &PositionEncodingKind::UTF16,
        resolve_provider,
        host.language_classifier(),
    )
}

fn configured_claimant_snapshot(
    root: &str,
    tsconfig_names: &[&str],
) -> Arc<verter_workspace::WorkspaceSnapshot> {
    let root_cp = verter_workspace::CanonicalPath::new(root);
    let make_configured = |tsconfig: &str| {
        let spec = verter_session_query::resolution::StaticMembershipSpec {
            files: Vec::new(),
            include: vec![verter_session_query::resolution::CompiledGlob::new(
                verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                    &root_cp, "**/*",
                ),
            )],
            exclude: vec![verter_session_query::resolution::CompiledGlob::new(
                verter_session_query::resolution::NormalizedGlob::from_root_and_pattern(
                    &root_cp,
                    "node_modules/**",
                ),
            )]
            .into(),
        };
        verter_workspace::workspace_snapshot::OwnershipProject {
            id: verter_session_query::resolution::ProjectId(0),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path: verter_workspace::CanonicalPath::new(tsconfig),
                membership: verter_session_query::resolution::ConfiguredMembership {
                    spec,
                    materialized_files: Default::default(),
                },
                compiler_options:
                    verter_session_query::resolution::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        }
    };
    let projects = tsconfig_names
        .iter()
        .map(|name| make_configured(&format!("{root}/{name}")))
        .collect();
    Arc::new(
        verter_workspace::snapshot_builder::build_workspace_snapshot_simple(
            projects,
            verter_workspace::workspace_snapshot::SnapshotGeneration(1),
        ),
    )
}

/// Publish a READY root with TWO sibling configured projects (`tsconfig.json` +
/// `tsconfig.app.json`) both `include`-ing everything under `root`, with no
/// reference edge — so every carrier under `root` is a genuine MULTI-CLAIMANT
/// overlap (`configured_owner_resolution_for_file` ⇒ `Ambiguous`) onto an EXISTING
/// workspace. Published LAST (after server construction + `did_open`) so it wins
/// over any bootstrap/rescan root those steps publish.
fn publish_multi_claimant_root(vfs_ws: &verter_workspace::FilesystemWorkspace, root: &str) {
    let snapshot = configured_claimant_snapshot(root, &["tsconfig.json", "tsconfig.app.json"]);
    let views = crate::workspace_state::build_lsp_views(vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
}

/// Build a provider-backed rename server without the managed carrier-publish
/// coordinator. That keeps this admission test focused: before the admission
/// fix, the provider request reaches the mock and exposes the partial edit
/// instead of being independently stopped by workspace-frontier readiness.
fn make_claimancy_rename_test_server(
    ws: Arc<verter_workspace::FilesystemWorkspace>,
) -> (
    tower_lsp_server::LspService<VerterLanguageServer>,
    crate::outbound::Wire,
    Arc<MockTypeProvider>,
) {
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws));
    let provider = Arc::new(MockTypeProvider::new());
    let host_for_server = Arc::clone(&host);
    let provider_for_server: Arc<dyn TypeProvider> = provider.clone();
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&provider_for_server)),
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::None,
                type_provider_topology: crate::TypeProviderTopology::implied_by(
                    crate::TypeProviderKind::None,
                ),
                mcp_port: None,
                type_provider_reason: Some("test provider".into()),
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let socket = service.inner().outbound().wire();
    (service, socket, provider)
}

/// Wait for a published workspace root matching `pred`. The generation
/// send from `FilesystemWorkspace::subscribe_published` is the receipt;
/// `load_published` is only the predicate, never the readiness poll.
fn wait_published_root(
    ws: &verter_workspace::FilesystemWorkspace,
    pred: impl Fn(&verter_workspace::PublishedRoot) -> bool,
) -> Arc<verter_workspace::PublishedRoot> {
    let rx = ws.subscribe_published();
    if let Some(root) = ws.load_published() {
        if pred(&root) {
            return root;
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "published workspace root never satisfied the wait predicate"
        );
        match rx.recv_timeout(remaining) {
            Ok(_) => {
                if let Some(root) = ws.load_published() {
                    if pred(&root) {
                        return root;
                    }
                }
            }
            Err(_) => {
                panic!("published workspace root never satisfied the wait predicate")
            }
        }
    }
}

/// D1 fixtures for the optional, background-warmed native completion lane.
/// Interactive completion may consume a committed analysis snapshot but never
/// loads or compiles a cold child itself; TypeScript owns cold typed results.
#[derive(Clone, Copy)]
struct D1CarrierCase {
    name: &'static str,
    extension: &'static str,
    language_id: &'static str,
    index_label: &'static str,
    child_source: &'static str,
    stale_child_source: &'static str,
    opened_parent_source: &'static str,
    edited_parent_source: &'static str,
    incomplete_parent_source: &'static str,
}

const D1_CARRIER_CASES: [D1CarrierCase; 4] = [
    D1CarrierCase {
        name: "vue-ts",
        extension: "vue",
        language_id: "vue",
        index_label: "z-index",
        child_source: "<script setup lang=\"ts\">\nconst internalOnly = true\ndefineProps<{ label: string; zIndex?: number }>()\n</script>\n",
        stale_child_source: "<script setup lang=\"ts\">\ndefineProps<{ staleOnly: string }>()\n</script>\n",
        opened_parent_source: "<script setup lang=\"ts\">\nimport BeforeComp from './BeforeComp.vue'\n</script>\n<template>\n  <BeforeComp  />\n</template>\n",
        edited_parent_source: "<script setup lang=\"ts\">\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n  <DirectComp  />\n</template>\n",
        incomplete_parent_source: "<script setup lang=\"ts\">\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n  <DirectComp ",
    },
    D1CarrierCase {
        name: "vue-js",
        extension: "vue",
        language_id: "vue",
        index_label: "z-index",
        child_source: "<script setup>\nconst internalOnly = true\ndefineProps({ label: { type: String, required: true }, zIndex: Number })\n</script>\n",
        stale_child_source: "<script setup>\ndefineProps({ staleOnly: String })\n</script>\n",
        opened_parent_source: "<script setup>\nimport BeforeComp from './BeforeComp.vue'\n</script>\n<template>\n  <BeforeComp  />\n</template>\n",
        edited_parent_source: "<script setup>\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n  <DirectComp  />\n</template>\n",
        incomplete_parent_source: "<script setup>\nimport DirectComp from './DirectComp.vue'\n</script>\n<template>\n  <DirectComp ",
    },
    D1CarrierCase {
        name: "svelte-ts",
        extension: "svelte",
        language_id: "svelte",
        index_label: "zIndex",
        child_source: "<script lang=\"ts\">\nconst internalOnly = true;\nlet { label, zIndex }: { label: string; zIndex?: number } = $props();\n</script>\n<p>{label}</p>\n",
        stale_child_source: "<script lang=\"ts\">\nlet { staleOnly }: { staleOnly: string } = $props();\n</script>\n",
        opened_parent_source: "<script lang=\"ts\">\nimport BeforeComp from './BeforeComp.svelte';\n</script>\n<BeforeComp  />\n",
        edited_parent_source: "<script lang=\"ts\">\nimport DirectComp from './DirectComp.svelte';\n</script>\n<DirectComp  />\n",
        incomplete_parent_source: "<script lang=\"ts\">\nimport DirectComp from './DirectComp.svelte';\n</script>\n<DirectComp ",
    },
    D1CarrierCase {
        name: "svelte-js",
        extension: "svelte",
        language_id: "svelte",
        index_label: "zIndex",
        child_source: "<script>\nconst internalOnly = true;\n/** @type {{ label: string, zIndex?: number }} */\nlet { label, zIndex } = $props();\n</script>\n<p>{label}</p>\n",
        stale_child_source: "<script>\n/** @type {{ staleOnly: string }} */\nlet { staleOnly } = $props();\n</script>\n",
        opened_parent_source: "<script>\nimport BeforeComp from './BeforeComp.svelte';\n</script>\n<BeforeComp  />\n",
        edited_parent_source: "<script>\nimport DirectComp from './DirectComp.svelte';\n</script>\n<DirectComp  />\n",
        incomplete_parent_source: "<script>\nimport DirectComp from './DirectComp.svelte';\n</script>\n<DirectComp ",
    },
];

/// Liveness escape for the "does not wait for a blocked provider update" probes.
///
/// These tests prove a NON-BLOCKING property, and they prove it STRUCTURALLY: the
/// blocked provider update is released only AFTER the probe's result is in hand, so
/// any probe that genuinely waited on that update can never complete — it hangs
/// forever, and ANY finite bound catches it. The bound therefore exists purely to
/// turn that hang into a test failure instead of a stuck suite.
///
/// It is deliberately NOT a latency assertion. The previous 250ms bound read as one,
/// and on a loaded machine it fired against completions that were correctly
/// non-blocking but merely slow — a false failure that says nothing about the
/// property under test. Do not tighten this into a performance budget; if request
/// latency needs a bound, that belongs in a separate, explicitly-named latency test.
const BLOCKED_PROVIDER_PROBE_LIVENESS: std::time::Duration = std::time::Duration::from_secs(30);

// =========================================================================
// D5 — slot-name completion contracts (real parses, server-owned items)
// =========================================================================

const D5_CHILD_SOURCE: &str = "<script setup lang=\"ts\">\ndefineSlots<{\n  header(props: { title: string; count: number }): any;\n  default(props: { body: string }): any;\n  mySlot(props: { note: string }): any;\n}>()\n</script>\n<template>\n  <header><slot name=\"header\" title=\"hdr\" :count=\"1\" /></header>\n  <main><slot body=\"main\" /></main>\n</template>\n";

async fn d5_complete_labels(files: &[(&str, &str, &str)], doc: &str, needle: &str) -> Vec<String> {
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(files).await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, doc);
    server.publish_import_dependencies_settled(&uri).await;
    let source = files
        .iter()
        .find(|(path, _, _)| path == &doc)
        .map(|(_, _, source)| *source)
        .expect("document must be one of the served files");
    let cursor = source.find(needle).unwrap_or_else(|| {
        panic!("needle {needle:?} must exist in {doc}");
    }) + needle.len();
    let position = LineIndex::new_utf16(source)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let labels = completion_labels(
        server
            .completion(completion_params(&uri, position, None))
            .await
            .expect("completion succeeds"),
    );
    drain_handle.abort();
    drop(service);
    labels
}

// =========================================================================
// D3 — slot-name token hover from the child's declared slots surface
// =========================================================================

const D3_CHILD_SOURCE: &str = "<script setup lang=\"ts\">\ndefineSlots<{\n  header(props: { title: string; count: number }): any;\n  default(props: { body: string }): any;\n  /** camelCase declare; template may use kebab `#my-slot` */\n  mySlot(props: { note: string }): any;\n}>()\n</script>\n<template>\n  <header><slot name=\"header\" title=\"hdr\" :count=\"1\" /></header>\n  <main><slot body=\"main\" /></main>\n</template>\n";

const D3_PARENT_SOURCE: &str = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #header=\"{ title, count: slotCount }\">\n      <span>{{ title }}:{{ slotCount }}</span>\n    </template>\n    <template #default=\"{ body }\">\n      <p>{{ body }}</p>\n    </template>\n    <template #my-slot=\"{ note }\">\n      <em>{{ note }}</em>\n    </template>\n    <template #nope=\"{ ghost }\">\n      <i>{{ ghost }}</i>\n    </template>\n  </MyComp>\n  <MyComp>\n    <template v-slot:header=\"{ title }\">\n      <b>{{ title }}</b>\n    </template>\n  </MyComp>\n</template>\n";

async fn d3_hover_text(needle: &str, character_shift: u32) -> Option<String> {
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", D3_CHILD_SOURCE),
        ("src/App.vue", "vue", D3_PARENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let mut position = find_document_position(server, &app_uri, needle, 0);
    position.character += character_shift;
    let hover = server
        .hover(hover_params(&app_uri, position))
        .await
        .expect("hover request should succeed");
    let text = hover.map(|h| hover_text(Some(h)));
    drain_handle.abort();
    drop(service);
    text
}

// =========================================================================
// D3/D4 Svelte parity pins — snippet names, render callsites, parameters
// (provider-rail round trips through the mapped carrier projection)
// =========================================================================

const D3_SVELTE_SOURCE: &str = "<script lang=\"ts\">\n  let items = $state([1, 2]);\n</script>\n\n{#snippet row(item: number)}\n  <li>{item}</li>\n{/snippet}\n\n<ul>\n  {#each items as it}\n    {@render row(it)}\n  {/each}\n</ul>\n";

// =========================================================================
// Svelte template-attribute navigation: TypeProvider + source-map path
// =========================================================================
//
// Svelte markup tokens resolve through the carrier IDE projection
// (`.svelte.tsx` / `.svelte.jsx`): the mock provider stands in for tsgo,
// seeded at the exact tsx offset the request computes, and the assertion
// proves the round trip maps range-exactly back onto the authored `.svelte`
// span. Tokens the projector strips or synthesizes (component `on:` event
// names, valued `class:`/`style:` names, the `bind:this` keyword) carry no
// resolvable symbol — native resolution must not fabricate a target and the
// provider has no declaration for them: fail closed, no link.

/// Seed the mock provider so a definition query at the `query` needle maps to
/// the SAME-FILE authored `target` span, then assert goto_definition returns
/// that span range-exactly. Both the query and target tsx offsets are computed
/// through the pinned carrier mapper, so the test proves routing + round-trip
/// mapping integrity (the real tsgo answer shape is covered by the
// real-provider carrier suites).
async fn assert_svelte_same_file_definition(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    uri: &Uri,
    query: (&str, usize),
    target: &str,
) {
    let position = find_document_position(server, uri, query.0, query.1);
    let ctx = synced_type_provider_context(server, uri).await;
    let query_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .unwrap_or_else(|| {
        let content = server
            .documents
            .provider_surfaces()
            .current_snapshot(&ctx.tsx_path)
            .map(|s| s.provider_content.clone())
            .unwrap_or_default();
        panic!("query token must stay mapped into the IDE surface\n{content}")
    });
    let target_range = range_for_authored_snippet(server, uri, target);
    let target_start = merge::carrier_position_to_tsx_offset_validated(
        &target_range.start,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("target token must stay mapped into the IDE surface");
    provider.set_definitions(
        &ctx.tsx_path,
        query_offset,
        vec![TypeLocation {
            path: ctx.tsx_path.clone(),
            start: target_start,
            end: target_start + target.len() as u32,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("a mapped svelte token should resolve");
    let locations = definition_locations(response);
    let hit = locations
        .iter()
        .find(|loc| loc.uri == *uri)
        .unwrap_or_else(|| panic!("definition should stay in the same file: {locations:?}"));
    assert_eq!(
        hit.range, target_range,
        "definition must land range-exact on the authored `{target}`"
    );
}

/// Seed the mock provider so a definition query at the `query` needle in the
/// parent answers with the child's IDE surface at the mapped prop token (the
/// shape a real tsgo returns — the definition merge maps `.verter.ts` API
/// locations fail-closed by design, foreign IDE surfaces through the pinned
/// generation), then assert goto_definition maps range-exactly onto the
/// authored child prop declaration in the `.svelte` source.
async fn assert_svelte_child_prop_definition(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    workspace_id: &str,
    app_uri: &Uri,
    query: (&str, usize),
    child_rel_path: &str,
    child_target: &str,
) {
    let position = find_document_position(server, app_uri, query.0, query.1);
    let child_uri = workspace_uri(workspace_id, child_rel_path);
    // Sync BOTH IDE surfaces through the test seam (the same surfaces the
    // request pins): the parent's for the query offset, the child's for the
    // foreign-surface target token.
    server.sync_ide_to_provider(app_uri).await;
    server.sync_ide_to_provider(&child_uri).await;
    let ctx = synced_type_provider_context(server, app_uri).await;
    let query_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("query token must stay mapped into the IDE surface");
    let child_ide_path = server
        .active_ide_path_for_uri(&child_uri)
        .expect("child IDE path is live");
    let child_snapshot = server
        .documents
        .provider_surfaces()
        .current_snapshot(&child_ide_path)
        .expect("child CarrierIde surface recorded");
    let child_target_range = range_for_authored_snippet(server, &child_uri, child_target);
    let child_ctx = synced_type_provider_context(server, &child_uri).await;
    let child_target_start = merge::carrier_position_to_tsx_offset_validated(
        &child_target_range.start,
        &child_ctx.carrier_line_index,
        &child_ctx.mapper,
        &child_ctx.tsx_line_index,
    )
    .unwrap_or_else(|| {
        panic!(
            "child target `{child_target}` must stay mapped into the child IDE surface:\n{}",
            child_snapshot.provider_content
        )
    });
    provider.set_definitions(
        &ctx.tsx_path,
        query_offset,
        vec![TypeLocation {
            path: child_ide_path.clone(),
            start: child_target_start,
            end: child_target_start + child_target.len() as u32,
        }],
    );

    let response = server
        .goto_definition(goto_definition_params(app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("a mapped svelte component prop should resolve");
    let locations = definition_locations(response);
    let hit = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .unwrap_or_else(|| panic!("definition should point to the child: {locations:?}"));
    assert_eq!(
        hit.range, child_target_range,
        "definition must land range-exact on the authored child `{child_target}`"
    );
}

/// A stripped/synthesized svelte token has no symbol a provider could resolve:
/// native resolution must not fabricate a target (the Vue file-start-fallback
/// class of bug) and the provider (unseeded mock) answers nothing, so
/// goto_definition fails closed — no link, no mis-mapped affordance.
async fn assert_svelte_token_fails_closed(
    server: &VerterLanguageServer,
    uri: &Uri,
    query: (&str, usize),
) {
    let position = find_document_position(server, uri, query.0, query.1);
    // Force the provider context to exist (the request path builds it anyway);
    // nothing is seeded, so any answer is a native fabrication.
    let _ctx = synced_type_provider_context(server, uri).await;
    let response = server
        .goto_definition(goto_definition_params(uri, position))
        .await
        .expect("goto definition should succeed");
    assert!(
        response.is_none(),
        "an unmapped/synthetic token must fail closed with no link, got {response:?}"
    );
}

async fn make_svelte_definition_server(
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    tower_lsp_server::LspService<VerterLanguageServer>,
    tokio::task::JoinHandle<()>,
    Arc<MockTypeProvider>,
    String,
) {
    let owned: Vec<(&str, &str, &str)> = files
        .iter()
        .map(|(path, source)| (*path, "svelte", *source))
        .collect();
    make_definition_test_server(&owned).await
}

const SVELTE_TS_SNIPPET_SOURCE: &str = "<script lang=\"ts\">\n  let item = 'x';\n</script>\n{#snippet rowSnippet(thing: string)}\n  <li>{thing}</li>\n{/snippet}\n<ul>{@render rowSnippet(item)}</ul>\n";
const SVELTE_JS_SNIPPET_SOURCE: &str = "<script>\n  let item = 'x';\n</script>\n{#snippet rowSnippet(thing)}\n  <li>{thing}</li>\n{/snippet}\n<ul>{@render rowSnippet(item)}</ul>\n";

const SVELTE_TS_ON_SOURCE: &str = "<script lang=\"ts\">\n  function handlePick(e: MouseEvent) { void e }\n</script>\n<button on:click={handlePick}>x</button>\n";
const SVELTE_JS_ON_SOURCE: &str = "<script>\n  function handlePick(e) { void e }\n</script>\n<button on:click={handlePick}>x</button>\n";

const SVELTE_TS_BIND_THIS_SOURCE: &str = "<script lang=\"ts\">\n  let boxEl: HTMLElement | undefined = $state();\n</script>\n<div bind:this={boxEl}>x</div>\n";
const SVELTE_JS_BIND_THIS_SOURCE: &str =
    "<script>\n  let boxEl = $state();\n</script>\n<div bind:this={boxEl}>x</div>\n";

const SVELTE_TS_USE_SOURCE: &str = "<script lang=\"ts\">\n  function tooltipAction(node: HTMLElement, label: string) { void node; void label }\n</script>\n<div use:tooltipAction={'hint'}>x</div>\n";
const SVELTE_JS_USE_SOURCE: &str = "<script>\n  function tooltipAction(node, label) { void node; void label }\n</script>\n<div use:tooltipAction={'hint'}>x</div>\n";

const SVELTE_TS_CLASS_SOURCE: &str =
    "<script lang=\"ts\">\n  let isActive = true;\n</script>\n<div class:isActive>x</div>\n";
const SVELTE_JS_CLASS_SOURCE: &str =
    "<script>\n  let isActive = true;\n</script>\n<div class:isActive>x</div>\n";

const SVELTE_TS_STYLE_SOURCE: &str =
    "<script lang=\"ts\">\n  let accentColor = 'red';\n</script>\n<div style:accentColor>x</div>\n";
const SVELTE_JS_STYLE_SOURCE: &str =
    "<script>\n  let accentColor = 'red';\n</script>\n<div style:accentColor>x</div>\n";

const SVELTE_TS_CHILD_SOURCE: &str = "<script lang=\"ts\">\n  let { title, onclick }: { title: string; onclick?: (e: MouseEvent) => void } = $props();\n</script>\n<h1>{title}</h1>\n";
const SVELTE_JS_CHILD_SOURCE: &str =
    "<script>\n  let { title, onclick } = $props();\n</script>\n<h1>{title}</h1>\n";

const SVELTE_TS_PARENT_PROP_SOURCE: &str = "<script lang=\"ts\">\n  import Child from './Child.svelte';\n  let t = 'x';\n</script>\n<Child title={t} />\n";
const SVELTE_JS_PARENT_PROP_SOURCE: &str = "<script>\n  import Child from './Child.svelte';\n  let t = 'x';\n</script>\n<Child title={t} />\n";

const SVELTE_TS_PARENT_BIND_SOURCE: &str = "<script lang=\"ts\">\n  import Child from './Child.svelte';\n  let t = $state('x');\n</script>\n<Child bind:title={t} />\n";
const SVELTE_JS_PARENT_BIND_SOURCE: &str = "<script>\n  import Child from './Child.svelte';\n  let t = $state('x');\n</script>\n<Child bind:title={t} />\n";

const SVELTE_TS_PARENT_ONEVENT_SOURCE: &str = "<script lang=\"ts\">\n  import Child from './Child.svelte';\n  function handlePick(e: MouseEvent) { void e }\n</script>\n<Child onclick={handlePick} />\n";
const SVELTE_JS_PARENT_ONEVENT_SOURCE: &str = "<script>\n  import Child from './Child.svelte';\n  function handlePick(e) { void e }\n</script>\n<Child onclick={handlePick} />\n";

const SVELTE_TS_PARENT_ON_EVENT_SOURCE: &str = "<script lang=\"ts\">\n  import Child from './Child.svelte';\n  function handlePick(e: CustomEvent) { void e }\n</script>\n<Child on:pick={handlePick} />\n";
const SVELTE_JS_PARENT_ON_EVENT_SOURCE: &str = "<script>\n  import Child from './Child.svelte';\n  function handlePick(e) { void e }\n</script>\n<Child on:pick={handlePick} />\n";

// =========================================================================
// W02 — a carrier with a MISSING IDE projection heals on the FIRST
// interactive request, not only after a debounced coordinator tick, a
// pending-snapshot drain, a reopen, or a restart.
//
// How these tests EXCLUDE background repair as the explanation: both heal
// paths that already exist are STRUCTURALLY unreachable here, not merely
// slow. (1) The debounced coordinator loop (spawned by
// `VerterLanguageServer::new`) acts only on files signalled through
// `sync_coordinator.signal(..)` — sent exclusively by the SERVER handlers
// `handle_did_open` / `handle_did_change`, which these tests never invoke
// (they drive the REGISTRY `documents.did_open` / `documents.did_change`
// directly, the same ingress the coordinator-recovery tests use), so the
// coordinator's pending map stays empty and its tick never calls
// `install_missing_carrier_projection`. (2) The pending-snapshot drain and
// the workspace scanner run only from `initialized()` / background init,
// which these tests never call. The projection-precondition asserts after
// every commit prove the document is still projection-less at the moment
// the interactive request is issued.
// =========================================================================

/// The compute half of `set_type_hover_at_vue_position`, run on a TWIN server:
/// resolve the provider (path, offset) a healed surface will serve for
/// `position` — same bytes, same compile profile, same resolver root, same
/// provider kind ⇒ the identical deterministic mapping — WITHOUT touching the
/// server under test (the seeding helper would itself compile + install the
/// projection and destroy the broken-state precondition).
fn provider_target_via_twin_server(
    kind: crate::TypeProviderKind,
    path: &str,
    language_id: &str,
    source: &str,
    needle: &str,
    delta: usize,
) -> (String, u32, Position) {
    let twin_provider = Arc::new(MockTypeProvider::new());
    let twin_service = make_hover_test_service_with_kind(twin_provider, kind);
    let twin = twin_service.inner();
    install_test_resolver(twin);
    let uri: Uri = format!("file://{path}").parse().expect("valid twin uri");
    let _ = twin.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: language_id.to_string(),
        version: 1,
        text: source.to_string(),
    });
    assert!(
        twin.documents.get_projection(&uri).is_some(),
        "twin precondition: the FIXED source must compile — otherwise the heal \
         under test has no healthy surface to converge to"
    );
    let position = find_document_position(twin, &uri, needle, delta);
    let ctx = synced_type_provider_context_surface_only(twin, &uri);
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("twin: the healthy surface must map the hover position");
    (ctx.tsx_path.clone(), tsx_offset, position)
}

/// Shared fixture for the definition transient-recovery pair: a member access
/// (`foo.bar`) whose receiver the native path could resolve but whose MEMBER
/// only the provider can — the exact asymmetry of the reported defect ("`foo`
/// works (sometimes) but `bar` NEVER works" while hover at the same cursor
/// shows the correct type). Returns `(uri, member_position, expected_decl_range)`
/// with the provider scripted to answer the member's declaration span inside
/// the generated TSX (copied authored text), so the full carrier-IDE reverse
/// map runs on recovery.
fn seed_member_definition_fixture(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    path: &str,
) -> (Uri, Position, Range) {
    let app_source = r#"<script setup lang="ts">
const foo = { bar: 1 }
</script>
<template><div>{{ foo.bar }}</div></template>
"#;
    let app_uri = open_test_vue(server, path, app_source);
    let vue_li = crate::documents::line_index::LineIndex::new_utf16(app_source);

    // Cursor: the MEMBER `bar` in the template `{{ foo.bar }}`.
    let usage_off = app_source.find("{{ foo.bar }}").expect("member usage") + "{{ foo.".len();
    let member_position = vue_li
        .offset_to_position(usage_off as u32)
        .expect("usage position");

    // Expected result: the authored `bar` in `const foo = { bar: 1 }`.
    let decl_off = app_source.find("{ bar: 1 }").expect("member decl") + "{ ".len();
    let expected_decl_range = Range {
        start: vue_li
            .offset_to_position(decl_off as u32)
            .expect("decl start"),
        end: vue_li
            .offset_to_position((decl_off + "bar".len()) as u32)
            .expect("decl end"),
    };

    // Script the provider: definition AND type-definition at the member's
    // generated offset answer the declaration's span INSIDE the generated TSX
    // (copied authored script text — the shape a real engine returns for a
    // local object member). Both tables are scripted so the same fixture backs
    // the definition and type-definition recovery pairs.
    let ctx = synced_type_provider_context_surface_only(server, &app_uri);
    let usage_tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &member_position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("member usage should map into the generated TSX");
    let tsx_decl_off = ctx
        .tsx_content
        .find("{ bar: 1 }")
        .expect("authored decl is copied into the TSX")
        + "{ ".len();
    let decl_location = crate::type_provider::protocol::TypeLocation {
        path: ctx.tsx_path.clone(),
        start: tsx_decl_off as u32,
        end: (tsx_decl_off + "bar".len()) as u32,
    };
    provider.set_definitions(&ctx.tsx_path, usage_tsx_offset, vec![decl_location.clone()]);
    provider.set_type_definitions(&ctx.tsx_path, usage_tsx_offset, vec![decl_location]);
    (app_uri, member_position, expected_decl_range)
}

fn definition_params(uri: &Uri, position: Position) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn definition_locations_of(resp: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match resp {
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

/// Byte offset → LSP position for a plain ASCII test source.
#[cfg(test)]
fn ascii_position(source: &str, needle: &str) -> Position {
    let offset = source.find(needle).expect("needle present");
    let before = &source[..offset];
    let line = before.matches('\n').count() as u32;
    let character = before
        .rsplit('\n')
        .next()
        .map(|tail| tail.len() as u32)
        .unwrap_or(0);
    Position { line, character }
}

/// Build the LSP completion item a tsserver-family provider's resolve handle
/// becomes after `merge_completions`: the provider-NEUTRAL `verter_resolve`
/// envelope with a `TsserverEntry` resolve key.
#[cfg(test)]
fn tsserver_resolve_envelope_item(
    provider_id: &str,
    provider_path: &str,
    entry_name: &str,
) -> CompletionItem {
    let provider_data = serde_json::to_value(
        crate::type_provider::protocol::CompletionResolveData::TsserverEntry {
            name: entry_name.to_string(),
            source: Some("vue".to_string()),
            data: Some(serde_json::json!({ "exportName": entry_name })),
            offset: 0,
        },
    )
    .expect("resolve key serializes");
    CompletionItem {
        label: entry_name.to_string(),
        data: Some(serde_json::json!({
            "verter_resolve": {
                "kind": "type_provider",
                "provider_id": provider_id,
                "provider_path": provider_path,
                "provider_data": provider_data,
            }
        })),
        ..Default::default()
    }
}

/// Build the LSP completion item a TSGO (upstream-LSP) provider's resolve handle
/// becomes after `merge_completions`: the provider-NEUTRAL `verter_resolve`
/// envelope with an `Lsp { label, data }` resolve key — the ONLY key shape real
/// `TsgoTypeProvider::resolve_completion` accepts. Tests must model the real
/// per-provider data shape, not feed `TsserverEntry` to a tsgo provider.
#[cfg(test)]
pub(super) fn tsgo_resolve_envelope_item(
    provider_id: &str,
    provider_path: &str,
    entry_name: &str,
) -> CompletionItem {
    let provider_data =
        serde_json::to_value(crate::type_provider::protocol::CompletionResolveData::Lsp {
            label: entry_name.to_string(),
            data: serde_json::json!({ "exportName": entry_name }),
        })
        .expect("resolve key serializes");
    CompletionItem {
        label: entry_name.to_string(),
        data: Some(serde_json::json!({
            "verter_resolve": {
                "kind": "type_provider",
                "provider_id": provider_id,
                "provider_path": provider_path,
                "provider_data": provider_data,
            }
        })),
        ..Default::default()
    }
}

// ── external-TS membership reconciler: production-path coverage ──

/// A small `.vue` carrier source used by the membership production-path tests.
const MEMBERSHIP_TEST_VUE: &str =
    "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n\
     <template><div>{{ msg }}</div></template>\n";

struct WatchedDependencyFixture {
    _temp: tempfile::TempDir,
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    provider: Arc<MockTypeProvider>,
    root: String,
    published: Arc<parking_lot::Mutex<Vec<PublishDiagnosticsParams>>>,
    published_changed: Arc<tokio::sync::Notify>,
    sync_complete: Arc<parking_lot::Mutex<Vec<u64>>>,
    store_changed: Arc<std::sync::atomic::AtomicUsize>,
    drain: tokio::task::JoinHandle<()>,
}

impl Drop for WatchedDependencyFixture {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

async fn watched_dependency_fixture(helper_exists: bool) -> WatchedDependencyFixture {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::test_utils::canonical_test_path(temp.path());
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("tsconfig.json"), "{}").unwrap();
    if helper_exists {
        std::fs::write(temp.path().join("src/helper.ts"), "export const value = 1;").unwrap();
    }
    let provider = Arc::new(MockTypeProvider::new());
    let provider_for_server = provider.clone();
    let (mut service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::new(VerterHost::new_standalone(HostConfig::default())),
                type_provider: Some(provider_for_server.clone()),
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                type_provider_topology: crate::TypeProviderTopology::implied_by(
                    crate::TypeProviderKind::Tsserver,
                ),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let mut socket = service.inner().outbound().wire();
    let response = tower_service::Service::call(
        &mut service,
        tower_lsp_server::jsonrpc::Request::build("initialize")
            .id(1)
            .params(serde_json::json!({ "processId": null, "rootUri": null, "capabilities": {} }))
            .finish(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(response.is_ok());
    service.inner().outbound().assume_initialized();
    let published = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = published.clone();
    let published_changed = Arc::new(tokio::sync::Notify::new());
    let captured_changed = published_changed.clone();
    let sync_complete = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured_complete = sync_complete.clone();
    let store_changed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured_store_changed = store_changed.clone();
    let drain = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            if message.method() == "$/verter/carrierStoreChanged" {
                captured_store_changed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            if message.method() == "$/verter/typeProviderSyncComplete" {
                let params = serde_json::to_value(message.params().unwrap()).unwrap();
                captured_complete
                    .lock()
                    .push(params["gen"].as_u64().unwrap());
            }
            if message.method() == "textDocument/publishDiagnostics" {
                let params = serde_json::from_value(
                    serde_json::to_value(message.params().unwrap()).unwrap(),
                )
                .unwrap();
                captured.lock().push(params);
                captured_changed.notify_one();
            }
        }
    });
    let server = service.inner();
    install_test_resolver_for_root(server, &root, Some(&format!("{root}/tsconfig.json")));
    if helper_exists {
        // The dependency reaches the provider before its importers record
        // their edges: loading it afterwards replaces the host entry and
        // drops the importers' reverse-dependency bucket.
        crate::workspace_scanner::resync_non_carrier_file(
            &format!("{root}/src/helper.ts"),
            &server.documents.host_arc(),
            server.project_sync.as_ref().unwrap(),
            server.documents.provider_surfaces(),
            &server.vfs_workspace,
            &server.provider_sync_states,
        )
        .await;
    }
    for (name, source) in [
        ("Consumer.vue", "<script setup lang=\"ts\">\nimport { value } from './helper';\n</script>\n<template>{{ value }}</template>"),
        ("Unrelated.vue", "<template><p>unrelated</p></template>"),
    ] {
        let id = format!("{root}/src/{name}");
        let uri = crate::uri::path_to_file_uri(&id).unwrap();
        std::fs::write(temp.path().join("src").join(name), source).unwrap();
        server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(), language_id: "vue".into(), version: 1, text: source.into(),
        });
        server.refresh_carrier_dependency_tracking(&id);
        server.ensure_current_file_synced(&uri).await;
        server.sync_coordinator.signal_diagnostics_only(id, uri.to_string(), tokio::time::Instant::now());
    }
    server
        .sync_coordinator
        .await_until(
            || {
                ["Consumer.vue", "Unrelated.vue"].iter().all(|name| {
                    server
                        .documents
                        .diagnostics_ready(&workspace_uri(&root, &format!("src/{name}")))
                }) && server.sync_coordinator.diag_tasks_live() == 0
            },
            || panic!("initial watched-file fixture diagnostics did not complete"),
        )
        .await;
    WatchedDependencyFixture {
        _temp: temp,
        service,
        provider,
        root,
        published,
        published_changed,
        sync_complete,
        store_changed,
        drain,
    }
}

async fn drain_pending_provider_sync_for(server: &VerterLanguageServer) {
    crate::server::drain_pending_snapshot_provider_sync(
        server.project_sync.as_ref(),
        &server.documents,
        &server.vfs_workspace,
        &server.provider_sync_states,
        &server.pending_snapshot_provider_sync,
        false,
        None,
        server.carrier_publish_coordinator.as_ref(),
        &server.carrier_transaction_coordinator,
    )
    .await;
}

struct ScanPublicationFixture {
    _temp: tempfile::TempDir,
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    provider: Arc<MockTypeProvider>,
    root: String,
    ready: Arc<parking_lot::Mutex<Vec<u64>>>,
    sync_complete: Arc<parking_lot::Mutex<Vec<u64>>>,
    signals: Arc<tokio::sync::Notify>,
    drain: tokio::task::JoinHandle<()>,
}

impl Drop for ScanPublicationFixture {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

impl ScanPublicationFixture {
    fn server(&self) -> &VerterLanguageServer {
        self.service.inner()
    }

    fn id(&self, path: &str) -> String {
        format!("{}/{path}", self.root)
    }

    /// Open `path` through the production `didOpen` handler, as a restarted
    /// client replays it.
    async fn open(&self, path: &str) -> Uri {
        let uri = workspace_uri(&self.root, path);
        let text = std::fs::read_to_string(self._temp.path().join(path)).unwrap();
        super::lifecycle::handle_did_open(
            self.server(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "vue".into(),
                    version: 1,
                    text,
                },
            },
        )
        .await;
        uri
    }

    async fn await_announced(&self, announced: &parking_lot::Mutex<Vec<u64>>, what: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                let signalled = self.signals.notified();
                tokio::pin!(signalled);
                signalled.as_mut().enable();
                if !announced.lock().is_empty() {
                    return;
                }
                signalled.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{what} was never announced"));
    }

    /// Level 1 of the readiness ladder. Init re-arms every open document's
    /// diagnostics just before announcing it, with the scan flag already
    /// raised, so a receipt observed after this point was certified during the
    /// scan.
    async fn await_ready_announced(&self) {
        self.await_announced(&self.ready, "$/verter/ready").await;
    }

    async fn await_sync_complete(&self) {
        self.await_announced(&self.sync_complete, "level 2").await;
    }

    /// Wait for `ready` while polling `uri`'s diagnostics status by name, the
    /// way a restarted client waiting on that document does: the existing
    /// per-document analysis request names it, then the unchanged statistics
    /// snapshot is read. The poll cadence is the client's; the coordinator's
    /// own receipts decide the wait.
    async fn await_while_polling(&self, uri: &Uri, ready: impl FnMut() -> bool, what: &str) {
        let poll = async {
            loop {
                let _ = self
                    .server()
                    .get_analysis(crate::server::protocol_types::GetAnalysisParams {
                        uri: uri.to_string(),
                    })
                    .await;
                let _ = self
                    .server()
                    .get_statistics(Some(
                        crate::server::protocol_types::StatisticsRequestParams {
                            include_events: false,
                            scope: None,
                        },
                    ))
                    .await;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        };
        tokio::select! {
            () = poll => unreachable!("the status poll never ends"),
            () = self.server().sync_coordinator.await_until(ready, || panic!("{what}")) => {}
        }
    }
}

/// A real workspace for the production init path: a `Target` carrier that
/// imports a `Child` carrier, and an `Unrelated` carrier in another directory
/// that only the workspace scan ever touches.
///
/// `eager_import_warmup: false` turns off the `didOpen` handler's own inline
/// warmup of imported carriers, which would otherwise deliver `Child` inside
/// the open itself. The import-dependency publication — the route that mints
/// the DependencyReady receipt — is unaffected.
async fn scan_publication_fixture(eager_import_warmup: bool) -> ScanPublicationFixture {
    let temp = tempfile::tempdir().unwrap();
    let root = crate::test_utils::canonical_test_path(temp.path());
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::create_dir_all(temp.path().join("far")).unwrap();
    std::fs::write(temp.path().join("tsconfig.json"), "{}").unwrap();
    for (path, source) in [
        (
            "src/Child.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><p>{{ label }}</p></template>",
        ),
        (
            "src/Target.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue';\n</script>\n<template><Child label=\"target\" /></template>",
        ),
        ("far/Unrelated.vue", "<template><p>unrelated</p></template>"),
    ] {
        std::fs::write(temp.path().join(path), source).unwrap();
    }

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = Arc::new(VerterHost::new(HostConfig::default(), Arc::clone(&ws) as _));
    let mock = Arc::new(MockTypeProvider::new());
    let provider: Arc<dyn TypeProvider> = Arc::clone(&mock) as _;
    let (mut service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host),
                type_provider: Some(Arc::clone(&provider)),
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                type_provider_topology: crate::TypeProviderTopology::implied_by(
                    crate::TypeProviderKind::Tsserver,
                ),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: !eager_import_warmup,
            },
        )
    });
    let mut socket = service.inner().outbound().wire();
    let response = tower_service::Service::call(
        &mut service,
        tower_lsp_server::jsonrpc::Request::build("initialize")
            .id(1)
            .params(serde_json::json!({ "processId": null, "rootUri": null, "capabilities": {} }))
            .finish(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(response.is_ok());
    service.inner().outbound().assume_initialized();

    let ready = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sync_complete = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let signals = Arc::new(tokio::sync::Notify::new());
    let drain = tokio::spawn({
        let ready = Arc::clone(&ready);
        let sync_complete = Arc::clone(&sync_complete);
        let signals = Arc::clone(&signals);
        async move {
            while let Some(message) = socket.next().await {
                let announced = match message.method() {
                    "$/verter/ready" => &ready,
                    "$/verter/typeProviderSyncComplete" => &sync_complete,
                    _ => continue,
                };
                let params = serde_json::to_value(message.params().unwrap()).unwrap();
                announced.lock().push(params["gen"].as_u64().unwrap());
                signals.notify_waiters();
            }
        }
    });

    let server = service.inner();
    server.swap_vfs_workspace(ws);
    server.vite_config_options.lock().await.enabled = false;
    *server.workspace_roots.lock().await = vec![crate::uri::path_to_file_uri(&root)
        .expect("workspace URI")
        .as_str()
        .to_string()];
    ScanPublicationFixture {
        _temp: temp,
        service,
        provider: mock,
        root,
        ready,
        sync_complete,
        signals,
        drain,
    }
}

/// Open a `.svelte` carrier document into the server's host. The path's
/// `.svelte` extension classifies it as the Svelte carrier row (the editor
/// `language_id` is only authoritative for the Vue carrier).
fn open_test_svelte(server: &VerterLanguageServer, path: &str, source: &str) -> Uri {
    let uri: Uri = format!("file://{path}").parse().expect("valid test uri");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: source.to_string(),
    });
    uri
}

// ─── Request-scoped provider-surface snapshot: fail-closed interactive queries ───
//
// Interactive provider-backed queries (hover/completion/definition/…) must be
// built from ONE immutable, generation-stamped `ProviderSurfaceSnapshot` and
// re-validated after the provider await — a concurrent re-sync or close must
// never let a handler serve a result mapped through a torn
// (path, content, mapper) tuple assembled from independent live reads.

/// Shared carrier fixture for the request-surface tests.
const REQUEST_SURFACE_APP: &str = r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
"#;

/// Build a tsgo-kind mock service with an owner-resolved, provider-synced
/// carrier: the IDE surface has been delivered to the provider through the
/// direct-open path, so interactive queries can route to it.
async fn make_request_surface_carrier() -> (
    tower_lsp_server::LspService<VerterLanguageServer>,
    Arc<MockTypeProvider>,
    Uri,
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(server, "/workspace/src/App.vue", REQUEST_SURFACE_APP);
    server.sync_ide_to_provider(&uri).await;
    (service, provider, uri)
}

// =========================================================================
// $/verter/getBindingTypes — structured provider display_signature
// (P1-01 / P1-02 / P1-04) + the co-migrated v-bind completion detail (S2)
// =========================================================================

/// Seed a FULL structured provider hover at a Vue position. The P1 fixtures
/// control every structured field independently of the rendered `contents`
/// blob — that independence is exactly what P1-02 asserts.
fn set_structured_hover_at_vue_position(
    server: &VerterLanguageServer,
    provider: &MockTypeProvider,
    uri: &Uri,
    position: Position,
    info: crate::type_provider::protocol::HoverInfo,
) {
    let ctx = synced_type_provider_context_surface_only(server, uri);
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("vue position should map to tsx");
    provider.set_hover(&ctx.tsx_path, tsx_offset, Some(info));
}

/// Shared setup for the virtual-file request-surface tests: a synced carrier
/// with a recorded CarrierIde surface, plus a `verter-virtual://` document
/// opened over `virtual_content` (which may deliberately differ from the
/// recorded surface to model a stale tab).
async fn make_virtual_file_fixture(
    recorded_content: &str,
    virtual_content: &str,
) -> (
    tower_lsp_server::LspService<VerterLanguageServer>,
    Arc<MockTypeProvider>,
    Uri,
    String,
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let tsx_path = "/workspace/src/App.vue.tsx".to_string();
    let source_uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: source_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div/></template>".to_string(),
    });
    server.provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some(tsx_path.clone()),
            api_path: None,
            decl_path: None,
            shadow_path: None,
            ide_background_loaded: true,
            api_background_loaded: false,
            decl_background_loaded: false,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            committed_api_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );
    let seed_revision = server.documents.snapshot_identity(&source_uri);
    if let Some(sync) = server.project_sync.clone() {
        sync.open_tsx(&tsx_path, recorded_content)
            .await
            .expect("virtual fixture publishes through the production open path");
    }
    server.record_carrier_ide_snapshot_with_pin(
        seed_revision
            .as_ref()
            .map(|revision| (&source_uri, revision)),
        "/workspace/src/App.vue",
        &tsx_path,
        recorded_content,
        None,
    );

    let virtual_uri_str = format!(
        "verter-virtual://generated/App.vue.tsx?sourceUri={}",
        source_uri.as_str()
    );
    let virtual_uri: Uri = virtual_uri_str.parse().expect("virtual uri parses");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: virtual_uri.clone(),
        language_id: "typescriptreact".to_string(),
        version: 1,
        text: virtual_content.to_string(),
    });
    (service, provider, virtual_uri, tsx_path)
}

/// Shared setup for the FOREIGN-carrier mapping tests: parent + child carriers
/// synced (both CarrierIde surfaces recorded), with the mock provider primed to
/// return a definition location inside the CHILD's IDE surface for a query at
/// the parent's mapped `msg` position.
async fn make_foreign_mapping_fixture() -> (
    tower_lsp_server::LspService<VerterLanguageServer>,
    Arc<MockTypeProvider>,
    Uri,
    Position,
    String,
    String,
) {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    // A SHARED fixture helper: both callers run in their own PROCESS under nextest, so a
    // function-local `AtomicUsize` starting at 0 minted `/verter_foreign_mapping_0` in
    // BOTH — one store dir, one manifest.json, concurrently. Route through the
    // process-varying seam.
    let workspace_root = unique_server_ws_root("foreign_mapping");
    let tsconfig = format!("{workspace_root}/tsconfig.json");
    install_test_resolver_for_root(server, &workspace_root, Some(&tsconfig));

    let parent_canonical = format!("{workspace_root}/src/App.vue");
    let child_canonical = format!("{workspace_root}/src/Child.vue");
    let parent_uri = open_test_vue(server, &parent_canonical, REQUEST_SURFACE_APP);
    let child_uri = open_test_vue(server, &child_canonical, REQUEST_SURFACE_APP);
    server.sync_ide_to_provider(&parent_uri).await;
    server.sync_ide_to_provider(&child_uri).await;

    let child_ide_path = server
        .active_ide_path_for_uri(&child_uri)
        .expect("child IDE path is live");
    let child_snapshot = server
        .documents
        .provider_surfaces()
        .current_snapshot(&child_ide_path)
        .expect("child CarrierIde surface recorded");
    // A definition target inside the CHILD's IDE surface: the mapped `msg`
    // declaration token.
    let child_target = child_snapshot
        .provider_content
        .find("msg")
        .expect("token present in child IDE content") as u32;

    // The parent query position + the tsx offset the handler will compute.
    // A STRING LITERAL position: Verter's own definition resolution yields
    // nothing there, so the merge consumes the PROVIDER locations (a Verter
    // same-file definition would short-circuit the merge and never reach the
    // foreign-mapping path under test).
    let position = find_document_position(server, &parent_uri, "'hello'", 1);
    let parent_ctx = synced_type_provider_context(server, &parent_uri).await;
    let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
        &position,
        &parent_ctx.carrier_line_index,
        &parent_ctx.mapper,
        &parent_ctx.tsx_line_index,
    )
    .expect("parent position maps to tsx");
    provider.set_definitions(
        &parent_ctx.tsx_path,
        tsx_offset,
        vec![crate::type_provider::protocol::TypeLocation {
            path: child_ide_path.clone(),
            start: child_target,
            end: child_target + 3,
        }],
    );
    (
        service,
        provider,
        parent_uri,
        position,
        child_ide_path,
        child_canonical,
    )
}

/// Record `content` as the child's current IDE surface over the child's source,
/// keeping the pinned surface's map.
fn record_foreign_child_surface(
    store: &crate::provider_surface_store::ProviderSurfaceStore,
    pinned: &crate::provider_surface_store::ProviderSurfaceSnapshot,
    child_canonical: &str,
    content: &str,
) {
    store.record(
        crate::provider_surface_store::RecordSurface::carrier_legacy(
            crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
            pinned.stamp.provider_path.to_string(),
            child_canonical.to_string(),
            Arc::from(content),
            pinned.source_map.as_ref().map(|map| (**map).clone()),
            Arc::from(REQUEST_SURFACE_APP),
        ),
    );
}

fn foreign_definition_params(parent_uri: &Uri, position: Position) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: parent_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

// =========================================================================
// D6 — directive-name hover + definition
// =========================================================================

const D6_PARENT_SOURCE: &str = "<script setup lang=\"ts\">\nimport { ref } from \"vue\";\nimport { vFocus } from \"./focus\";\nconst visible = ref(true);\nconst items = ref([1, 2]);\nconst color = ref(\"red\");\nconst vMyThing = (el: HTMLElement, binding: { value: string }) => {\n  void el;\n  void binding;\n};\n</script>\n<template>\n  <div v-if=\"visible\">\n    <span v-show=\"visible\">{{ color }}</span>\n    <li v-for=\"it in items\" :key=\"it\">{{ it }}</li>\n    <p v-html=\"color\"></p>\n    <u v-text=\"color\"></u>\n    <s v-pre>{{ raw }}</s>\n    <b v-once>{{ color }}</b>\n    <i v-memo=\"[color]\">{{ color }}</i>\n    <q v-cloak>{{ color }}</q>\n    <b v-my-thing=\"color\"></b>\n    <i v-nope=\"color\"></i>\n    <em v-focus></em>\n  </div>\n  <b v-else-if=\"items\">{{ color }}</b>\n  <i v-else>{{ color }}</i>\n</template>\n";

const D6_FOCUS_SOURCE: &str =
    "export const vFocus = {\n  mounted(el: HTMLElement) {\n    el.focus();\n  },\n};\n";

// =========================================================================
// D6 Svelte — directive keyword doc hovers + transition-family name hover
// =========================================================================

const D6_SVELTE_SOURCE: &str = "<script lang=\"ts\">\n  import { fade, fly } from \"svelte/transition\";\n  import { flip } from \"svelte/animate\";\n  function highlight(node: HTMLElement, params: { color: string }) {\n    void node;\n    void params;\n    return { destroy() {} };\n  }\n</script>\n\n<p use:highlight={{ color: \"red\" }}>x</p>\n<span transition:fade>y</span>\n<b in:fly={{ x: 10 }} out:fade>z</b>\n<li animate:flip>w</li>\n";

/// Write one LSP `Content-Length`-framed message.
async fn write_lsp_frame<W>(w: &Arc<tokio::sync::Mutex<W>>, msg: &serde_json::Value)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let body = serde_json::to_string(msg).unwrap();
    let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    let mut guard = w.lock().await;
    guard.write_all(frame.as_bytes()).await.unwrap();
    guard.flush().await.unwrap();
}

// ===========================================================================
// Dependency readiness (SurfaceReady / DependencyReady) handler protocol.
//
// Interactive handlers never START an import-set or barrel sync: publication is
// background-owned (open / edit / readiness-miss enqueue), mints the
// DependencyReady receipt (the import-set freshness memo) only from background
// completion, and handlers may only CAPTURE a committed receipt, JOIN an
// in-flight publication for the current revision, or answer without the
// provider. The cancel loop this kills: a request-started preamble whose p90
// sat on the definition deadline was cancelled, a cancelled pass never
// published its freshness memo, so the next identical request repeated the
// identical storm.
// ===========================================================================

const READINESS_CHILD_SOURCE: &str = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
const READINESS_PARENT_SOURCE: &str = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";

fn import_sync_verb_count(provider: &MockTypeProvider) -> usize {
    provider
        .calls()
        .iter()
        .filter(|c| {
            matches!(
                c,
                MockCall::OpenFile { .. } | MockCall::UpdateFile { .. } | MockCall::LoadFile { .. }
            )
        })
        .count()
}

// ===========================================================================
// Optional diagnostic request deadlines.
//
// Production feature requests have no latency deadline. Explicit test and
// diagnostic hosts may still install one to probe cancellation behavior.
// ===========================================================================

/// Build a language server over a mock provider, with `configure` applied to the
/// host config first.
/// A minimal SFC with one template interpolation, used by the deadline tests to
/// give a definition/hover a real position to resolve.
const DEADLINE_TEST_SOURCE: &str = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";

fn wedged_provider_server(
    provider: Arc<MockTypeProvider>,
    configure: impl FnOnce(&mut HostConfig),
) -> (
    tower_lsp_server::LspService<VerterLanguageServer>,
    Arc<VerterHost>,
) {
    let mut config = HostConfig::default();
    assert!(
        !config.audit_enabled,
        "these must exercise the production audit-off path"
    );
    configure(&mut config);
    let host = Arc::new(VerterHost::new_standalone(config));

    let type_provider: Arc<dyn TypeProvider> = provider;
    let host_for_server = Arc::clone(&host);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider)),
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
                mcp_port: None,
                type_provider_reason: Some("managed tsgo".into()),
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    (service, host)
}

// ── Rename completeness: refuse a partial edit set ─────────────────────

/// A JS (`@ts-check`) carrier whose binding lives in BOTH the script and the
/// markup. `jsValue` is authored three times: the declaration, the script
/// call argument, and the markup expression.
const RENAME_COMPLETENESS_VUE: &str = "<script setup>\n// @ts-check\nconst jsValue = { label: \"javascript\" };\nfunction renderJs(value) {\n  return value.label;\n}\nconst jsRendered = renderJs(jsValue);\n</script>\n\n<template>\n  <p>{{ jsRendered }} {{ jsValue.label }}</p>\n</template>\n";

const RENAME_COMPLETENESS_SVELTE: &str = "<script>\n// @ts-check\nlet jsValue = { label: \"javascript\" };\nfunction renderJs(value) {\n  return value.label;\n}\nlet jsRendered = renderJs(jsValue);\n</script>\n\n<p>{jsRendered} {jsValue.label}</p>\n";

/// The SET of authored `token` ranges in `source`, in the document's own
/// coordinates. Asserting against this set — never against a count — keeps a
/// future fourth occurrence from passing silently.
pub(super) fn authored_token_ranges(
    source: &str,
    token: &str,
) -> std::collections::BTreeSet<(u32, u32, u32, u32)> {
    let line_index = crate::documents::line_index::LineIndex::new_utf16(source);
    let mut ranges = std::collections::BTreeSet::new();
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut from = 0usize;
    while let Some(found) = source[from..].find(token) {
        let start = from + found;
        let end = start + token.len();
        from = start + 1;
        if start > 0 && is_word(bytes[start - 1]) {
            continue;
        }
        if end < bytes.len() && is_word(bytes[end]) {
            continue;
        }
        let s = line_index
            .offset_to_position(start as u32)
            .expect("position");
        let e = line_index.offset_to_position(end as u32).expect("position");
        ranges.insert((s.line, s.character, e.line, e.character));
    }
    ranges
}

/// The SET of ranges a rename transaction mutates in `uri`.
pub(super) fn rename_edit_ranges(
    edit: &WorkspaceEdit,
    uri: &Uri,
) -> std::collections::BTreeSet<(u32, u32, u32, u32)> {
    workspace_edit_triples(edit)
        .into_iter()
        .filter(|(edited, _, _)| {
            verter_span::path::fs_paths_equal(
                &crate::documents::uri_to_canonical_id(edited),
                &crate::documents::uri_to_canonical_id(uri),
            )
        })
        .map(|(_, range, _)| {
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// https://github.com/pikax/verter/issues/96 — the PRODUCTION ingress.
//
// The serve loop does not spawn a task per notification. `outbound::serve`
// pushes handler futures into an mpsc channel and polls them through
// `buffer_unordered` INLINE on the serve thread (documented on
// `SERVE_THREAD_STACK_BYTES` in `lib.rs`). `BufferUnordered` fills its queue
// from the channel WITHOUT polling, then polls the queued futures one at a
// time — and `handle_did_change` runs from entry through commit without
// pending (an uncontended `did_change_mutex.lock()` completes in the current
// poll, and the commit itself is a synchronous `block_in_place_if_available`).
//
// So handler k runs to completion before handler k+1 is polled at all. Any
// coalescing scheme that depends on later notifications having ANNOUNCED
// themselves before an earlier one commits is inert here: at commit time,
// handler k is the only handler that has ever been entered.
//
// That is why the document commit must not compile at all.

/// Frame `count` full-document `textDocument/didChange` notifications for
/// `uri`, versions `first_version..`, each carrying a distinguishable source.
fn did_change_burst_frames(uri: &Uri, first_version: i32, count: usize) -> (Vec<u8>, String) {
    let revision = |marker: i32| {
        format!(
            "<script setup lang=\"ts\">\nconst count = {marker}\n</script>\n\
             <template><div>{{{{ count }}}}</div></template>\n"
        )
    };
    let mut bytes = Vec::new();
    let mut last = String::new();
    for index in 0..count {
        let version = first_version + index as i32;
        last = revision(version);
        let body = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri.as_str(), "version": version },
                "contentChanges": [ { "text": last } ],
            },
        }))
        .expect("didChange frame serializes");
        bytes.extend_from_slice(
            format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes(),
        );
    }
    (bytes, last)
}

/// Drive an LSP session over duplex pipes through the REAL serve loop.
///
/// Returns the client-side write half (frames written here reach the server the
/// way a client's stdin does, through the frame reader → the serve loop's mpsc
/// channel → `buffer_unordered`) once the session is initialized, so a caller
/// can measure from a settled baseline.
async fn serve_over_duplex_initialized(
    service: tower_lsp_server::LspService<VerterLanguageServer>,
) -> (tokio::io::DuplexStream, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (server_stdin, mut client_to_server) = tokio::io::duplex(1 << 20);
    let (server_stdout, mut client_from_server) = tokio::io::duplex(1 << 20);
    let serve = tokio::spawn(async move {
        let outbound = service.inner().outbound().clone();
        crate::outbound::serve(server_stdin, server_stdout, service, outbound).await;
    });

    let body = serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "processId": null, "rootUri": null, "capabilities": {} },
    }))
    .expect("initialize frame serializes");
    client_to_server
        .write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes())
        .await
        .expect("initialize frame reaches the server");
    client_to_server.flush().await.expect("flush initialize");

    // Read until the initialize RESPONSE appears. This is the fence that proves
    // the serve loop is live and the session has left `Uninitialized`, where
    // ordinary notifications would be discarded rather than handled.
    let mut seen = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client_from_server.read(&mut chunk),
        )
        .await
        .expect("the server must answer initialize")
        .expect("server stdout must stay readable");
        assert!(
            read > 0,
            "the server closed stdout before answering initialize"
        );
        seen.extend_from_slice(&chunk[..read]);
        if String::from_utf8_lossy(&seen).contains("\"id\":1") {
            break;
        }
    }

    (client_to_server, serve)
}

/// Cold compile RUNS this host has started — the feature-independent rail
/// bumped once per cold run past the warm-hit consult.
///
/// It must be this rail and not the post-success compile tick: a compile that
/// FAILS returns before that tick, so a burst of malformed revisions could
/// execute a cold compile per keystroke while a tick-based counter reported
/// zero. A malformed intermediate revision is the ordinary state of a file
/// being typed, so the instrument has to see it.
fn cold_compile_runs(host: &Arc<VerterHost>) -> u64 {
    host.provenance_snapshot().compile_cold_runs
}

/// Build a provider-less server for the ingress measurement.
///
/// `type_provider: None` makes the coordinator's `project_sync` `None`, and
/// `sync_file` returns at its first statement in that case — so the debounced
/// coordinator can contribute NO compiles to the measurement however long the
/// serve loop takes. Semantic analysis stays off (its analyses run on a
/// separate host anyway). The document has no imports, so the background
/// import-dependency publication has nothing to walk.
fn ingress_measurement_server(
    host: &Arc<VerterHost>,
) -> tower_lsp_server::LspService<VerterLanguageServer> {
    let host_for_server = Arc::clone(host);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: None,
                project_sync_mode: ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::None,
                type_provider_topology: crate::TypeProviderTopology::implied_by(
                    crate::TypeProviderKind::None,
                ),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: true,
            },
        )
    });
    service
}

/// Wait for the cold-compile rail to reach a value and STAY there, then return
/// the delta from `before`.
///
/// The debounced refresh is asynchronous, so a bare sleep either flakes short or
/// pads every run. This polls for the first movement, then requires the rail to
/// hold still across a full debounce window — so a per-notification storm cannot
/// be sampled mid-flight and read as a small number.
async fn await_settled_cold_compiles(host: &Arc<VerterHost>, before: u64) -> u64 {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let quiet_for = std::time::Duration::from_millis(crate::sync_coordinator::DEBOUNCE_MS * 3);
    loop {
        let observed = cold_compile_runs(host) - before;
        if observed > 0 {
            tokio::time::sleep(quiet_for).await;
            let again = cold_compile_runs(host) - before;
            if again == observed {
                return observed;
            }
            continue;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the debounced coordinator never compiled the settled revision"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// Frame `count` full-document `didChange` notifications carrying revisions that
/// do NOT compile — the ordinary mid-edit state of a file being typed.
fn invalid_did_change_burst_frames(
    uri: &Uri,
    first_version: i32,
    count: usize,
) -> (Vec<u8>, String) {
    let mut bytes = Vec::new();
    let mut last = String::new();
    for index in 0..count {
        let version = first_version + index as i32;
        last = format!("<script setup lang=\"ts\">\nconst broken{version} = (((\n");
        let body = serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri.as_str(), "version": version },
                "contentChanges": [ { "text": last } ],
            },
        }))
        .expect("didChange frame serializes");
        bytes.extend_from_slice(
            format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes(),
        );
    }
    (bytes, last)
}

/// Whether the host still has no IDE TSX for `canonical_id` (a pure cached read).
fn service_projection_still_absent(host: &Arc<VerterHost>, canonical_id: &str) -> bool {
    host.get_ide(canonical_id, &verter_session::CompileProfile::default())
        .is_none()
}

fn lane_interleaving_deps(
    server: &VerterLanguageServer,
) -> crate::sync_coordinator::SyncCoordinatorDeps {
    crate::sync_coordinator::SyncCoordinatorDeps {
        documents: Arc::clone(&server.documents),
        dependency_receipts: Default::default(),
        project_sync: server.project_sync.clone(),
        needs_provider_sync: Arc::clone(&server.needs_deferred_sync),
        pending_snapshot_provider_sync: Arc::clone(&server.pending_snapshot_provider_sync),
        client: server.client.clone(),
        type_provider: server.type_provider.clone(),
        cached_verter_diags: Arc::clone(&server.cached_verter_diags),
        position_encoding: Arc::clone(&server.position_encoding),
        provider_sync_states: Arc::clone(&server.provider_sync_states),
        vfs_workspace: Arc::clone(&server.vfs_workspace),
        type_provider_kind: server.type_provider_kind,
        carrier_publish_coordinator: server.carrier_publish_coordinator.clone(),
        carrier_transaction_coordinator: Arc::clone(&server.carrier_transaction_coordinator),
    }
}

async fn lane_interleaving_fixture(
    name: &str,
) -> (
    tower_lsp_server::LspService<VerterLanguageServer>,
    Arc<MockTypeProvider>,
    Uri,
) {
    let provider = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_tsgo(provider.clone());
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(
        server,
        &format!("/workspace/src/{name}.vue"),
        REQUEST_SURFACE_APP,
    );
    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
    let generation = server
        .current_or_init_ide_sync_open_generation(&uri, &canonical_id)
        .await
        .unwrap();
    let _lease = server.ide_sync_repair_lease(&canonical_id, generation);
    server.sync_ide_to_provider(&uri).await;
    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(
        server,
        &provider,
        &uri,
        position,
        "const msg: string // provider lane answer",
    );
    provider.clear_calls();
    (service, provider, uri)
}

fn ide_application_count(provider: &MockTypeProvider, canonical_id: &str) -> usize {
    let path = verter_session_query::resolution::carrier_ide_provider_path(canonical_id, false);
    provider
        .calls()
        .iter()
        .filter(|call| {
            matches!(call,
        MockCall::OpenFile { path: p, .. } | MockCall::OpenFileBackground { path: p, .. }
        | MockCall::UpdateFile { path: p, .. } | MockCall::LoadFile { path: p, .. } if p == &path)
        })
        .count()
}

fn edit_interleaving_document(server: &VerterLanguageServer, uri: &Uri, version: i32, value: &str) {
    let source = REQUEST_SURFACE_APP.replace("'hello'", &format!("'{value}'"));
    assert!(server.documents.did_change(uri, version, &source).changed);
    let canonical_id = crate::documents::uri_to_canonical_id(uri);
    server.needs_ide_sync.insert(canonical_id);
}

mod actions;
mod completion;
mod diagnostics;
mod formatting;
mod general;
mod hover;
mod navigation;
mod synchronization;
mod transport;
mod workspace;
