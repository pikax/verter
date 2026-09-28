//! Lifecycle coverage for the single-writer [`ProviderHub`](super::ProviderHub).
//!
//! The mock interleaving tests drive the REAL production path —
//! `ProviderHub::new` spawns the actor, `establish` installs the first engine
//! and arms its crash monitor, a crash is tripped through the crash signal the
//! hub handed the establisher, and the respawn is gated through a real
//! [`ProviderEstablisher`]. Those tests are deterministic and contain no
//! wall-clock sleep:
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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, Notify, Semaphore};

use verter_identity::identity::ProviderEpoch;

use super::desired::{DesiredMutation, Lane};
use super::{
    AppliedReceipt, EstablishFuture, HubPolicy, NotifySeverity, ProviderEstablisher, ProviderHub,
    ProviderNotifier, QueryFingerprint, TracingNotifier,
};
use crate::protocol::*;
use crate::traits::{ProviderFuture, TypeProvider};
use crate::tsserver::TsserverTypeProvider;

// @ai-generated
#[test]
fn project_bound_diagnostics_quarantine_is_scoped_to_the_configured_project() {
    let first = QueryFingerprint::new("diagnostics-in-project", "/ws/App.vue.jsx", 0, 0)
        .in_scope("/ws/tsconfig.app.json");
    let second = QueryFingerprint::new("diagnostics-in-project", "/ws/App.vue.jsx", 0, 0)
        .in_scope("/ws/tsconfig.tests.json");

    assert_ne!(first, second);
}

#[tokio::test]
async fn quarantined_diagnostics_are_unavailable_until_content_changes() {
    let harness = make_harness(MockProvider::new("tsgo"), MockProvider::new("tsgo")).await;
    let provider = &harness.provider;
    let path = "/workspace/App.vue.tsx";
    let project = "/workspace/tsconfig.json";
    assert!(provider.get_diagnostics(path).await.unwrap().is_empty());
    {
        let mut watch = provider.state.shared.query_watch.lock().unwrap();
        for fingerprint in [
            QueryFingerprint::new("diagnostics", path, 0, 0),
            QueryFingerprint::new("diagnostics-in-project", path, 0, 0).in_scope(project),
        ] {
            watch.begin(&fingerprint);
            for _ in 0..super::quarantine::QUARANTINE_STRIKE_THRESHOLD {
                watch.record_crash_implications();
            }
            watch.end(&fingerprint, false);
        }
    }
    let foreground = provider.get_diagnostics(path).await;
    let background = provider.get_diagnostics_background(path).await;
    let configured = provider.get_diagnostics_in_project(path, project).await;
    assert!(
        foreground.is_err() && background.is_err() && configured.is_err(),
        "quarantine cannot attest a successful diagnostic pull: {foreground:?}, {background:?}, {configured:?}"
    );
    assert!(provider
        .get_diagnostics("/workspace/Other.vue.tsx")
        .await
        .is_ok());
    provider
        .update_file(path, "const changed = true")
        .await
        .unwrap();
    assert!(provider.get_diagnostics(path).await.unwrap().is_empty());
    assert!(provider
        .get_diagnostics_background(path)
        .await
        .unwrap()
        .is_empty());
    assert!(provider
        .get_diagnostics_in_project(path, project)
        .await
        .is_ok());
}

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
    Hover {
        path: String,
        offset: u32,
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
        source_path: String,
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
        | MockCall::CloseFile { path }
        | MockCall::Hover { path, .. } => path,
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
    /// When set for a path, `get_hover` on THAT path BLOCKS on the gate and
    /// then fails with a transport-shaped error — simulating a request in
    /// flight against a child that dies mid-request (the killer-request shape
    /// crash quarantine covers). Hovers on other paths stay instant so the
    /// liveness probes (`await_down`/`await_live`) never park on the gate.
    hover_gate: parking_lot::Mutex<Option<(String, Arc<Semaphore>)>>,
    configure_gate: parking_lot::Mutex<Option<Arc<Semaphore>>>,
    /// When set, `update_file` BLOCKS on the gate before recording — an engine
    /// holding a state update beyond its submitter's deadline.
    update_gate: parking_lot::Mutex<Option<Arc<Semaphore>>>,
    shutdowns: AtomicUsize,
    /// When set, a gated hover SUCCEEDS once released (an answer that was in
    /// flight when its engine was retired) instead of failing.
    gated_hover_succeeds: std::sync::atomic::AtomicBool,
    /// When set, `configure_paths` fails (an engine rejecting replay).
    configure_fails: std::sync::atomic::AtomicBool,
    /// The ambient request deadline observed by every `open_file` call.
    open_deadlines: parking_lot::Mutex<Vec<Option<tokio::time::Instant>>>,
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
                hover_gate: parking_lot::Mutex::new(None),
                configure_gate: parking_lot::Mutex::new(None),
                update_gate: parking_lot::Mutex::new(None),
                shutdowns: AtomicUsize::new(0),
                gated_hover_succeeds: std::sync::atomic::AtomicBool::new(false),
                configure_fails: std::sync::atomic::AtomicBool::new(false),
                open_deadlines: parking_lot::Mutex::new(Vec::new()),
            }),
        }
    }

    /// Make every subsequent `get_hover` ON `path` BLOCK on `gate` and then
    /// fail with a transport-shaped error (the in-flight-when-the-child-died
    /// shape). Other paths keep the instant default.
    fn set_blocking_failing_hover(&self, path: &str, gate: Arc<Semaphore>) {
        *self.inner.hover_gate.lock() = Some((path.to_string(), gate));
    }

    /// Restore the instant hover default (lifts a killer gate).
    fn clear_blocking_failing_hover(&self) {
        *self.inner.hover_gate.lock() = None;
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
        record_call(&self.inner, call);
    }
}

/// `MockProvider::record` on the shared handle alone, so a future that owns
/// only the `Arc<MockInner>` (a gated forward) records identically.
fn record_call(inner: &Arc<MockInner>, call: MockCall) {
    inner.calls.lock().push(call.clone());
    if let Some(tap) = inner.tap.lock().as_ref() {
        let _ = tap.send(call);
    }
}

impl TypeProvider for MockProvider {
    fn provider_id(&self) -> &'static str {
        self.inner.id
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.inner
            .open_deadlines
            .lock()
            .push(crate::deadline::current());
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
        let inner = Arc::clone(&self.inner);
        let gate = inner.update_gate.lock().clone();
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
            }
            record_call(&inner, MockCall::UpdateFile { path, content });
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.record(MockCall::CloseFile {
            path: path.to_string(),
        });
        Box::pin(async { Ok(()) })
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        self.record(MockCall::RegisterCarrierMember {
            source_path: source_path.to_string(),
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

    fn get_completion_details<'a>(
        &'a self,
        _path: &'a str,
        _offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        // Enrichment observable from the outside: only the ENGINE that receives
        // the request can attach this documentation.
        let enriched = items
            .iter()
            .map(|item| {
                let mut enriched = item.clone();
                enriched.documentation = Some("engine-attached documentation".to_string());
                enriched
            })
            .collect();
        Box::pin(async move { Ok(enriched) })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        self.record(MockCall::Hover {
            path: path.to_string(),
            offset,
        });
        let gate = match &*self.inner.hover_gate.lock() {
            Some((gated_path, gate)) if gated_path == path => Some(Arc::clone(gate)),
            _ => None,
        };
        let succeeds = self.inner.gated_hover_succeeds.load(Ordering::SeqCst);
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
                if !succeeds {
                    return Err(TypeProviderError::new("connection closed"));
                }
            }
            Ok(None)
        })
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
        let gate = self.inner.configure_gate.lock().clone();
        let fails = self.inner.configure_fails.load(Ordering::SeqCst);
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
            }
            if fails {
                return Err(TypeProviderError::new(
                    "engine rejected the path configuration",
                ));
            }
            Ok(())
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        self.inner.shutdowns.fetch_add(1, Ordering::SeqCst);
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

/// Establisher whose FIRST establishment hands back `initial` immediately and
/// every later one respawns `replacement`, gated on a semaphore so a test can
/// hold the hub in its restarting (no-engine) state.
struct TestBackend {
    initial: parking_lot::Mutex<Option<MockProvider>>,
    replacement: MockProvider,
    spawn_gate: Arc<Semaphore>,
    /// The crash signal the hub handed the FIRST establishment.
    initial_crash_notify: Arc<parking_lot::Mutex<Option<Arc<Notify>>>>,
    /// The crash signal minted for the MOST RECENT respawn, so a test can
    /// crash the respawned generation too (multi-cycle crash scenarios).
    respawned_crash_notify: Arc<parking_lot::Mutex<Option<Arc<Notify>>>>,
}

impl ProviderEstablisher<MockProvider> for TestBackend {
    fn log_name(&self) -> &'static str {
        "test-provider"
    }

    fn user_label(&self) -> &'static str {
        "test"
    }

    fn restarting_error(&self) -> &'static str {
        "test provider is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        false
    }

    fn establish<'a>(&'a self, crash_notify: Arc<Notify>) -> EstablishFuture<'a, MockProvider> {
        if let Some(initial) = self.initial.lock().take() {
            *self.initial_crash_notify.lock() = Some(crash_notify);
            return Box::pin(async move { Ok(Arc::new(initial)) });
        }
        let provider = self.replacement.clone();
        let gate = Arc::clone(&self.spawn_gate);
        let respawned = Arc::clone(&self.respawned_crash_notify);
        Box::pin(async move {
            let permit = gate
                .acquire()
                .await
                .map_err(|_| TypeProviderError::new("test spawn gate closed"))?;
            permit.forget();
            *respawned.lock() = Some(crash_notify);
            Ok(Arc::new(provider))
        })
    }
}

