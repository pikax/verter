//! Restart-interleaving coverage for the single-writer
//! [`ResilientProvider`](super::ResilientProvider).
//!
//! Every test drives the REAL production path — `ResilientProvider::new` spawns
//! the actor + crash monitor, a crash is tripped through the public crash-notify
//! handle, and the respawn is gated through a real [`ResilientBackend`]. The
//! tests are fully deterministic and contain NO wall-clock sleep:
//!
//! * the crash is tripped with `Notify::notify_one` (lossless — it stores a
//!   permit if the monitor has not parked yet);
//! * the respawn is gated with a `Semaphore` (lossless — a permit added before
//!   `spawn` is reached is not lost);
//! * restart backoff runs under the paused virtual clock
//!   (`#[tokio::test(start_paused = true)]`), so the backoff never consumes real
//!   time;
//! * replay calls are observed over an unbounded channel "tap" on the
//!   replacement provider, drained event-by-event (the only timeouts are
//!   virtual-time failsafes that make a broken impl fail loudly instead of
//!   hanging).

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::{mpsc, Notify, Semaphore};

use super::{ResilientBackend, ResilientProvider, TracingNotifier};
use crate::protocol::*;
use crate::traits::{ProviderFuture, TypeProvider};

/// A recorded provider call.
#[derive(Debug, Clone, PartialEq)]
enum MockCall {
    OpenFile {
        path: String,
        content: String,
    },
    LoadFile {
        path: String,
        content: String,
    },
    UpdateFile {
        path: String,
        content: String,
    },
    CloseFile {
        path: String,
    },
    ConfigurePaths {
        base_url: String,
        paths: serde_json::Value,
    },
    UpdateWorkspaceFolders {
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    },
    RegisterCarrierMember {
        companion_path: String,
        content: String,
        project_file_name: String,
    },
}

fn call_path(call: &MockCall) -> &str {
    match call {
        MockCall::OpenFile { path, .. }
        | MockCall::LoadFile { path, .. }
        | MockCall::UpdateFile { path, .. }
        | MockCall::CloseFile { path } => path,
        MockCall::ConfigurePaths { base_url, .. } => base_url,
        MockCall::UpdateWorkspaceFolders { .. } => "",
        MockCall::RegisterCarrierMember { companion_path, .. } => companion_path,
    }
}

struct MockInner {
    id: &'static str,
    calls: parking_lot::Mutex<Vec<MockCall>>,
    /// Optional event tap: every recorded call is also forwarded here so a test
    /// can await replay deterministically (no polling).
    tap: parking_lot::Mutex<Option<mpsc::UnboundedSender<MockCall>>>,
}

/// A recording `TypeProvider` mock. Cloning shares the recorded state (so the
/// backend can hand the same logical provider back on respawn).
#[derive(Clone)]
struct MockProvider {
    inner: Arc<MockInner>,
}

impl MockProvider {
    fn new(id: &'static str) -> Self {
        Self {
            inner: Arc::new(MockInner {
                id,
                calls: parking_lot::Mutex::new(Vec::new()),
                tap: parking_lot::Mutex::new(None),
            }),
        }
    }

    /// Install an event tap and return its receiver. Calls recorded after this
    /// are also delivered over the returned channel.
    fn attach_tap(&self) -> mpsc::UnboundedReceiver<MockCall> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.inner.tap.lock() = Some(tx);
        rx
    }

    fn calls(&self) -> Vec<MockCall> {
        self.inner.calls.lock().clone()
    }

    /// Record a call. Synchronous — no guard is ever held across an `.await`.
    fn record(&self, call: MockCall) {
        self.inner.calls.lock().push(call.clone());
        if let Some(tap) = self.inner.tap.lock().as_ref() {
            let _ = tap.send(call);
        }
    }
}

impl TypeProvider for MockProvider {
    fn provider_id(&self) -> &'static str {
        self.inner.id
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.record(MockCall::OpenFile {
            path: path.to_string(),
            content: content.to_string(),
        });
        Box::pin(async { Ok(()) })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.record(MockCall::LoadFile {
            path: path.to_string(),
            content: content.to_string(),
        });
        Box::pin(async { Ok(()) })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.record(MockCall::UpdateFile {
            path: path.to_string(),
            content: content.to_string(),
        });
        Box::pin(async { Ok(()) })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.record(MockCall::CloseFile {
            path: path.to_string(),
        });
        Box::pin(async { Ok(()) })
    }

    fn register_carrier_member(
        &self,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        self.record(MockCall::RegisterCarrierMember {
            companion_path: companion_path.to_string(),
            content: content.to_string(),
            project_file_name: project_file_name.to_string(),
        });
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

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        self.record(MockCall::ConfigurePaths {
            base_url: base_url.to_string(),
            paths,
        });
        Box::pin(async { Ok(()) })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.record(MockCall::UpdateWorkspaceFolders { added, removed });
        Box::pin(async { Ok(()) })
    }
}

