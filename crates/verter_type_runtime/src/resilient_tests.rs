//! Restart-interleaving coverage for the single-writer
//! [`ResilientProvider`](super::ResilientProvider).
//!
//! The mock interleaving tests drive the REAL production path —
//! `ResilientProvider::new` spawns the actor + crash monitor, a crash is tripped
//! through the public crash-notify handle, and the respawn is gated through a
//! real [`ResilientBackend`]. Those tests are deterministic and contain no
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

use super::{ResilientBackend, ResilientProvider, TracingNotifier};
use crate::protocol::*;
use crate::traits::{ProviderFuture, TypeProvider};
use crate::tsserver::TsserverTypeProvider;

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
}

impl ResilientBackend<TsserverTypeProvider> for RealTsserverBackend {
    fn log_name(&self) -> &'static str {
        "real-tsserver"
    }

    fn user_label(&self) -> &'static str {
        "tsserver"
    }

    fn restarting_error(&self) -> &'static str {
        "real tsserver is restarting"
    }

    fn spawn<'a>(
        &'a self,
        crash_notify: Arc<Notify>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TsserverTypeProvider, TypeProviderError>>
                + Send
                + 'a,
        >,
    > {
        self.spawn_attempts.fetch_add(1, Ordering::SeqCst);
        let fail = self
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
        })
    }
}

pub(crate) struct RealRecoveryHarness {
    _project: tempfile::TempDir,
    provider: ResilientProvider<TsserverTypeProvider, RealTsserverBackend>,
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
    let workspace_dependencies = std::fs::canonicalize(
        repo_root
            .join("packages")
            .join("typescript-plugin")
            .join("node_modules"),
    )
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

    let esbuild = repo_root.join("node_modules/esbuild/bin/esbuild");
    let plugin_source = repo_root.join("packages/typescript-plugin/src/index.ts");
    let language_shared_source = repo_root.join("packages/language-shared/src/index.ts");
    let alias = format!(
        "--alias:@verter/language-shared={}",
        language_shared_source.to_string_lossy().replace('\\', "/")
    );
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(node_path)
            .arg(esbuild)
            .arg(plugin_source)
            .args([
                "--bundle",
                "--platform=node",
                "--format=cjs",
                "--target=node18",
            ])
            .arg(alias)
            .arg(format!("--outfile={}", plugin_entry.to_string_lossy()))
            .output(),
    )
    .await
    .expect("source plugin build exceeded 30 seconds")
    .expect("run workspace esbuild");
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