/// Build a hub through the production path and establish its first engine.
async fn establish_hub<P, E>(establisher: E, notifier: Arc<dyn ProviderNotifier>) -> ProviderHub<P>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
    E: ProviderEstablisher<P>,
{
    let hub = ProviderHub::new(establisher, notifier, HubPolicy::explicit(3));
    hub.establish()
        .await
        .expect("the first establishment must install the initial engine");
    hub
}

async fn make_resilient(
    initial: MockProvider,
    replacement: MockProvider,
) -> (ProviderHub<MockProvider>, Arc<Notify>, Arc<Semaphore>) {
    let (provider, crash_notify, spawn_gate, _notifier) =
        make_resilient_with_notifier(initial, replacement).await;
    (provider, crash_notify, spawn_gate)
}

/// Every engine child the hub establishes is announced through the same
/// structural channel — the first one and every respawn.
///
/// The editor tracks the provider child by pid; so does every benchmark
/// harness. Announcing only the initial start leaves both pointing at a dead
/// process after the first respawn, and — worse for measurement — makes a run
/// that tore its engine down and rebuilt it mid-flight indistinguishable from
/// one that did not. Every latency number in such a run silently averages over
/// a cold restart. A prose `show_message` is not that channel: it is
/// user-facing text, not data a receipt can count.
#[tokio::test]
async fn a_respawned_provider_is_announced_structurally() {
    let harness = make_harness(MockProvider::new("tsgo"), MockProvider::new("tsgo")).await;

    harness
        .provider
        .open_file("/p/App.vue.tsx", "const a = 1;")
        .await
        .unwrap();
    assert_eq!(
        harness.notifier.started().len(),
        1,
        "the hub announces the child it established itself — exactly once"
    );

    harness.crash_current_generation();
    harness.spawn_gate.add_permits(1);
    await_down(&harness.provider).await;

    // The respawn is gated on a backoff sleep; the fresh generation's
    // announcement is the observed completion event.
    harness.notifier.await_started(2).await;

    assert_eq!(
        harness.notifier.started().len(),
        2,
        "a post-crash respawn must announce its fresh child structurally, got \
         messages={:?}",
        harness.notifier.messages()
    );
}

/// A [`ProviderNotifier`] that records every user-facing notification so tests
/// can assert what the user was (and was NOT) told.
#[derive(Default)]
struct RecordingNotifier {
    messages: parking_lot::Mutex<Vec<(NotifySeverity, String)>>,
    started: parking_lot::Mutex<Vec<Option<u32>>>,
    /// Signalled on every structural start announcement — event-driven
    /// synchronization for tests awaiting a respawn.
    started_signal: Notify,
}

impl RecordingNotifier {
    fn messages(&self) -> Vec<(NotifySeverity, String)> {
        self.messages.lock().clone()
    }

    fn started(&self) -> Vec<Option<u32>> {
        self.started.lock().clone()
    }

    /// Wait, driven by the announcement event itself, until `count` engines
    /// were structurally announced. The bound is a failsafe that makes a
    /// missing announcement fail loudly instead of hanging.
    async fn await_started(&self, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if self.started().len() >= count {
                    return;
                }
                self.started_signal.notified().await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "only {} engine(s) were structurally announced within 30s: {:?}",
                self.started().len(),
                self.messages()
            )
        });
    }
}

impl ProviderNotifier for RecordingNotifier {
    fn notify(&self, severity: NotifySeverity, message: String) {
        self.messages.lock().push((severity, message));
    }

    fn provider_started(&self, pid: Option<u32>) {
        self.started.lock().push(pid);
        self.started_signal.notify_one();
    }
}

struct ResilientHarness {
    provider: Arc<ProviderHub<MockProvider>>,
    crash_notify: Arc<Notify>,
    spawn_gate: Arc<Semaphore>,
    notifier: Arc<RecordingNotifier>,
    /// The crash-notify handle of the most recently respawned generation.
    respawned_crash_notify: Arc<parking_lot::Mutex<Option<Arc<Notify>>>>,
}

impl ResilientHarness {
    /// Fire the crash signal of the CURRENT (most recently respawned)
    /// generation, falling back to the initial generation's handle.
    fn crash_current_generation(&self) {
        match self.respawned_crash_notify.lock().as_ref() {
            Some(notify) => notify.notify_one(),
            None => self.crash_notify.notify_one(),
        }
    }
}

async fn make_resilient_with_notifier(
    initial: MockProvider,
    replacement: MockProvider,
) -> (
    ProviderHub<MockProvider>,
    Arc<Notify>,
    Arc<Semaphore>,
    Arc<RecordingNotifier>,
) {
    let harness = make_harness(initial, replacement).await;
    let provider = Arc::try_unwrap(harness.provider)
        .unwrap_or_else(|_| panic!("the harness holds the only hub handle"));
    (
        provider,
        harness.crash_notify,
        harness.spawn_gate,
        harness.notifier,
    )
}

async fn make_harness(initial: MockProvider, replacement: MockProvider) -> ResilientHarness {
    let spawn_gate = Arc::new(Semaphore::new(0));
    let notifier = Arc::new(RecordingNotifier::default());
    let initial_crash_notify = Arc::new(parking_lot::Mutex::new(None));
    let respawned_crash_notify = Arc::new(parking_lot::Mutex::new(None));
    let provider = Arc::new(
        establish_hub(
            TestBackend {
                initial: parking_lot::Mutex::new(Some(initial)),
                replacement,
                spawn_gate: Arc::clone(&spawn_gate),
                initial_crash_notify: Arc::clone(&initial_crash_notify),
                respawned_crash_notify: Arc::clone(&respawned_crash_notify),
            },
            Arc::clone(&notifier) as Arc<dyn ProviderNotifier>,
        )
        .await,
    );
    let crash_notify = initial_crash_notify
        .lock()
        .clone()
        .expect("the first establishment received its crash signal");
    ResilientHarness {
        provider,
        crash_notify,
        spawn_gate,
        notifier,
        respawned_crash_notify,
    }
}

/// Spin until the hub reports no serving engine (a query returns the
/// establisher's restarting error). Deterministic: `yield_now` lets the crash
/// monitor and actor make progress; there is no wall-clock sleep, and the bound
/// is only a failsafe against a monitor that never retires the engine.
async fn await_down(provider: &ProviderHub<MockProvider>) {
    for _ in 0..100_000 {
        if provider.get_hover("/probe.vue.tsx", 0).await.is_err() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("inner provider never reported down after a crash");
}

pub(crate) struct RecoveryCarrierFixture {
    pub(crate) source_path: &'static str,
    pub(crate) companion_path: &'static str,
    pub(crate) content: &'static str,
    /// Deliberately disagrees with `content`: a typed success must come from the
    /// plugin store, never an accidental ordinary disk/open-file fallback.
    pub(crate) stale_disk_content: &'static str,
    pub(crate) hover_offset: u32,
    pub(crate) expected_hover: &'static str,
}

pub(crate) const RECOVERY_CARRIERS: [RecoveryCarrierFixture; 2] = [
    RecoveryCarrierFixture {
        source_path: "/project/src/Recovery.vue",
        companion_path: "/project/src/Recovery.vue.tsx",
        content: "export const vueRecoveryValue: string = 'vue';\nvueRecoveryValue;\n",
        stale_disk_content: "export const vueRecoveryValue = null;\nvueRecoveryValue;\n",
        hover_offset: 13,
        expected_hover: "const vueRecoveryValue: string",
    },
    RecoveryCarrierFixture {
        source_path: "/project/src/Recovery.svelte",
        companion_path: "/project/src/Recovery.svelte.tsx",
        content: "export const svelteRecoveryValue: number = 42;\nsvelteRecoveryValue;\n",
        stale_disk_content: "export const svelteRecoveryValue = null;\nsvelteRecoveryValue;\n",
        hover_offset: 13,
        expected_hover: "const svelteRecoveryValue: number",
    },
];

async fn register_recovery_carriers<P: TypeProvider>(provider: &P) {
    for fixture in &RECOVERY_CARRIERS {
        provider
            .register_carrier_member(
                fixture.source_path,
                fixture.companion_path,
                fixture.content,
                "/project/tsconfig.json",
            )
            .await
            .expect("recovery carrier registration must succeed");
    }
}

struct MaterializedRecoveryCarrier {
    source_path: String,
    companion_path: String,
    content: &'static str,
    hover_offset: u32,
    expected_hover: &'static str,
}

struct RealTsserverBackend {
    node_path: String,
    tsserver_path: String,
    workspace_root: String,
    plugin_path: String,
    carrier_store_dir: String,
    failures_before_success: Arc<AtomicUsize>,
    spawn_attempts: Arc<AtomicUsize>,
    /// The first establishment is the session start, not a respawn: it is
    /// neither counted nor failed.
    initial_established: std::sync::atomic::AtomicBool,
    /// The crash signal of the most recent establishment.
    crash_notify: Arc<parking_lot::Mutex<Option<Arc<Notify>>>>,
}

impl ProviderEstablisher<TsserverTypeProvider> for RealTsserverBackend {
    fn log_name(&self) -> &'static str {
        "real-tsserver"
    }

    fn user_label(&self) -> &'static str {
        "tsserver"
    }

    fn restarting_error(&self) -> &'static str {
        "real tsserver is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(
        &'a self,
        crash_notify: Arc<Notify>,
    ) -> EstablishFuture<'a, TsserverTypeProvider> {
        let respawn = self.initial_established.swap(true, Ordering::SeqCst);
        *self.crash_notify.lock() = Some(Arc::clone(&crash_notify));
        if respawn {
            self.spawn_attempts.fetch_add(1, Ordering::SeqCst);
        }
        let fail = respawn
            && self
                .failures_before_success
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok();
        if fail {
            return Box::pin(async { Err(TypeProviderError::new("spawn failed (test)")) });
        }

        let node_path = self.node_path.clone();
        let tsserver_path = self.tsserver_path.clone();
        let workspace_root = self.workspace_root.clone();
        let plugin_path = self.plugin_path.clone();
        let carrier_store_dir = self.carrier_store_dir.clone();
        Box::pin(async move {
            TsserverTypeProvider::spawn(
                &node_path,
                &tsserver_path,
                &workspace_root,
                Some(&plugin_path),
                Some(&carrier_store_dir),
                false,
                Some(crash_notify),
            )
            .await
            .map(Arc::new)
        })
    }
}