/// Backend that respawns a pre-built [`MockProvider`], gated on a semaphore so a
/// test can hold the wrapper in its restarting (inner-down) state.
struct TestBackend {
    replacement: MockProvider,
    spawn_gate: Arc<Semaphore>,
}

impl ResilientBackend<MockProvider> for TestBackend {
    fn log_name(&self) -> &'static str {
        "test-provider"
    }

    fn user_label(&self) -> &'static str {
        "test"
    }

    fn restarting_error(&self) -> &'static str {
        "test provider is restarting"
    }

    fn spawn<'a>(
        &'a self,
        _crash_notify: Arc<Notify>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MockProvider, TypeProviderError>> + Send + 'a>,
    > {
        let provider = self.replacement.clone();
        let gate = Arc::clone(&self.spawn_gate);
        Box::pin(async move {
            let permit = gate
                .acquire()
                .await
                .map_err(|_| TypeProviderError::new("test spawn gate closed"))?;
            permit.forget();
            Ok(provider)
        })
    }
}

fn make_resilient(
    initial: MockProvider,
    replacement: MockProvider,
) -> (
    ResilientProvider<MockProvider, TestBackend>,
    Arc<Notify>,
    Arc<Semaphore>,
) {
    let crash_notify = Arc::new(Notify::new());
    let spawn_gate = Arc::new(Semaphore::new(0));
    let provider = ResilientProvider::new(
        initial,
        Arc::clone(&crash_notify),
        TestBackend {
            replacement,
            spawn_gate: Arc::clone(&spawn_gate),
        },
        Arc::new(TracingNotifier),
        3,
    );
    (provider, crash_notify, spawn_gate)
}

/// Spin until the wrapper reports its inner provider is down (a query returns the
/// backend's restarting error). Deterministic: `yield_now` lets the crash monitor
/// and actor make progress; there is no wall-clock sleep, and the bound is only a
/// failsafe against a monitor that never clears the live cell.
async fn await_down(provider: &ResilientProvider<MockProvider, TestBackend>) {
    for _ in 0..100_000 {
        if provider.get_hover("/probe.vue.tsx", 0).await.is_err() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("inner provider never reported down after a crash");
}

/// Drain every replay call from the tap. The first call uses a generous
/// virtual-time failsafe (a missing replay fails loudly rather than hanging); the
/// remainder are drained until a short virtual-time gap proves replay is
/// quiescent. Under the paused clock neither timeout consumes real time.
async fn drain_replay(rx: &mut mpsc::UnboundedReceiver<MockCall>) -> Vec<MockCall> {
    let mut out = Vec::new();
    if let Ok(Some(first)) =
        tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv()).await
    {
        out.push(first);
    }
    while let Ok(Some(call)) =
        tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
    {
        out.push(call);
    }
    out
}