pub(crate) fn publish_unready_recovery_carrier_store<'a>(
    store_dir: &std::path::Path,
    project_file_name: &str,
    epoch: u64,
    carriers: impl IntoIterator<Item = (&'a str, &'a str)>,
) {
    std::fs::create_dir_all(store_dir.join("blobs"))
        .expect("create unready recovery carrier blob store");
    std::fs::create_dir_all(store_dir.join("maps"))
        .expect("create unready recovery carrier map store");
    let owned_sources = carriers
        .into_iter()
        .map(|(source_path, companion_path)| {
            serde_json::json!({
                "source_uri": source_path,
                "provider_uri": companion_path,
                "role": "CarrierIde",
                "script_kind": "TSX",
            })
        })
        .collect::<Vec<_>>();
    let mut projects = serde_json::Map::new();
    projects.insert(
        project_file_name.to_string(),
        serde_json::json!({
            "owned_sources": owned_sources,
            "ready_files": {},
        }),
    );
    std::fs::write(
        store_dir.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "epoch": epoch,
            "host_version": "real-recovery-test",
            "projects": projects,
        }))
        .expect("serialize unready recovery carrier manifest"),
    )
    .expect("publish unready recovery carrier manifest");
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
            r#"{"compilerOptions":{"strict":true,"jsx":"preserve"},"include":["*.tsx"]}"#,
        )
        .expect("write real recovery tsconfig");

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

        let crash_notify = Arc::new(Notify::new());
        let initial = TsserverTypeProvider::spawn(
            &node_path,
            &tsserver_path,
            &workspace_root,
            Some(&plugin_path),
            Some(&carrier_store_dir),
            false,
            Some(Arc::clone(&crash_notify)),
        )
        .await
        .expect("spawn initial real tsserver");
        let spawn_attempts = Arc::new(AtomicUsize::new(0));
        let provider = ResilientProvider::new(
            initial,
            Arc::clone(&crash_notify),
            RealTsserverBackend {
                node_path,
                tsserver_path,
                workspace_root,
                plugin_path,
                carrier_store_dir,
                failures_before_success: Arc::new(AtomicUsize::new(failures_before_success)),
                spawn_attempts: Arc::clone(&spawn_attempts),
            },
            Arc::new(TracingNotifier),
            3,
        );

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
        for carrier in &self.carriers {
            let mut last = None;
            for delay_ms in [0u64, 250, 500, 1000, 2000, 4000, 2000] {
                if delay_ms != 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                last = match self
                    .provider
                    .get_hover(&carrier.companion_path, carrier.hover_offset)
                    .await
                {
                    Ok(hover) => hover,
                    Err(_) => continue,
                };
                if let Some(hover) = &last {
                    if hover.contents.contains(carrier.expected_hover)
                        && !hover.contents.contains(": any")
                    {
                        break;
                    }
                }
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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

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
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

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

// ── respawn-failure recovery (D2) ─────────────────────────────────────

/// Backend whose `spawn` fails a configurable number of times before succeeding,
/// recording every attempt. Drives the respawn-retry path deterministically —
/// only the crash monitor calls `spawn`, so the load/store counter needs no CAS.
struct FlakyBackend {
    replacement: MockProvider,
    failures_before_success: Arc<AtomicUsize>,
    spawn_attempts: Arc<AtomicUsize>,
}

impl ResilientBackend<MockProvider> for FlakyBackend {
    fn log_name(&self) -> &'static str {
        "flaky-provider"
    }

    fn user_label(&self) -> &'static str {
        "flaky"
    }

    fn restarting_error(&self) -> &'static str {
        "flaky provider is restarting"
    }

    fn spawn<'a>(
        &'a self,
        _crash_notify: Arc<Notify>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MockProvider, TypeProviderError>> + Send + 'a>,
    > {
        self.spawn_attempts.fetch_add(1, Ordering::Relaxed);
        let provider = self.replacement.clone();
        let remaining = self.failures_before_success.load(Ordering::Relaxed);
        if remaining > 0 {
            self.failures_before_success
                .store(remaining - 1, Ordering::Relaxed);
            Box::pin(async { Err(TypeProviderError::new("spawn failed (test)")) })
        } else {
            Box::pin(async move { Ok(provider) })
        }
    }
}

fn make_flaky(
    initial: MockProvider,
    replacement: MockProvider,
    failures_before_success: usize,
) -> (
    ResilientProvider<MockProvider, FlakyBackend>,
    Arc<Notify>,
    Arc<AtomicUsize>,
) {
    let crash_notify = Arc::new(Notify::new());
    let spawn_attempts = Arc::new(AtomicUsize::new(0));
    let provider = ResilientProvider::new(
        initial,
        Arc::clone(&crash_notify),
        FlakyBackend {
            replacement,
            failures_before_success: Arc::new(AtomicUsize::new(failures_before_success)),
            spawn_attempts: Arc::clone(&spawn_attempts),
        },
        Arc::new(TracingNotifier),
        3,
    );
    (provider, crash_notify, spawn_attempts)
}

/// Spin (virtual-clock) until `check` holds. Sleep-based so the paused clock
/// advances through the monitor's backoff sleeps; the bound is a failsafe.
async fn spin_until(provider: &ResilientProvider<MockProvider, FlakyBackend>, up: bool) -> bool {
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
        make_flaky(initial, replacement.clone(), usize::MAX >> 1);

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