pub(crate) struct RealRecoveryHarness {
    _project: tempfile::TempDir,
    provider: ProviderHub<TsserverTypeProvider>,
    crash_notify: Arc<Notify>,
    spawn_attempts: Arc<AtomicUsize>,
    carriers: Vec<MaterializedRecoveryCarrier>,
    project_file_name: String,
}

pub(crate) fn real_tsserver_path(repo_root: &std::path::Path) -> std::path::PathBuf {
    let root = repo_root.to_string_lossy();
    if let Some(path) = crate::discovery::find_tsserver(None, Some(&root)) {
        return path;
    }

    let pnpm_store = repo_root.join("node_modules/.pnpm");
    let mut candidates = std::fs::read_dir(&pnpm_store)
        .unwrap_or_else(|error| panic!("read {}: {error}", pnpm_store.display()))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("typescript@")
        })
        .map(|entry| entry.path().join("node_modules/typescript/lib/tsserver.js"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .pop()
        .expect("real recovery tests require the workspace tsserver.js")
}

pub(crate) async fn build_real_tsserver_plugin(
    repo_root: &std::path::Path,
    fixture_root: &std::path::Path,
    node_path: &str,
) -> std::path::PathBuf {
    let plugin_probe = fixture_root.join("plugin-probe").join("node_modules");
    let plugin_package = plugin_probe.join("@verter").join("typescript-plugin");
    let plugin_entry = plugin_package.join("dist").join("index.js");
    assert!(
        !plugin_entry.exists(),
        "source-built plugin fixture must start without a dist artifact"
    );
    std::fs::create_dir_all(plugin_entry.parent().expect("plugin dist parent"))
        .expect("create source-built plugin package");
    std::fs::write(
        plugin_package.join("package.json"),
        r#"{"name":"@verter/typescript-plugin","version":"0.0.0-test","type":"commonjs","main":"dist/index.js"}"#,
    )
    .expect("write source-built plugin package.json");

    // Preserve normal Node package resolution for optional workspace dependencies
    // (notably `@verter/svelte-jsx`) from the unique temporary plugin package.
    let dependency_link = plugin_package.join("node_modules");
    let workspace_plugin_modules = repo_root
        .join("packages")
        .join("typescript-plugin")
        .join("node_modules");
    assert!(
        workspace_plugin_modules.is_dir(),
        "real recovery tests require {} (workspace plugin dependencies)",
        workspace_plugin_modules.display()
    );
    let workspace_dependencies = std::fs::canonicalize(&workspace_plugin_modules)
        .expect("canonical workspace plugin dependencies");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&dependency_link)
            .arg(&workspace_dependencies)
            .output()
            .expect("create plugin dependency junction");
        assert!(
            output.status.success(),
            "create plugin dependency junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&workspace_dependencies, &dependency_link)
        .expect("create plugin dependency symlink");

    // The shared builder drives esbuild's JavaScript API; the workspace
    // `esbuild/bin/esbuild` file is the platform native executable and node
    // cannot parse it as JavaScript.
    let plugin_builder = repo_root
        .join("scripts")
        .join("build-test-typescript-plugin.mjs");
    assert!(
        plugin_builder.is_file(),
        "shared test-plugin builder missing at {}",
        plugin_builder.display()
    );
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(node_path)
            .arg(&plugin_builder)
            .arg(&plugin_entry)
            .output(),
    )
    .await
    .expect("source plugin build exceeded 30 seconds")
    .expect("run shared test-plugin builder");
    assert!(
        output.status.success(),
        "build production plugin source: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        plugin_entry.is_file(),
        "source plugin build emitted no entry"
    );
    plugin_probe
}

pub(crate) fn publish_recovery_carrier_store<'a>(
    store_dir: &std::path::Path,
    project_file_name: &str,
    epoch: u64,
    version: u64,
    carriers: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) {
    // Exact `@verter/typescript-plugin` manifest/blob wire contract. Keeping the
    // store inside the fixture TempDir makes the external-process test isolated.
    let blobs_dir = store_dir.join("blobs");
    std::fs::create_dir_all(&blobs_dir).expect("create recovery carrier blob store");
    std::fs::create_dir_all(store_dir.join("maps")).expect("create recovery carrier map store");

    let mut owned_sources = Vec::new();
    let mut ready_files = serde_json::Map::new();
    for (source_path, companion_path, content) in carriers {
        let digest = blake3::hash(content.as_bytes());
        let content_hash = digest.as_bytes()[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let blob_name = format!("blake3-{content_hash}.tsx");
        std::fs::write(blobs_dir.join(&blob_name), content).expect("publish recovery carrier blob");
        owned_sources.push(serde_json::json!({
            "source_uri": source_path,
            "provider_uri": companion_path,
            "role": "CarrierIde",
            "script_kind": "TSX",
        }));
        ready_files.insert(
            companion_path.to_string(),
            serde_json::json!({
                "content_hash": content_hash,
                "version": version,
                "script_kind": "TSX",
                "role": "CarrierIde",
                "map_hash": "00000000000000000000000000000000",
                "blob_rel": format!("blobs/{blob_name}"),
            }),
        );
    }

    let mut projects = serde_json::Map::new();
    projects.insert(
        project_file_name.to_string(),
        serde_json::json!({
            "owned_sources": owned_sources,
            "ready_files": ready_files,
        }),
    );
    let manifest = serde_json::json!({
        "epoch": epoch,
        "host_version": "real-recovery-test",
        "projects": projects,
    });
    std::fs::write(
        store_dir.join("manifest.json"),
        serde_json::to_vec(&manifest).expect("serialize recovery carrier manifest"),
    )
    .expect("publish recovery carrier manifest");
}

#[test]
fn real_recovery_store_uses_production_blake3_wire_identity() {
    let store = tempfile::tempdir().expect("create wire-identity store");
    let content = "export const exactIdentity: string = 'ok';\n";
    publish_recovery_carrier_store(
        store.path(),
        "/w/tsconfig.json",
        7,
        9,
        [("/w/Exact.vue", "/w/Exact.vue.tsx", content)],
    );

    let digest = blake3::hash(content.as_bytes());
    let hash = digest.as_bytes()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let blob_rel = format!("blobs/blake3-{hash}.tsx");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(store.path().join("manifest.json")).expect("read exact manifest"),
    )
    .expect("parse exact manifest");
    let ready = &manifest["projects"]["/w/tsconfig.json"]["ready_files"]["/w/Exact.vue.tsx"];
    assert_eq!(ready["content_hash"], hash);
    assert_eq!(ready["blob_rel"], blob_rel);
    assert_eq!(ready["version"], 9);
    assert_eq!(
        std::fs::read_to_string(store.path().join(blob_rel)).expect("read exact blob"),
        content
    );
}