#[tokio::test(start_paused = true)]
async fn removed_carrier_is_absent_from_restart_replay() {
    // DISCRIMINATION: a revert to snapshot-then-swap (capture the desired-state
    // set at crash time, before the close, and replay from that snapshot) makes
    // this RED — the snapshot still contains the file, so replay re-opens it.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    let carrier = "/project/src/Carrier.vue.tsx";
    let kept = "/project/src/Kept.vue.tsx";
    provider
        .open_file(carrier, "const carrier = 1;")
        .await
        .unwrap();
    provider.open_file(kept, "const kept = 1;").await.unwrap();

    crash_notify.notify_one();
    await_down(&provider).await;

    // Retract the file WHILE restarting — applied to the desired set before the
    // respawn is permitted to proceed.
    provider.close_file(carrier).await.unwrap();

    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    assert!(
        replayed
            .iter()
            .any(|c| matches!(c, MockCall::OpenFile { path, .. } if path == kept)),
        "the still-open file must be replayed, got {replayed:?}"
    );
    assert!(
        !replayed.iter().any(|c| call_path(c) == carrier),
        "a file closed before respawn must NOT be replayed, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn mid_restart_update_replays_current_not_stale_bytes() {
    // DISCRIMINATION: a snapshot-then-swap revert replays the PRE-crash bytes
    // ("const v = 1;") captured before the update, making this RED.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    let file = "/project/src/Edited.vue.tsx";
    provider.open_file(file, "const v = 1;").await.unwrap();

    crash_notify.notify_one();
    await_down(&provider).await;

    provider.update_file(file, "const v = 2;").await.unwrap();

    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    let replayed_content = replayed.iter().find_map(|c| match c {
        MockCall::OpenFile { path, content } if path == file => Some(content.clone()),
        _ => None,
    });
    assert_eq!(
        replayed_content.as_deref(),
        Some("const v = 2;"),
        "replay must carry the post-crash content, never the stale pre-crash bytes, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn restart_replay_equals_desired_membership_set() {
    // DISCRIMINATION: a snapshot-then-swap revert replays {A, B} (the crash-time
    // snapshot) instead of {B, C}, making this RED on both counts (A wrongly
    // present, C wrongly absent).
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    let a = "/project/src/A.vue.tsx";
    let b = "/project/src/B.vue.tsx";
    let c = "/project/src/C.vue.tsx";
    provider.open_file(a, "a").await.unwrap();
    provider.open_file(b, "b").await.unwrap();

    crash_notify.notify_one();
    await_down(&provider).await;

    provider.close_file(a).await.unwrap(); // retract A
    provider.open_file(c, "c").await.unwrap(); // add C while restarting

    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    let opened: HashSet<String> = replayed
        .iter()
        .filter_map(|call| match call {
            MockCall::OpenFile { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    let expected: HashSet<String> = [b.to_string(), c.to_string()].into_iter().collect();
    assert_eq!(
        opened, expected,
        "post-restart advertised set must equal the desired set {{B, C}}, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn mutation_racing_respawn_reaches_fresh_inner() {
    // The deterministic, sleep-free analogue of "a mutation racing the respawn
    // replay reaches the fresh inner": an open issued while the provider is down
    // must survive to reach the freshly respawned provider.
    //
    // DISCRIMINATION: a revert that captures the desired set at crash time (or
    // otherwise drops mid-restart mutations in the TOCTOU window) never replays
    // the racing open, making this RED.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    crash_notify.notify_one();
    await_down(&provider).await;

    let racing = "/project/src/Racing.vue.tsx";
    // Must succeed (cached) even though the inner provider is down.
    provider
        .open_file(racing, "const racing = 1;")
        .await
        .unwrap();

    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    assert!(
        replayed.iter().any(|c| matches!(
            c,
            MockCall::OpenFile { path, content }
                if path == racing && content == "const racing = 1;"
        )),
        "an open issued during respawn must reach the fresh inner, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn carrier_registration_racing_respawn_reaches_fresh_inner() {
    // The deterministic, sleep-free verter_type_runtime analogue of the verter_lsp
    // test `registration_racing_respawn_replay_reaches_fresh_inner`: a carrier
    // registered WHILE the provider is restarting must reach the freshly respawned
    // inner via replay — never lost in the (former) snapshot→swap window.
    //
    // DISCRIMINATION: a snapshot-then-swap revert that snapshots the carrier set
    // at crash time drops a registration that lands after the snapshot, making
    // this RED.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    crash_notify.notify_one();
    await_down(&provider).await;

    let carrier = "/project/src/Racing.vue.tsx";
    provider
        .register_carrier_member(
            carrier,
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();

    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    assert!(
        replayed.iter().any(|c| matches!(
            c,
            MockCall::RegisterCarrierMember { companion_path, content, project_file_name }
                if companion_path == carrier
                    && content == "export default {} as any;\n"
                    && project_file_name == "/project/tsconfig.json"
        )),
        "a carrier registration racing the respawn must reach the fresh inner, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn carrier_registration_survives_respawn_contentlessly() {
    // PRESERVED behavior: a published carrier is re-registered into the fresh
    // inner after a crash, carrying its content + owning project (the contentless
    // register path), so a carrier query right after restart still routes
    // correctly.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    let carrier = "/project/src/App.vue.tsx";
    provider
        .register_carrier_member(
            carrier,
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();

    crash_notify.notify_one();
    await_down(&provider).await;
    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    assert!(
        replayed.iter().any(|c| matches!(
            c,
            MockCall::RegisterCarrierMember { companion_path, content, project_file_name }
                if companion_path == carrier
                    && content == "export default {} as any;\n"
                    && project_file_name == "/project/tsconfig.json"
        )),
        "a published carrier must be re-registered into the fresh inner after restart, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn retracted_carrier_is_absent_from_restart_replay() {
    // PRESERVED behavior (fail-closed across restart): a carrier whose companion
    // is closed before the respawn must NOT be re-registered into the fresh inner.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    let carrier = "/project/src/Gone.vue.tsx";
    let kept = "/project/src/Kept.vue.tsx";
    provider
        .register_carrier_member(
            carrier,
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();
    provider
        .register_carrier_member(
            kept,
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();

    crash_notify.notify_one();
    await_down(&provider).await;

    provider.close_file(carrier).await.unwrap();

    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    assert!(
        replayed.iter().any(|c| matches!(
            c,
            MockCall::RegisterCarrierMember { companion_path, .. } if companion_path == kept
        )),
        "the still-registered carrier must be replayed, got {replayed:?}"
    );
    assert!(
        !replayed.iter().any(|c| matches!(
            c,
            MockCall::RegisterCarrierMember { companion_path, .. } if companion_path == carrier
        )),
        "a carrier retracted before respawn must NOT be re-registered, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn restart_replays_state_without_downgrading_loaded_files() {
    // PRESERVED behavior: load/open mode fidelity, path configs, and workspace
    // folders all survive a respawn.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    let loaded = "/project/src/loaded.vue.tsx";
    let opened = "/project/src/open.vue.tsx";
    provider
        .load_file(loaded, "const loaded = 1;")
        .await
        .unwrap();
    provider
        .update_file(loaded, "const loaded = 2;")
        .await
        .unwrap();
    provider.open_file(opened, "const open = 1;").await.unwrap();
    provider
        .configure_paths("/project/src", serde_json::json!({ "@/*": ["./*"] }))
        .await
        .unwrap();
    provider
        .update_workspace_folders(
            vec![serde_json::json!({ "uri": "file:///project" })],
            vec![],
        )
        .await
        .unwrap();

    crash_notify.notify_one();
    await_down(&provider).await;
    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    assert!(
        replayed
            .iter()
            .any(|c| matches!(c, MockCall::LoadFile { path, content } if path == loaded && content == "const loaded = 2;")),
        "a loaded file replays via load_file with its latest content, got {replayed:?}"
    );
    assert!(
        !replayed
            .iter()
            .any(|c| matches!(c, MockCall::OpenFile { path, .. } if path == loaded)),
        "a loaded file must NOT be downgraded to open on replay, got {replayed:?}"
    );
    assert!(
        replayed
            .iter()
            .any(|c| matches!(c, MockCall::OpenFile { path, .. } if path == opened)),
        "an opened file replays via open_file, got {replayed:?}"
    );
    assert!(
        replayed.iter().any(
            |c| matches!(c, MockCall::ConfigurePaths { base_url, .. } if base_url == "/project/src")
        ),
        "path configuration replays after restart, got {replayed:?}"
    );
    assert!(
        replayed.iter().any(|c| matches!(
            c,
            MockCall::UpdateWorkspaceFolders { added, .. }
                if added.iter().any(|f| f.get("uri").and_then(|v| v.as_str()) == Some("file:///project"))
        )),
        "workspace folders replay after restart, got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn open_forwards_to_the_live_provider() {
    // While the inner provider is live, a mutation reaches it (the actor forwards
    // it) — proving the actor is not a write-only cache.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let (provider, _crash_notify, _spawn_gate) = make_resilient(initial.clone(), replacement);

    provider
        .open_file("/project/src/Live.vue.tsx", "x")
        .await
        .unwrap();

    assert!(
        initial.calls().iter().any(
            |c| matches!(c, MockCall::OpenFile { path, .. } if path == "/project/src/Live.vue.tsx")
        ),
        "an open against a live wrapper must forward to the live provider"
    );
}

#[tokio::test(start_paused = true)]
async fn register_carrier_forwards_to_the_live_provider() {
    // A carrier registered against a live wrapper must forward to the live inner
    // (not be swallowed) — the production bug the carrier path was built to fix.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let (provider, _crash_notify, _spawn_gate) = make_resilient(initial.clone(), replacement);

    provider
        .register_carrier_member(
            "/project/src/App.vue.tsx",
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();

    assert!(
        initial.calls().iter().any(|c| matches!(
            c,
            MockCall::RegisterCarrierMember { companion_path, project_file_name, .. }
                if companion_path == "/project/src/App.vue.tsx"
                    && project_file_name == "/project/tsconfig.json"
        )),
        "register_carrier_member must forward to the live inner provider, calls={:?}",
        initial.calls()
    );
}

#[test]
fn resilient_does_not_reintroduce_snapshot_then_swap() {
    // Static architecture guard: the single-writer actor owns the desired-state
    // set task-locally. The retired snapshot-then-swap design stored it in
    // shared, lock-guarded maps and cloned a crash-time snapshot to replay from
    // (plus a `registration_gate` held across that window); reintroducing any of
    // those is the TOCTOU / backpressure-stall regression this guard blocks.
    let source = include_str!("resilient.rs");

    for forbidden in [
        "file_cache: Arc<RwLock<",
        "path_configs: Arc<RwLock<",
        "workspace_folders: Arc<RwLock<",
        "carrier_registrations: Arc<RwLock<",
        "cache_snapshot",
        "carrier_snapshot",
        "registration_gate",
    ] {
        assert!(
            !source.contains(forbidden),
            "snapshot-then-swap replay pattern reintroduced in resilient.rs: `{forbidden}`"
        );
    }

    for required in ["enum Command", "GoLive", "async fn run_actor"] {
        assert!(
            source.contains(required),
            "single-writer actor structure missing from resilient.rs: `{required}`"
        );
    }
}