impl RealRecoveryHarness {
    pub(crate) async fn new(failures_before_success: usize) -> Self {
        let project = tempfile::tempdir().expect("create real recovery project");
        std::fs::write(
            project.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"jsx":"preserve"},"include":["*.ts","*.tsx"]}"#,
        )
        .expect("write real recovery tsconfig");
        std::fs::write(project.path().join("main.ts"), "export {};\n")
            .expect("write configured-project anchor");

        let carriers: Vec<MaterializedRecoveryCarrier> = RECOVERY_CARRIERS
            .iter()
            .map(|fixture| {
                let source_name = std::path::Path::new(fixture.source_path)
                    .file_name()
                    .expect("fixture source file name");
                let companion_name = std::path::Path::new(fixture.companion_path)
                    .file_name()
                    .expect("fixture companion file name");
                let source_path = project.path().join(source_name);
                let companion_path = project.path().join(companion_name);
                std::fs::write(&companion_path, fixture.stale_disk_content)
                    .expect("write stale recovery carrier bytes");
                MaterializedRecoveryCarrier {
                    source_path: source_path.to_string_lossy().replace('\\', "/"),
                    companion_path: companion_path.to_string_lossy().replace('\\', "/"),
                    content: fixture.content,
                    hover_offset: fixture.hover_offset,
                    expected_hover: fixture.expected_hover,
                }
            })
            .collect();

        let workspace_root = project.path().to_string_lossy().replace('\\', "/");
        let project_file_name = project
            .path()
            .join("tsconfig.json")
            .to_string_lossy()
            .replace('\\', "/");
        let carrier_store_dir = project.path().join("carrier-store");
        publish_recovery_carrier_store(
            &carrier_store_dir,
            &project_file_name,
            1,
            1,
            carriers.iter().map(|carrier| {
                (
                    carrier.source_path.as_str(),
                    carrier.companion_path.as_str(),
                    carrier.content,
                )
            }),
        );
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root above crates/");
        let node_path = crate::discovery::find_node()
            .expect("real recovery tests require the workspace Node.js runtime");
        let tsserver_path = real_tsserver_path(repo_root).to_string_lossy().into_owned();
        let plugin_path = build_real_tsserver_plugin(repo_root, project.path(), &node_path)
            .await
            .to_string_lossy()
            .into_owned();
        let carrier_store_dir = carrier_store_dir.to_string_lossy().into_owned();

        let anchor_path = format!("{workspace_root}/main.ts");
        let spawn_attempts = Arc::new(AtomicUsize::new(0));
        let crash_slot = Arc::new(parking_lot::Mutex::new(None));
        let provider = establish_hub(
            RealTsserverBackend {
                node_path,
                tsserver_path,
                workspace_root,
                plugin_path,
                carrier_store_dir,
                failures_before_success: Arc::new(AtomicUsize::new(failures_before_success)),
                spawn_attempts: Arc::clone(&spawn_attempts),
                initial_established: std::sync::atomic::AtomicBool::new(false),
                crash_notify: Arc::clone(&crash_slot),
            },
            Arc::new(TracingNotifier),
        )
        .await;
        let crash_notify = crash_slot
            .lock()
            .clone()
            .expect("the initial real tsserver received its crash signal");
        provider
            .open_file(&anchor_path, "export {};\n")
            .await
            .expect("open configured-project anchor");

        Self {
            _project: project,
            provider,
            crash_notify,
            spawn_attempts,
            carriers,
            project_file_name,
        }
    }

    pub(crate) fn crash_notify(&self) -> Arc<Notify> {
        Arc::clone(&self.crash_notify)
    }

    pub(crate) fn spawn_attempts(&self) -> usize {
        self.spawn_attempts.load(Ordering::SeqCst)
    }

    pub(crate) async fn register_carriers(&self) {
        for carrier in &self.carriers {
            // Production carrier membership is contentless at the tsserver seam:
            // `content` hydrates Rust's position cache, while the plugin remains
            // the engine's sole byte authority. An ordinary `open_file` here
            // would bypass the replay behavior this regression must prove.
            self.provider
                .register_carrier_member(
                    &carrier.source_path,
                    &carrier.companion_path,
                    carrier.content,
                    &self.project_file_name,
                )
                .await
                .expect("register real recovery carrier");
            self.provider
                .activate_carrier_member(
                    &carrier.source_path,
                    &carrier.companion_path,
                    &self.project_file_name,
                    crate::traits::CarrierScriptKind::Tsx,
                )
                .await
                .expect("activate real recovery source identity");
        }
    }

    pub(crate) async fn await_down(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if self
                    .provider
                    .get_hover(
                        &self.carriers[0].companion_path,
                        self.carriers[0].hover_offset,
                    )
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("real provider did not enter restarting state");
    }

    pub(crate) async fn assert_carriers_answer_typed(&self) {
        // The crash monitor's own backoff before the 3rd (successful) spawn
        // attempt is a DETERMINISTIC 1s + 2s + 4s = 7s
        // (`spawn_crash_monitor`'s `(1u64 << (attempt - 1)).min(4)` schedule)
        // — and that 7s elapses BEFORE the real tsserver process even starts
        // spawning. On top of it, a genuine child-process spawn + IPC
        // handshake + carrier replay costs real, load-variable time. The
        // previous hardcoded poll schedule (`[0, 250, 500, 1000, 2000, 4000,
        // 2000]`, ~9.75s total) left almost no slack past that 7s floor, so
        // under any contention (concurrent cargo/test processes competing
        // for fork/exec and CPU) the real spawn routinely missed the
        // remaining ~2.75s and this assertion flaked. The poll itself — a
        // real `get_hover` succeeding — IS the completion signal; only the
        // total deadline needs a load-tolerant margin.
        const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
        const DEADLINE: std::time::Duration = std::time::Duration::from_secs(45);

        for carrier in &self.carriers {
            let mut last;
            let started = tokio::time::Instant::now();
            loop {
                last = self
                    .provider
                    .get_hover(&carrier.companion_path, carrier.hover_offset)
                    .await
                    .unwrap_or_default();
                if let Some(hover) = &last {
                    if hover.contents.contains(carrier.expected_hover)
                        && !hover.contents.contains(": any")
                    {
                        break;
                    }
                }
                if started.elapsed() >= DEADLINE {
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            let hover = last.unwrap_or_else(|| {
                panic!(
                    "real tsserver returned no hover for recovered carrier {}",
                    carrier.source_path
                )
            });
            assert!(
                hover.contents.contains(carrier.expected_hover),
                "real tsserver must derive {} from replayed bytes, got {}",
                carrier.expected_hover,
                hover.contents
            );
            assert!(
                !hover.contents.contains(": any"),
                "recovered typed carrier must not degrade to any: {}",
                hover.contents
            );
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.provider
            .shutdown()
            .await
            .expect("shutdown real recovery provider");
    }
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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

    crash_notify.notify_one();
    await_down(&provider).await;

    let carrier = "/project/src/Racing.vue.tsx";
    provider
        .register_carrier_member(
            "/project/src/Racing.vue",
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
            MockCall::RegisterCarrierMember { source_path, companion_path, content, project_file_name }
                if companion_path == carrier
                    && source_path == "/project/src/Racing.vue"
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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

    let carrier = "/project/src/App.vue.tsx";
    provider
        .register_carrier_member(
            "/project/src/App.vue",
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
            MockCall::RegisterCarrierMember { source_path, companion_path, content, project_file_name }
                if companion_path == carrier
                    && source_path == "/project/src/App.vue"
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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

    let carrier = "/project/src/Gone.vue.tsx";
    let kept = "/project/src/Kept.vue.tsx";
    provider
        .register_carrier_member(
            "/project/src/Gone.vue",
            carrier,
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();
    provider
        .register_carrier_member(
            "/project/src/Kept.vue",
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
async fn crash_interrupts_a_stalled_mutation_and_replays_retained_state() {
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let gate = Arc::new(Semaphore::new(0));
    *initial.inner.configure_gate.lock() = Some(Arc::clone(&gate));
    let mut entered = initial.attach_tap();
    let harness = make_harness(initial.clone(), replacement.clone()).await;
    let provider = Arc::clone(&harness.provider);
    let mutation = tokio::spawn(async move {
        provider
            .configure_paths("/project", serde_json::json!({"@/*": ["src/*"]}))
            .await
    });
    assert!(matches!(
        entered.recv().await,
        Some(MockCall::ConfigurePaths { .. })
    ));
    assert!(
        !mutation.is_finished(),
        "healthy pending work must not be abandoned"
    );

    // This mutation queues behind the stalled forward. Recovery must retain it
    // without forwarding it to the failed provider or reordering the replay.
    let queued = harness
        .provider
        .open_file("/project/Latest.ts", "export const latest = 42;");
    tokio::pin!(queued);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(queued.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;

    harness.crash_current_generation();
    await_down(&harness.provider).await;
    assert_eq!(initial.inner.shutdowns.load(Ordering::SeqCst), 1);
    assert!(mutation.await.unwrap().is_err());
    queued.await.unwrap();
    harness.spawn_gate.add_permits(1);
    await_live(&harness.provider).await;
    assert_eq!(
        gate.available_permits(),
        0,
        "the wedged operation was never released"
    );
    let calls = replacement.calls();
    assert_eq!(
        calls
            .iter()
            .filter(
                |c| matches!(c, MockCall::ConfigurePaths { base_url, .. } if base_url == "/project")
            )
            .count(),
        1
    );
    assert_eq!(calls.iter().filter(|c| matches!(c, MockCall::OpenFile { path, content } if path == "/project/Latest.ts" && content == "export const latest = 42;")).count(), 1);
    assert!(!initial
        .calls()
        .iter()
        .any(|c| matches!(c, MockCall::OpenFile { path, .. } if path == "/project/Latest.ts")));
}

#[tokio::test(start_paused = true)]
async fn restart_replays_updates_as_open_and_retains_background_only_files() {
    // Replay must match the real backends: update_file opens an editor overlay,
    // while discovery-only files remain background loads.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement).await;

    let loaded = "/project/src/loaded.vue.tsx";
    let opened = "/project/src/open.vue.tsx";
    let background = "/project/src/background.vue.tsx";
    provider
        .load_file(background, "const background = 1;")
        .await
        .unwrap();
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
            .any(|c| matches!(c, MockCall::OpenFile { path, content } if path == loaded && content == "const loaded = 2;")),
        "an editor update promotes a loaded file to an open overlay, got {replayed:?}"
    );
    assert!(
        !replayed
            .iter()
            .any(|c| matches!(c, MockCall::OpenFile { path, .. } if path == background)),
        "discovery-only files must not become open overlays, got {replayed:?}"
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
    let (provider, _crash_notify, _spawn_gate) = make_resilient(initial.clone(), replacement).await;

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

/// A completion item with no optional payload — the hub must enrich it, so
/// whatever the engine attaches is observable as a diff against this.
fn bare_completion(label: &str) -> Completion {
    Completion {
        label: label.to_string(),
        kind: None,
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

#[tokio::test(start_paused = true)]
async fn completion_details_forward_to_the_serving_engine() {
    // DISCRIMINATION: the trait's default `get_completion_details` body returns
    // the items unchanged, so a hub that fails to forward them silently drops
    // documentation and enrichment — this stays RED until the hub forwards.
    let initial = MockProvider::new("tsserver");
    let (provider, _crash_notify, _spawn_gate) =
        make_resilient(initial, MockProvider::new("tsserver")).await;

    let enriched = provider
        .get_completion_details("/p/App.vue.tsx", 8, &[bare_completion("alpha")])
        .await
        .expect("completion details must answer");

    assert_eq!(
        enriched
            .iter()
            .map(|item| item.documentation.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("engine-attached documentation")],
        "the hub must forward completion details to the serving engine for enrichment"
    );
}

#[tokio::test(start_paused = true)]
async fn register_carrier_forwards_to_the_live_provider() {
    // A carrier registered against a live wrapper must forward to the live inner
    // (not be swallowed) — the production bug the carrier path was built to fix.
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let (provider, _crash_notify, _spawn_gate) = make_resilient(initial.clone(), replacement).await;

    provider
        .register_carrier_member(
            "/project/src/App.vue",
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

// ── respawn-failure recovery (D2) ─────────────────────────────────────

/// Backend whose `spawn` fails a configurable number of times before succeeding,
/// recording every attempt. Drives the respawn-retry path deterministically —
/// only the crash monitor calls `spawn`, so the load/store counter needs no CAS.
struct FlakyBackend {
    initial: parking_lot::Mutex<Option<MockProvider>>,
    initial_crash_notify: Arc<parking_lot::Mutex<Option<Arc<Notify>>>>,
    replacement: MockProvider,
    failures_before_success: Arc<AtomicUsize>,
    spawn_attempts: Arc<AtomicUsize>,
}

impl ProviderEstablisher<MockProvider> for FlakyBackend {
    fn log_name(&self) -> &'static str {
        "flaky-provider"
    }

    fn user_label(&self) -> &'static str {
        "flaky"
    }

    fn restarting_error(&self) -> &'static str {
        "flaky provider is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        false
    }

    fn establish<'a>(&'a self, crash_notify: Arc<Notify>) -> EstablishFuture<'a, MockProvider> {
        if let Some(initial) = self.initial.lock().take() {
            *self.initial_crash_notify.lock() = Some(crash_notify);
            return Box::pin(async move { Ok(Arc::new(initial)) });
        }
        self.spawn_attempts.fetch_add(1, Ordering::Relaxed);
        let provider = self.replacement.clone();
        let remaining = self.failures_before_success.load(Ordering::Relaxed);
        if remaining > 0 {
            self.failures_before_success
                .store(remaining - 1, Ordering::Relaxed);
            Box::pin(async { Err(TypeProviderError::new("spawn failed (test)")) })
        } else {
            Box::pin(async move { Ok(Arc::new(provider)) })
        }
    }
}

async fn make_flaky(
    initial: MockProvider,
    replacement: MockProvider,
    failures_before_success: usize,
) -> (ProviderHub<MockProvider>, Arc<Notify>, Arc<AtomicUsize>) {
    let spawn_attempts = Arc::new(AtomicUsize::new(0));
    let initial_crash_notify = Arc::new(parking_lot::Mutex::new(None));
    let provider = establish_hub(
        FlakyBackend {
            initial: parking_lot::Mutex::new(Some(initial)),
            initial_crash_notify: Arc::clone(&initial_crash_notify),
            replacement,
            failures_before_success: Arc::new(AtomicUsize::new(failures_before_success)),
            spawn_attempts: Arc::clone(&spawn_attempts),
        },
        Arc::new(TracingNotifier),
    )
    .await;
    let crash_notify = initial_crash_notify
        .lock()
        .clone()
        .expect("the first establishment received its crash signal");
    (provider, crash_notify, spawn_attempts)
}

/// Spin (virtual-clock) until `check` holds. Sleep-based so the paused clock
/// advances through the monitor's backoff sleeps; the bound is a failsafe.
async fn spin_until(provider: &ProviderHub<MockProvider>, up: bool) -> bool {
    for _ in 0..50_000 {
        let answered = provider.get_hover("/probe.vue.tsx", 0).await.is_ok();
        if answered == up {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

/// D2: a TRANSIENT respawn failure must NOT leave the provider dead for the rest
/// of the session — the crash monitor retries within the same restart budget and
/// the provider recovers, so the next query answers.
#[tokio::test]
async fn failed_respawn_retries_within_budget_and_recovers() {
    let harness = RealRecoveryHarness::new(2).await;
    harness.register_carriers().await;

    harness.crash_notify.notify_one();
    harness.await_down().await;
    harness.assert_carriers_answer_typed().await;
    assert_eq!(
        harness.spawn_attempts(),
        3,
        "two failed respawns + one successful real-tsserver respawn"
    );
    harness.shutdown().await;
}

/// D2 bound: a PERSISTENTLY failing respawn exhausts the shared restart budget
/// and fails closed (verter-only), never a hot unbounded respawn loop.
#[tokio::test(start_paused = true)]
async fn persistently_failing_respawn_exhausts_budget_and_stays_down() {
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let (provider, crash_notify, spawn_attempts) =
        make_flaky(initial, replacement.clone(), usize::MAX >> 1).await;

    register_recovery_carriers(&provider).await;

    crash_notify.notify_one();
    assert!(
        spin_until(&provider, false).await,
        "the live cell must be cleared after a crash"
    );
    // Let the monitor run through every budgeted attempt (backoff under the
    // paused clock), then confirm it gave up: the provider stays down and no
    // further spawn attempts are made.
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let attempts_after_budget = spawn_attempts.load(Ordering::Relaxed);
    assert!(
        provider.get_hover("/probe.vue.tsx", 0).await.is_err(),
        "a persistently failing backend stays down (fails closed) after the budget"
    );
    assert_eq!(
        attempts_after_budget, 3,
        "exactly max_restarts spawn attempts — the give-up is bounded, not a loop"
    );
    // No further attempts accumulate once the budget is exhausted.
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    assert_eq!(
        spawn_attempts.load(Ordering::Relaxed),
        attempts_after_budget,
        "no further respawn attempts after the budget is exhausted"
    );
    for fixture in &RECOVERY_CARRIERS {
        assert!(
            provider
                .get_hover(fixture.companion_path, fixture.hover_offset)
                .await
                .is_err(),
            "a persistently failed respawn must fail closed for {} typed queries",
            fixture.source_path
        );
    }
    assert!(
        !replacement.calls().iter().any(|call| matches!(
            call,
            MockCall::RegisterCarrierMember { companion_path, .. }
                if RECOVERY_CARRIERS
                    .iter()
                    .any(|fixture| fixture.companion_path == companion_path)
        )),
        "a provider that never spawned successfully must receive no carrier replay"
    );
}

/// A query against a hub that exhausted its restart budget reports the
/// TERMINAL state — never an eternal "restarting" that claims a recovery is
/// under way when the recovery has permanently given up.
#[tokio::test(start_paused = true)]
async fn an_exhausted_hub_reports_exhaustion_not_eternal_restarting() {
    let (provider, crash_notify, _spawn_attempts) = make_flaky(
        MockProvider::new("tsserver"),
        MockProvider::new("tsserver"),
        usize::MAX >> 1,
    )
    .await;

    crash_notify.notify_one();
    assert!(
        spin_until(&provider, false).await,
        "the live cell must be cleared after a crash"
    );
    // Run the monitor through its whole (failing) budget under the paused clock.
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    let error = provider
        .get_hover("/probe.vue.tsx", 0)
        .await
        .expect_err("an exhausted hub fails closed");
    assert!(
        error.message.contains("exhausted its restart budget"),
        "an exhausted hub must say it stays down for the session, got: {}",
        error.message
    );
    assert_ne!(
        error.message, "flaky provider is restarting",
        "an exhausted hub must not claim a restart is in progress"
    );
}

// ─── Deliberate-teardown vs crash discrimination + killer-request quarantine ───

/// Spin (cooperatively) until `cond` holds, failing loudly instead of hanging.
async fn await_cond(mut cond: impl FnMut() -> bool, what: &str) {
    for _ in 0..100_000 {
        if cond() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition never held: {what}");
}

/// Spin until the wrapper serves queries again (restart completed). Uses a
/// VIRTUAL-clock sleep (not a busy yield) so the paused test clock advances
/// through the monitor's restart backoff.
async fn await_live(provider: &ProviderHub<MockProvider>) {
    for _ in 0..1_000 {
        if provider.get_hover("/probe-live.vue.tsx", 0).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("wrapper never went live again after respawn was permitted");
}

/// Drive the recurrence that identifies a genuine killer request: the hover
/// `(companion, offset)` is IN FLIGHT at TWO consecutive engine deaths —
/// cycle 1 against `initial`, then (replayed, exactly as an editor retry
/// would) cycle 2 against the respawned `replacement`. Returns with the
/// killer quarantined and the engine live again.
async fn drive_killer_to_quarantine(
    harness: &ResilientHarness,
    provider: &Arc<ProviderHub<MockProvider>>,
    initial: &MockProvider,
    replacement: &MockProvider,
    companion: &'static str,
    offset: u32,
) {
    // Cycle 1: the killer reaches the engine and is in flight when it dies.
    let gate_one = Arc::new(Semaphore::new(0));
    initial.set_blocking_failing_hover(companion, Arc::clone(&gate_one));
    let killer_one = tokio::spawn({
        let provider = Arc::clone(provider);
        async move { provider.get_hover(companion, offset).await }
    });
    await_cond(
        || {
            initial.calls().iter().any(|c| {
                matches!(c, MockCall::Hover { path, offset: o } if path == companion && *o == offset)
            })
        },
        "cycle-1 killer hover reached the initial provider",
    )
    .await;
    harness.crash_notify.notify_one();
    await_down(provider).await;
    gate_one.add_permits(1);
    let killed_one = killer_one.await.unwrap();
    assert!(
        killed_one.is_err(),
        "the cycle-1 in-flight killer surfaces its transport failure, got {killed_one:?}"
    );
    harness.spawn_gate.add_permits(1);
    await_live(provider).await;

    // Replay: the identical request goes back in (an editor/retry layer would
    // do exactly this) and kills the respawned engine too.
    let gate_two = Arc::new(Semaphore::new(0));
    replacement.set_blocking_failing_hover(companion, Arc::clone(&gate_two));
    let killer_two = tokio::spawn({
        let provider = Arc::clone(provider);
        async move { provider.get_hover(companion, offset).await }
    });
    await_cond(
        || {
            replacement.calls().iter().any(|c| {
                matches!(c, MockCall::Hover { path, offset: o } if path == companion && *o == offset)
            })
        },
        "cycle-2 killer hover reached the respawned provider",
    )
    .await;
    harness.crash_current_generation();
    await_down(provider).await;
    gate_two.add_permits(1);
    let killed_two = killer_two.await.unwrap();
    assert!(
        killed_two.is_err(),
        "the cycle-2 in-flight killer surfaces its transport failure, got {killed_two:?}"
    );
    harness.spawn_gate.add_permits(1);
    await_live(provider).await;
    // Lift the gate so any FUTURE (buggy) replay would complete instantly
    // instead of hanging the test.
    replacement.clear_blocking_failing_hover();
}

fn hover_count(provider: &MockProvider, companion: &str, offset: u32) -> usize {
    provider
        .calls()
        .iter()
        .filter(|c| {
            matches!(c, MockCall::Hover { path, offset: o } if path == companion && *o == offset)
        })
        .count()
}

#[tokio::test(start_paused = true)]
async fn deliberate_shutdown_is_not_reported_as_a_crash_and_never_respawns() {
    // DISCRIMINATION: without teardown intent on the wrapper, the torn-down
    // child's exit EOF (surfaced on the SAME crash-notify handle a real death
    // uses) drives the monitor through the crash path — the user sees
    // "crashed. Restarting" on every clean editor shutdown and a zombie engine
    // is respawned into the dying session. This is exactly the spurious
    // notification the Neovim real-client smoke observed at client stop.
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate, notifier) =
        make_resilient_with_notifier(initial, replacement).await;

    provider
        .open_file("/p/App.vue.tsx", "const a = 1;")
        .await
        .unwrap();

    provider.shutdown().await.unwrap();
    // The torn-down child's stdout EOF fires the crash-notify handle.
    crash_notify.notify_one();

    // Give a (wrongly armed) monitor its full notification + backoff horizon,
    // WITH spawn permits available so a respawn attempt would be observable.
    spawn_gate.add_permits(4);
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;

    assert!(
        notifier.messages().is_empty(),
        "a deliberate shutdown must produce NO user-facing crash/restart \
         notification, got {:?}",
        notifier.messages()
    );
    let replayed = drain_replay(&mut replay_rx).await;
    assert!(
        replayed.is_empty(),
        "a deliberate shutdown must never respawn/replay into a fresh engine, \
         got {replayed:?}"
    );
}

/// A shutdown that lands while recovery sleeps out its restart backoff must
/// abandon the respawn BEFORE any process is spawned or the user is told about
/// a restart failure — `shutdown().await` having returned means teardown.
#[tokio::test(start_paused = true)]
async fn shutdown_during_restart_backoff_never_respawns_or_notifies() {
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(initial, replacement.clone()).await;

    harness.crash_notify.notify_one();
    await_down(&harness.provider).await;
    // Let the monitor pass its loop-head teardown check and park inside the
    // 1s backoff sleep (t+0.5s < t+1s, paused clock).
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    harness.provider.shutdown().await.unwrap();

    // Permit any (buggy) respawn attempt and give it its full backoff horizon.
    harness.spawn_gate.add_permits(4);
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;

    let restart_failures = harness
        .notifier
        .messages()
        .iter()
        .filter(|(_, message)| message.contains("Failed to restart"))
        .count();
    assert_eq!(
        restart_failures,
        0,
        "a recovery abandoned by teardown must not report restart failures, got {:?}",
        harness.notifier.messages()
    );
    assert_eq!(
        replacement.inner.shutdowns.load(Ordering::SeqCst),
        0,
        "a recovery abandoned by teardown must never spawn (and tear down) an engine"
    );
    assert!(
        !harness.provider.is_serving(),
        "a hub shut down mid-backoff stays down"
    );
}

#[tokio::test(start_paused = true)]
async fn killer_request_is_quarantined_and_never_replayed_into_restarted_engine() {
    // DISCRIMINATION: without quarantine, the identical (method, path, offset)
    // request keeps going back into every restarted engine; against a real
    // engine whose death that exact request causes, that is the infinite
    // crash-restart loop that burns the restart budget to verter-only mode.
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(initial.clone(), replacement.clone()).await;
    let provider = Arc::clone(&harness.provider);
    let companion = "/p/SvelteJs.svelte.jsx";
    provider
        .open_file(companion, "let title = $state(1);")
        .await
        .unwrap();

    drive_killer_to_quarantine(&harness, &provider, &initial, &replacement, companion, 42).await;

    // The killer fingerprint now fails closed WITHOUT touching the engine.
    let replays_before = hover_count(&replacement, companion, 42);
    let quarantined = provider.get_hover(companion, 42).await;
    assert!(
        matches!(quarantined, Ok(None)),
        "the quarantined killer request must fail closed (empty result), got {quarantined:?}"
    );
    assert_eq!(
        hover_count(&replacement, companion, 42),
        replays_before,
        "the quarantined killer request must NEVER be replayed into the restarted engine"
    );

    // Everything else on the same file is served by the restarted engine.
    let neighbor = provider.get_hover(companion, 43).await;
    assert!(neighbor.is_ok(), "a non-killer request must be served");
    assert!(
        hover_count(&replacement, companion, 43) > 0,
        "a different position on the same file must reach the restarted engine"
    );
}

#[tokio::test(start_paused = true)]
async fn quarantine_clears_when_the_file_content_changes() {
    // A quarantined fingerprint is tied to the content that killed the engine:
    // once the file changes, the same position is a NEW request and must be
    // served again (otherwise a position is blackholed forever).
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(initial.clone(), replacement.clone()).await;
    let provider = Arc::clone(&harness.provider);
    let companion = "/p/SvelteJs.svelte.jsx";
    provider
        .open_file(companion, "let title = $state(1);")
        .await
        .unwrap();

    drive_killer_to_quarantine(&harness, &provider, &initial, &replacement, companion, 42).await;
    let replays_before = hover_count(&replacement, companion, 42);
    assert!(
        matches!(provider.get_hover(companion, 42).await, Ok(None)),
        "killer stays quarantined while the content is unchanged"
    );
    assert_eq!(
        hover_count(&replacement, companion, 42),
        replays_before,
        "quarantined killer must not reach the engine before the content changes"
    );

    provider
        .update_file(companion, "let title = $state(2);")
        .await
        .unwrap();

    let served = provider.get_hover(companion, 42).await;
    assert!(
        served.is_ok(),
        "after a content change the request is served"
    );
    assert!(
        hover_count(&replacement, companion, 42) > replays_before,
        "after a content change the same position must reach the engine again"
    );
}

/// An answer that the epoch REJECTED (the engine was retired before the result
/// settled) is not a successful completion: it must not erase the crash strikes
/// the fingerprint accumulated — only a strike-free quarantine can self-heal a
/// bystander, and a discarded answer proves nothing about the request.
#[tokio::test(start_paused = true)]
async fn a_discarded_answer_from_a_retired_engine_keeps_its_crash_strikes() {
    let initial = MockProvider::new("tsgo");
    let harness = make_harness(initial.clone(), MockProvider::new("tsgo")).await;
    let provider = Arc::clone(&harness.provider);
    let companion = "/p/Struck.svelte.jsx";
    let offset = 42u32;
    let fp = QueryFingerprint::new("hover", companion, u64::from(offset), 0);

    // The hover is in flight against the first engine and will SUCCEED — but
    // only after that engine has been retired.
    let answer_gate = Arc::new(Semaphore::new(0));
    initial.set_blocking_failing_hover(companion, Arc::clone(&answer_gate));
    initial
        .inner
        .gated_hover_succeeds
        .store(true, Ordering::SeqCst);
    let in_flight = tokio::spawn({
        let provider = Arc::clone(&provider);
        async move { provider.get_hover(companion, offset).await }
    });
    await_cond(
        || hover_count(&initial, companion, offset) == 1,
        "the hover reached the first engine",
    )
    .await;

    harness.crash_notify.notify_one();
    await_down(&provider).await;
    // The crash struck the in-flight fingerprint once.
    assert_eq!(
        provider
            .state
            .shared
            .query_watch
            .lock()
            .unwrap()
            .strike_count(&fp),
        1,
        "the crash must strike the in-flight request"
    );

    answer_gate.add_permits(1);
    let settled = in_flight.await.unwrap();
    assert!(
        settled.is_err(),
        "an answer from a retired engine must not settle as a result, got {settled:?}"
    );
    assert_eq!(
        provider
            .state
            .shared
            .query_watch
            .lock()
            .unwrap()
            .strike_count(&fp),
        1,
        "a discarded answer must not erase the crash strikes of its fingerprint"
    );
}

/// A state update whose submitter deadline elapses before the actor settles the
/// forward still applies (in order) — so the touched paths' crash attribution
/// must still lift. A timeout must not leave stale quarantine behind forever.
#[tokio::test(start_paused = true)]
async fn a_deadline_elapsed_update_still_lifts_the_paths_quarantine() {
    let initial = MockProvider::new("tsserver");
    let engine = initial.clone();
    let (provider, _crash_notify, _spawn_gate) =
        make_resilient(initial, MockProvider::new("tsserver")).await;
    let path = "/workspace/App.vue.tsx";

    // Quarantine the path's diagnostics (two crash implications).
    {
        let mut watch = provider.state.shared.query_watch.lock().unwrap();
        let fingerprint = QueryFingerprint::new("diagnostics", path, 0, 0);
        watch.begin(&fingerprint);
        for _ in 0..super::quarantine::QUARANTINE_STRIKE_THRESHOLD {
            watch.record_crash_implications();
        }
        watch.end(&fingerprint, false);
    }
    assert!(
        provider.get_diagnostics(path).await.is_err(),
        "the seeded quarantine must fail the path closed"
    );

    // The engine holds the update beyond its submitter's deadline.
    let update_gate = Arc::new(Semaphore::new(0));
    *engine.inner.update_gate.lock() = Some(update_gate);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(50);
    let timed_out = crate::deadline::with_deadline_at(
        deadline,
        provider.update_file(path, "const changed = true"),
    )
    .await
    .expect_err("the submitter deadline must elapse while the engine holds the update");
    assert!(
        timed_out.message.contains("deadline elapsed"),
        "got: {}",
        timed_out.message
    );

    // The mutation is recorded (it applies in order) — the quarantine lifted
    // with it, even though the submitter gave up waiting.
    assert!(
        provider.get_diagnostics(path).await.is_ok(),
        "a timed-out update must still lift the touched path's quarantine"
    );
}

#[tokio::test(start_paused = true)]
async fn repeated_killer_request_does_not_burn_the_restart_budget() {
    // ENDURANCE: an editor (or retry layer) that re-issues the killer request
    // in a loop must not drive unbounded crash/restart cycles. The recurrence
    // that identifies the killer costs exactly TWO crash cycles; from then on
    // every replay fails closed against the quarantine while the engine keeps
    // serving everything else — never a third cycle, never verter-only mode.
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(initial.clone(), replacement.clone()).await;
    let provider = Arc::clone(&harness.provider);
    let companion = "/p/SvelteJs.svelte.jsx";
    provider
        .open_file(companion, "let title = $state(1);")
        .await
        .unwrap();

    drive_killer_to_quarantine(&harness, &provider, &initial, &replacement, companion, 42).await;

    // Leave the gate generously provisioned: a buggy re-crash loop would
    // consume additional permits and mint additional notifications.
    harness.spawn_gate.add_permits(8);
    let replays_before = hover_count(&replacement, companion, 42);
    for _ in 0..25 {
        let replayed = provider.get_hover(companion, 42).await;
        assert!(
            matches!(replayed, Ok(None)),
            "every replayed killer fails closed, got {replayed:?}"
        );
    }
    assert_eq!(
        hover_count(&replacement, companion, 42),
        replays_before,
        "no replayed killer may reach the restarted engine"
    );
    let crash_notices = harness
        .notifier
        .messages()
        .iter()
        .filter(|(_, message)| message.contains("crashed. Restarting"))
        .count();
    assert_eq!(
        crash_notices,
        2,
        "exactly TWO crash notifications (the identification cost) — replayed \
         killers must not mint further crash/restart cycles, got {:?}",
        harness.notifier.messages()
    );
    // The engine is still live for everything else — never verter-only mode.
    assert!(
        provider.get_hover(companion, 43).await.is_ok(),
        "the restarted engine keeps serving non-quarantined requests"
    );
}

// ─── Hub lifecycle contract: epochs, deadlines, isolation, singleflight ───

/// Establisher that hands out a scripted engine per establishment, optionally
/// gated, parked forever, or failing, and records every attempt.
struct ScriptedBackend {
    engines: parking_lot::Mutex<std::collections::VecDeque<MockProvider>>,
    attempts: Arc<AtomicUsize>,
    gate: Option<Arc<Semaphore>>,
    park_forever: bool,
}

impl ScriptedBackend {
    fn new(engines: Vec<MockProvider>, attempts: &Arc<AtomicUsize>) -> Self {
        Self {
            engines: parking_lot::Mutex::new(engines.into()),
            attempts: Arc::clone(attempts),
            gate: None,
            park_forever: false,
        }
    }
}

impl ProviderEstablisher<MockProvider> for ScriptedBackend {
    fn log_name(&self) -> &'static str {
        "scripted-provider"
    }

    fn user_label(&self) -> &'static str {
        "scripted"
    }

    fn restarting_error(&self) -> &'static str {
        "scripted provider is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        false
    }

    fn establish<'a>(&'a self, _crash_notify: Arc<Notify>) -> EstablishFuture<'a, MockProvider> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let engine = self.engines.lock().pop_front();
        let gate = self.gate.clone();
        let park_forever = self.park_forever;
        Box::pin(async move {
            if park_forever {
                std::future::pending::<()>().await;
            }
            if let Some(gate) = gate {
                gate.acquire()
                    .await
                    .map_err(|_| TypeProviderError::new("gate closed"))?
                    .forget();
            }
            engine
                .map(Arc::new)
                .ok_or_else(|| TypeProviderError::new("engine unavailable (test)"))
        })
    }
}

fn on_demand_hub(backend: ScriptedBackend) -> ProviderHub<MockProvider> {
    ProviderHub::new(
        backend,
        Arc::new(TracingNotifier),
        HubPolicy::on_demand(3, std::time::Duration::from_secs(10)),
    )
}

/// The ordered editor inputs both sides of the fresh-vs-recovered comparison
/// receive.
async fn apply_ordered_inputs(hub: &ProviderHub<MockProvider>) {
    hub.update_workspace_folders(vec![serde_json::json!({ "uri": "file:///w" })], vec![])
        .await
        .unwrap();
    hub.open_file("/w/b.vue.tsx", "const b = 1;").await.unwrap();
    hub.load_file_background("/w/lib.ts", "export const lib = 1;")
        .await
        .unwrap();
    hub.open_file("/w/a.vue.tsx", "const a = 1;").await.unwrap();
    hub.update_file("/w/b.vue.tsx", "const b = 2;")
        .await
        .unwrap();
    hub.configure_paths("/w", serde_json::json!({ "@/*": ["src/*"] }))
        .await
        .unwrap();
    hub.register_carrier_member(
        "/w/Card.vue",
        "/w/Card.vue.tsx",
        "export default {} as any;\n",
        "/w/tsconfig.json",
    )
    .await
    .unwrap();
    hub.open_file("/w/gone.ts", "export {};").await.unwrap();
    hub.close_file("/w/gone.ts").await.unwrap();
}

/// The editor state an engine holds after receiving `calls`, including the
/// order its live files first became live in.
#[derive(Debug, Default, PartialEq)]
struct HeldState {
    live_order: Vec<String>,
    files: std::collections::BTreeMap<String, (bool, String)>,
    carriers: std::collections::BTreeMap<String, (String, String, String)>,
    paths: std::collections::BTreeMap<String, serde_json::Value>,
    folders: Vec<serde_json::Value>,
}

fn held_state(calls: &[MockCall]) -> HeldState {
    let mut held = HeldState::default();
    for call in calls {
        match call {
            MockCall::OpenFile { path, content } | MockCall::UpdateFile { path, content } => {
                if !held.live_order.contains(path) {
                    held.live_order.push(path.clone());
                }
                held.files.insert(path.clone(), (true, content.clone()));
            }
            MockCall::LoadFile { path, content } => {
                if !held.live_order.contains(path) {
                    held.live_order.push(path.clone());
                }
                if !held.files.get(path).is_some_and(|(open, _)| *open) {
                    held.files.insert(path.clone(), (false, content.clone()));
                }
            }
            MockCall::CloseFile { path } => {
                held.live_order.retain(|live| live != path);
                held.files.remove(path);
                held.carriers.remove(path);
            }
            MockCall::ConfigurePaths { base_url, paths } => {
                held.paths.insert(base_url.clone(), paths.clone());
            }
            MockCall::UpdateWorkspaceFolders { added, removed } => {
                for folder in removed.iter().chain(added) {
                    held.folders
                        .retain(|existing| existing.get("uri") != folder.get("uri"));
                }
                held.folders.extend(added.iter().cloned());
            }
            MockCall::RegisterCarrierMember {
                source_path,
                companion_path,
                content,
                project_file_name,
            } => {
                held.carriers.insert(
                    companion_path.clone(),
                    (
                        source_path.clone(),
                        content.clone(),
                        project_file_name.clone(),
                    ),
                );
            }
            MockCall::Hover { .. } => {}
        }
    }
    held
}

#[tokio::test(start_paused = true)]
async fn background_load_never_replaces_an_unsaved_overlay_live_or_replayed() {
    let initial = MockProvider::new("tsserver");
    let replacement = MockProvider::new("tsserver");
    let mut replay_rx = replacement.attach_tap();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial.clone(), replacement).await;
    let overlay = "/project/src/Edited.vue.tsx";

    provider
        .open_file(overlay, "const unsaved = 2;")
        .await
        .unwrap();
    // Workspace discovery reads the SAVED bytes from disk afterwards.
    provider
        .load_file_background(overlay, "const saved = 1;")
        .await
        .unwrap();
    assert!(
        !initial
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::LoadFile { path, .. } if path == overlay)),
        "a discovery load must not reach an engine that holds the open overlay, got {:?}",
        initial.calls()
    );

    crash_notify.notify_one();
    await_down(&provider).await;
    spawn_gate.add_permits(1);
    let replayed = drain_replay(&mut replay_rx).await;

    let restored: Vec<&str> = replayed
        .iter()
        .filter_map(|c| match c {
            MockCall::OpenFile { path, content } | MockCall::LoadFile { path, content }
                if path == overlay =>
            {
                Some(content.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        restored,
        vec!["const unsaved = 2;"],
        "replay must restore the unsaved overlay exactly once, never the disk bytes, \
         got {replayed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_retired_engine_answer_never_settles_for_its_replacement() {
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(initial.clone(), replacement).await;
    let provider = Arc::clone(&harness.provider);
    let first_epoch = provider.serving_epoch().expect("the first engine serves");

    let answer_gate = Arc::new(Semaphore::new(0));
    initial.set_blocking_failing_hover("/p/Slow.vue.tsx", Arc::clone(&answer_gate));
    initial
        .inner
        .gated_hover_succeeds
        .store(true, Ordering::SeqCst);
    let in_flight = tokio::spawn({
        let provider = Arc::clone(&provider);
        async move { provider.get_hover("/p/Slow.vue.tsx", 7).await }
    });
    await_cond(
        || hover_count(&initial, "/p/Slow.vue.tsx", 7) == 1,
        "the hover reached the first engine",
    )
    .await;

    harness.crash_notify.notify_one();
    await_down(&provider).await;
    // The retired engine now produces a well-formed answer.
    answer_gate.add_permits(1);
    let settled = in_flight.await.unwrap();
    assert!(
        settled.is_err(),
        "an answer from a retired engine must not settle as a result, got {settled:?}"
    );

    harness.spawn_gate.add_permits(1);
    await_live(&provider).await;
    let second_epoch = provider.serving_epoch().expect("the replacement serves");
    assert!(
        second_epoch > first_epoch,
        "recovery installs a strictly newer epoch: {first_epoch:?} -> {second_epoch:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn receipts_name_the_epoch_that_applied_the_mutation() {
    let harness = make_harness(MockProvider::new("tsgo"), MockProvider::new("tsgo")).await;
    let provider = &harness.provider;
    let open = |content: &str| DesiredMutation::Open {
        path: "/p/App.vue.tsx".to_string(),
        content: content.to_string(),
    };

    let first = provider
        .submit_mutation(open("const a = 1;"), Lane::Foreground)
        .await
        .unwrap();
    assert_eq!(
        first,
        AppliedReceipt {
            epoch: Some(ProviderEpoch(1))
        }
    );

    harness.crash_notify.notify_one();
    await_down(provider).await;
    let held = provider
        .submit_mutation(open("const a = 2;"), Lane::Foreground)
        .await
        .unwrap();
    assert_eq!(
        held,
        AppliedReceipt { epoch: None },
        "no engine applied a mutation issued while none serves"
    );

    harness.spawn_gate.add_permits(1);
    await_live(provider).await;
    let second = provider
        .submit_mutation(open("const a = 3;"), Lane::Foreground)
        .await
        .unwrap();
    assert_eq!(
        second,
        AppliedReceipt {
            epoch: Some(ProviderEpoch(2))
        }
    );
}

#[tokio::test(start_paused = true)]
async fn a_crash_report_for_a_retired_epoch_is_inert() {
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(MockProvider::new("tsgo"), replacement.clone()).await;
    let provider = &harness.provider;

    harness.crash_notify.notify_one();
    await_down(provider).await;
    harness.spawn_gate.add_permits(4);
    await_live(provider).await;
    assert_eq!(provider.serving_epoch(), Some(ProviderEpoch(2)));

    // Late reports naming the retired engine, through both the adapter signal
    // and the hub's own recovery entry.
    harness.crash_notify.notify_one();
    provider.recover(ProviderEpoch(1)).await;
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;

    assert_eq!(provider.serving_epoch(), Some(ProviderEpoch(2)));
    assert_eq!(replacement.inner.shutdowns.load(Ordering::SeqCst), 0);
    let crash_notices = harness
        .notifier
        .messages()
        .iter()
        .filter(|(_, message)| message.contains("crashed. Restarting"))
        .count();
    assert_eq!(crash_notices, 1, "{:?}", harness.notifier.messages());
}

#[tokio::test(start_paused = true)]
async fn a_hung_establishment_is_bounded_and_arms_the_retry_cooldown() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut backend = ScriptedBackend::new(vec![MockProvider::new("tsgo")], &attempts);
    backend.park_forever = true;
    let hub = ProviderHub::new(
        backend,
        Arc::new(TracingNotifier),
        HubPolicy::on_demand(3, std::time::Duration::from_secs(10))
            .with_establish_timeout(std::time::Duration::from_millis(50)),
    );

    let started = tokio::time::Instant::now();
    assert!(hub.get_hover("/w/a.ts", 0).await.is_err());
    assert_eq!(
        started.elapsed(),
        std::time::Duration::from_millis(50),
        "the establishment fails at exactly its own bound"
    );
    assert!(!hub.is_serving());
    assert!(hub.get_hover("/w/a.ts", 0).await.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a failure within the cooldown is returned without another establishment"
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_during_establishment_tears_the_engine_down_and_installs_nothing() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let engine = MockProvider::new("tsgo");
    let gate = Arc::new(Semaphore::new(0));
    let mut backend = ScriptedBackend::new(vec![engine.clone()], &attempts);
    backend.gate = Some(Arc::clone(&gate));
    let hub = Arc::new(on_demand_hub(backend));

    let demand = tokio::spawn({
        let hub = Arc::clone(&hub);
        async move { hub.establish().await }
    });
    await_cond(
        || attempts.load(Ordering::SeqCst) == 1,
        "the establishment started",
    )
    .await;
    hub.shutdown().await.unwrap();
    gate.add_permits(1);

    assert!(demand.await.unwrap().is_err());
    assert!(!hub.is_serving());
    await_cond(
        || engine.inner.shutdowns.load(Ordering::SeqCst) == 1,
        "the engine established into a torn-down hub was shut down",
    )
    .await;
}

#[tokio::test(start_paused = true)]
async fn an_engine_that_rejects_replay_is_torn_down_and_never_serves() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let engine = MockProvider::new("tsgo");
    engine.inner.configure_fails.store(true, Ordering::SeqCst);
    let hub = on_demand_hub(ScriptedBackend::new(vec![engine.clone()], &attempts));

    hub.configure_paths("/w", serde_json::json!({ "@/*": ["src/*"] }))
        .await
        .unwrap();
    let demand = hub.get_hover("/w/a.ts", 0).await;

    assert!(demand.is_err(), "got {demand:?}");
    assert!(!hub.is_serving());
    assert_eq!(engine.inner.shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(
        hover_count(&engine, "/w/a.ts", 0),
        0,
        "an engine that did not accept the desired state never answers"
    );
}

#[tokio::test]
async fn a_forwarded_mutation_runs_under_the_submitters_absolute_deadline() {
    let initial = MockProvider::new("tsgo");
    let (provider, _crash_notify, _spawn_gate) =
        make_resilient(initial.clone(), MockProvider::new("tsgo")).await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);

    crate::deadline::with_deadline_at(deadline, provider.open_file("/p/A.ts", "a"))
        .await
        .unwrap();
    provider.open_file("/p/B.ts", "b").await.unwrap();

    assert_eq!(
        initial.inner.open_deadlines.lock().clone(),
        vec![Some(deadline), None],
        "the engine sees the caller's own deadline across the hub queue, and none \
         for an un-deadlined caller"
    );
}

#[tokio::test(start_paused = true)]
async fn concurrent_demands_establish_once_and_warm_use_adds_no_establishment() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let engine = MockProvider::new("tsgo");
    let gate = Arc::new(Semaphore::new(0));
    let mut backend = ScriptedBackend::new(vec![engine.clone()], &attempts);
    backend.gate = Some(Arc::clone(&gate));
    let hub = Arc::new(on_demand_hub(backend));
    hub.configure_paths("/w", serde_json::json!({}))
        .await
        .unwrap();

    let demands: Vec<_> = (0..8)
        .map(|offset| {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move { hub.get_hover("/w/a.ts", offset).await })
        })
        .collect();
    await_cond(|| attempts.load(Ordering::SeqCst) == 1, "one establishment").await;
    gate.add_permits(1);
    for demand in demands {
        demand.await.unwrap().unwrap();
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(hub.serving_epoch(), Some(ProviderEpoch(1)));

    for offset in 0..20 {
        hub.get_hover("/w/a.ts", offset).await.unwrap();
        hub.update_file("/w/a.ts", "const warm = 1;").await.unwrap();
    }
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "warm use establishes nothing"
    );
    assert_eq!(
        engine
            .calls()
            .iter()
            .filter(|c| matches!(c, MockCall::ConfigurePaths { .. }))
            .count(),
        1,
        "warm use replays nothing"
    );
}

#[tokio::test(start_paused = true)]
async fn recovered_state_equals_a_fresh_execution_of_identical_ordered_inputs() {
    // The first engine executes the ordered inputs live; the replacement
    // receives them only through recovery; a lazily established engine
    // receives them only through its first replay. All three must hold the
    // same state, down to the order files first became live in.
    let initial = MockProvider::new("tsgo");
    let replacement = MockProvider::new("tsgo");
    let harness = make_harness(initial.clone(), replacement.clone()).await;
    apply_ordered_inputs(&harness.provider).await;
    let executed = held_state(&initial.calls());
    harness.crash_notify.notify_one();
    await_down(&harness.provider).await;
    harness.spawn_gate.add_permits(1);
    await_live(&harness.provider).await;

    let attempts = Arc::new(AtomicUsize::new(0));
    let lazy_engine = MockProvider::new("tsgo");
    let lazy = on_demand_hub(ScriptedBackend::new(vec![lazy_engine.clone()], &attempts));
    apply_ordered_inputs(&lazy).await;
    lazy.establish().await.unwrap();

    assert_eq!(executed.live_order.len(), 3, "{executed:?}");
    assert_eq!(held_state(&replacement.calls()), executed);
    assert_eq!(held_state(&lazy_engine.calls()), executed);
}

#[tokio::test]
async fn a_wedged_or_failed_instance_never_blocks_an_independent_healthy_instance() {
    // Instance A: its engine wedges on a forwarded state update.
    let wedged_engine = MockProvider::new("tsgo");
    let wedge = Arc::new(Semaphore::new(0));
    *wedged_engine.inner.configure_gate.lock() = Some(Arc::clone(&wedge));
    let mut entered = wedged_engine.attach_tap();
    let (wedged, _crash, _gate) =
        make_resilient(wedged_engine.clone(), MockProvider::new("tsgo")).await;
    let wedged = Arc::new(wedged);
    let stuck = tokio::spawn({
        let wedged = Arc::clone(&wedged);
        async move { wedged.configure_paths("/a", serde_json::json!({})).await }
    });
    assert!(matches!(
        entered.recv().await,
        Some(MockCall::ConfigurePaths { .. })
    ));

    // Instance C: every establishment fails.
    let failing_attempts = Arc::new(AtomicUsize::new(0));
    let failing = on_demand_hub(ScriptedBackend::new(Vec::new(), &failing_attempts));

    // Instance B: independent and healthy.
    let healthy_engine = MockProvider::new("tsgo");
    let (healthy, _crash, _gate) =
        make_resilient(healthy_engine.clone(), MockProvider::new("tsgo")).await;

    let served = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        assert!(failing.get_hover("/c/x.ts", 0).await.is_err());
        healthy.open_file("/b/App.ts", "const b = 1;").await?;
        healthy.get_hover("/b/App.ts", 3).await
    })
    .await
    .expect("a held or failed instance must not delay an independent instance");
    served.unwrap();
    assert_eq!(healthy.serving_epoch(), Some(ProviderEpoch(1)));
    assert_eq!(hover_count(&healthy_engine, "/b/App.ts", 3), 1);
    assert!(!stuck.is_finished(), "instance A is still wedged");

    // Control still progresses on the wedged instance itself.
    tokio::time::timeout(std::time::Duration::from_secs(5), wedged.shutdown())
        .await
        .expect("shutdown must interrupt a wedged forward")
        .unwrap();
    assert!(stuck.await.unwrap().is_err());
    assert!(!wedged.is_serving());
}
