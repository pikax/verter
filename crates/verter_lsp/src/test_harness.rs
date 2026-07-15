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

impl TestProviderKind {
    /// The require-mode env var that turns this provider's absence into a HARD
    /// failure instead of a graceful skip. CI sets `VERTER_REQUIRE_TSGO=1` (see
    /// `.github/workflows/ci.yml`), so the tsgo real-provider parity tests
    /// genuinely gate there and can never skip-as-pass on a runner where the
    /// asset is expected. `VERTER_REQUIRE_TSSERVER` is the analogous knob for
    /// the tsserver variant.
    fn require_env(self) -> &'static str {
        match self {
            TestProviderKind::Tsserver => "VERTER_REQUIRE_TSSERVER",
            TestProviderKind::Tsgo => "VERTER_REQUIRE_TSGO",
        }
    }

    fn label(self) -> &'static str {
        match self {
            TestProviderKind::Tsserver => "tsserver",
            TestProviderKind::Tsgo => "tsgo",
        }
    }
}

// ---------------------------------------------------------------------------
// Require-mode (fail-closed) provider gating
// ---------------------------------------------------------------------------

/// What an absent provider means for a real-provider test: a HARD failure when
/// the run requires that provider (`VERTER_REQUIRE_{TSGO,TSSERVER}=1`, e.g.
/// strict CI), else a graceful skip. Pure so both branches are unit-tested
/// regardless of whether the provider happens to be installed on the running
/// machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderAbsence {
    /// Required but missing — the test must FAIL (never skip-as-pass).
    HardFail,
    /// Not required — record a skip and degrade gracefully.
    SkipWithReason,
}

/// Pure decision: given whether the provider is required, how should its
/// absence be handled.
pub(crate) fn provider_absence_outcome(required: bool) -> ProviderAbsence {
    if required {
        ProviderAbsence::HardFail
    } else {
        ProviderAbsence::SkipWithReason
    }
}

/// Read the require-mode env var for a provider kind (`"1"` ⇒ required).
fn provider_required(kind: TestProviderKind) -> bool {
    std::env::var(kind.require_env())
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Resolve an absent-provider situation: under require-mode this PANICS (the
/// fail-closed gate); otherwise it prints a skip marker and returns `None` so
/// the caller returns early. A skip is never reported as a pass.
///
/// Split from the env read (`provider_required`) so the panic-vs-skip policy
/// (`provider_absence_outcome`) is independently unit testable.
fn handle_absent_provider(kind: TestProviderKind, reason: &str) -> Option<RealProviderTestSession> {
    match provider_absence_outcome(provider_required(kind)) {
        ProviderAbsence::HardFail => panic!(
            "{}=1 but the {} real-provider test cannot run: {reason}",
            kind.require_env(),
            kind.label(),
        ),
        ProviderAbsence::SkipWithReason => {
            eprintln!("skipping ({}): {reason}", kind.label());
            None
        }
    }
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
    suppress_imported_carrier_prewarm: bool,
    plugin_response_remap: bool,
    resilient: bool,
}

impl TestSessionBuilder {
    pub(crate) fn new(kind: TestProviderKind) -> Self {
        Self {
            kind,
            fixture: None,
            fixture_files: Vec::new(),
            virtual_files: Vec::new(),
            suppress_imported_carrier_prewarm: false,
            // The verter_lsp-internal backend is the DEFAULT: the Rust merge layer
            // is the sole companion→source response mapper, so the spawned plugin
            // returns RAW companion responses. A test exercising the VS Code DIRECT
            // surface (the plugin as the sole mapper) opts in via
            // `plugin_response_remap(true)`.
            plugin_response_remap: false,
            resilient: false,
        }
    }

    /// Wrap the spawned `TsserverTypeProvider` in the PRODUCTION
    /// [`ResilientProvider`](crate::resilient_provider::ResilientProvider) via
    /// `crate::tsserver::resilient::new` — the exact wrap `try_spawn_tsserver`
    /// installs in `main.rs`. This exercises the carrier path THROUGH the wrapper
    /// (the production seam), not the raw provider; only meaningful for the tsserver
    /// kind. No production wiring changes — this only chooses to build the same wrap
    /// the binary builds.
    pub(crate) fn resilient(mut self, enabled: bool) -> Self {
        self.resilient = enabled;
        self
    }

    /// Spawn the tsserver plugin with companion→source RESPONSE remap ENABLED —
    /// the VS Code DIRECT surface, where the plugin (not verter_lsp) is the sole
    /// response mapper. The default (`false`) is the verter_lsp-internal backend,
    /// where the Rust merge layer maps and the plugin returns raw responses.
    /// Only meaningful for the tsserver provider (the plugin lives there).
    pub(crate) fn plugin_response_remap(mut self, enabled: bool) -> Self {
        self.plugin_response_remap = enabled;
        self
    }

    /// Use an E2E fixture workspace root for the project scaffold.
    pub(crate) fn fixture(mut self, name: &str) -> Self {
        self.fixture = Some(name.to_string());
        self
    }

    /// TEST SEAM: suppress the `did_open` imported-carrier-API prewarm.
    ///
    /// With this set, opening a parent `.vue` does NOT eagerly sync an imported
    /// child component's `{carrier}.ts` PUBLIC-API surface — so a cross-file rename
    /// lane can exercise the path where `handle_rename`'s OWN sync-before-query is
    /// the only thing that would sync a closed child's API surface. Under tsserver
    /// that in-`handle_rename` sync opens the child too late to join the parent's
    /// program (the Block H-membership gap), so the lane this seam feeds is
    /// `#[ignore]`'d: it does NOT prove `handle_rename`'s own sync closes the closed
    /// child today — it is the discriminator Block H-membership is validated against.
    #[allow(dead_code)]
    pub(crate) fn suppress_imported_carrier_prewarm(mut self, suppress: bool) -> Self {
        self.suppress_imported_carrier_prewarm = suppress;
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

        // Per-session carrier-store isolation. The production store dir is keyed
        // `(host_version, workspace_root)`, so two sessions over the SAME fixture
        // share one on-disk store and an earlier session's manifest/blobs leak into
        // a later session's cold read. Each session installs a UNIQUE host-version
        // segment (`unique_store_segment`) so its dir is
        // `…/verter-carrier-store/<unique-segment>/<workspace-hash>/`, fully isolated.
        // Both the tsserver spawn (the `VERTER_CARRIER_STORE_DIR` the plugin reads)
        // and the LSP-side publish backend read the SAME segment through
        // `default_carrier_store_host_version`, so they always agree on the dir.
        let store_segment = unique_store_segment();
        let session_carrier_store_dir =
            crate::external_ts::carrier_store_dir_for(&store_segment, &workspace_id);

        let provider: Arc<dyn TypeProvider> = match self.kind {
            TestProviderKind::Tsserver => {
                let tsdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../packages/vue-vscode/node_modules/typescript/lib")
                    .to_string_lossy()
                    .replace('\\', "/");
                let node_path = match crate::tsserver::find_node() {
                    Some(p) => p,
                    None => return handle_absent_provider(self.kind, "node not found"),
                };
                let tsserver_path =
                    match crate::tsserver::find_tsserver(Some(&tsdk), Some(&workspace_id)) {
                        Some(p) => p,
                        None => return handle_absent_provider(self.kind, "tsserver.js not found"),
                    };
                let plugin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../packages/vue-vscode/node_modules")
                    .to_string_lossy()
                    .replace('\\', "/");
                // Deliver the SAME carrier-publish store dir the `VerterLanguageServer`
                // built below publishes into, so the spawned tsserver's plugin reads
                // exactly the store the LSP writes. Both sides resolve the per-session
                // ISOLATED dir: the spawn from `session_carrier_store_dir` here, the
                // LSP backend from the matching `store_segment` override installed
                // around its construction below.
                let carrier_store_dir = session_carrier_store_dir
                    .to_string_lossy()
                    .replace('\\', "/");
                let tsserver_path_str = tsserver_path.to_string_lossy().replace('\\', "/");
                // When `.resilient()` is set, spawn WITH a crash-notify and wrap in
                // the production `ResilientProvider` (the carrier path then runs
                // through the wrapper, the real production seam). Otherwise the raw
                // provider, as before.
                let crash_notify: Option<Arc<tokio::sync::Notify>> = if self.resilient {
                    Some(Arc::new(tokio::sync::Notify::new()))
                } else {
                    None
                };
                let spawned = crate::tsserver::ipc::TsserverTypeProvider::spawn(
                    &node_path,
                    &tsserver_path_str,
                    &workspace_id,
                    Some(&plugin_path),
                    Some(&carrier_store_dir),
                    self.plugin_response_remap,
                    crash_notify.clone(),
                )
                .await;
                let p = match spawned {
                    Ok(p) => p,
                    Err(e) => {
                        return handle_absent_provider(
                            self.kind,
                            &format!("tsserver spawn failed: {e}"),
                        )
                    }
                };
                match crash_notify {
                    Some(crash_notify) => {
                        // Byte-identical to the `main.rs` production wrap: the
                        // notifier rides an empty client cell (logs only) — the test
                        // never injects a real `Client`.
                        let client_cell = Arc::new(tokio::sync::OnceCell::new());
                        let resilient = crate::tsserver::resilient::new(
                            p,
                            crash_notify,
                            node_path,
                            tsserver_path_str,
                            workspace_id.clone(),
                            Some(plugin_path),
                            client_cell,
                            3,
                        );
                        let provider: Arc<dyn TypeProvider> = Arc::new(resilient);
                        provider
                    }
                    None => {
                        let provider: Arc<dyn TypeProvider> = Arc::new(p);
                        provider
                    }
                }
            }
            TestProviderKind::Tsgo => {
                // Canonical discovery: explicit `VERTER_TSGO_BIN` override >
                // repo-local workspace `node_modules` (the SAME tsgo the project
                // pins, regardless of PATH / npm-cache state) > PATH > npm/npx
                // cache. This is the production discovery path — the harness must
                // not fork its own discovery.
                let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let tsgo_bin = match crate::tsgo::ipc::find_tsgo_binary_canonical(Some(&repo_root))
                {
                    Ok(bin) => bin,
                    Err(err) => {
                        return handle_absent_provider(
                            self.kind,
                            &format!("tsgo binary not found: {err}"),
                        )
                    }
                };
                let root_uri = crate::uri::path_to_file_uri_string(&workspace_id);
                // OWNED one-instance dual-surface provider: spawn ONE `tsgo --lsp`
                // (the feature surface), then attach an `--api` checker to the SAME
                // process. The checker stores NO configured project — the carrier's
                // owning tsconfig is supplied per query (its `@/`-aliased import +
                // ambient globals resolve through the real configured project the
                // per-query binding opens) — NOT a config-less inferred project.
                //
                // This is a DELIBERATE lower-level OWNED-provider harness: it injects the
                // BARE `TsgoOwnedProvider`, NOT the production `TsgoCompositeProvider`
                // host-aware admission layer (`wrap_host_aware_admission`). The always-
                // present OWNED carrier-diagnostics ADMISSION gate (a non-bound carrier
                // fails closed) is covered hermetically + discriminatingly by
                // `crates/verter_lsp/tests/owned_binding_gate.rs` — it does NOT ride this
                // harness. No carrier-diagnostics ADMISSION-GATE assertion depends on this
                // bare (ungated) path: the carrier tests that DO ride the harness assert
                // POSITIVE carrier DX (a bound-carrier `.vue`/`.svelte` in a configured
                // fixture: aliased imports resolve, a deliberate `TS2322` surfaces,
                // go-to-definition lands) plus FEATURE-position fail-closed (a definition
                // at a comment/non-symbol position) — both served by OWNED's `--lsp`
                // surface, identical to what the production composite delegates for a
                // BOUND carrier. So wrapping the harness in the admission layer would not
                // change any assertion here (and would require the harness to publish an
                // owning snapshot to the composite's binding host).
                let inner =
                    match crate::tsgo::ipc::TsgoTypeProvider::spawn(&tsgo_bin, &root_uri).await {
                        Ok(p) => Arc::new(p),
                        Err(e) => {
                            return handle_absent_provider(
                                self.kind,
                                &format!("tsgo spawn failed: {e}"),
                            )
                        }
                    };
                match crate::tsgo::ipc::TsgoOwnedProvider::attach(inner, &tsgo_bin).await {
                    Ok(owned) => Arc::new(owned),
                    Err(e) => {
                        return handle_absent_provider(
                            self.kind,
                            &format!("tsgo --api attach failed: {e}"),
                        )
                    }
                }
            }
        };

        let provider_kind = match self.kind {
            TestProviderKind::Tsserver => crate::TypeProviderKind::Tsserver,
            TestProviderKind::Tsgo => crate::TypeProviderKind::Tsgo,
        };
        let suppress_imported_carrier_prewarm = self.suppress_imported_carrier_prewarm;

        let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> =
            Arc::new(verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            ));
        let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
        let host_for_server = Arc::clone(&host);
        let type_provider_for_server = Arc::clone(&provider);
        // Construct the server with the per-session store-dir override installed:
        // `VerterLanguageServer::new` builds the tsserver `CarrierPublishCoordinator`
        // (whose backend reads `default_carrier_store_host_version`) and spawns the
        // sync coordinator that captures it, all SYNCHRONOUSLY inside this factory.
        // `with_isolated_store_segment` holds the install lock across that synchronous
        // construction, so the LSP backend resolves the SAME isolated dir the spawn
        // above used and no concurrent session observes this session's segment.
        let (service, socket) = with_isolated_store_segment(&store_segment, || {
            tower_lsp_server::LspService::new(move |client| {
                VerterLanguageServer::new(
                    client,
                    LspConfig {
                        host: Arc::clone(&host_for_server),
                        type_provider: Some(Arc::clone(&type_provider_for_server)),
                        project_sync_mode: crate::ProjectSyncMode::FullProject,
                        type_provider_kind: provider_kind,
                        suggest_tsgo: false,
                        mcp_port: None,
                        type_provider_reason: None,
                        suppress_imported_carrier_prewarm,
                    },
                )
            })
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
            carrier_store_dir: session_carrier_store_dir,
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
    /// This session's ISOLATED carrier-publish store dir
    /// (`…/verter-carrier-store/<unique-segment>/<workspace-hash>/`). Unique per
    /// session so no carrier-store state leaks between tests; removed on
    /// [`Self::shutdown`]. Both the spawned plugin (via `VERTER_CARRIER_STORE_DIR`)
    /// and the LSP-side publish backend resolve exactly this dir.
    carrier_store_dir: std::path::PathBuf,
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

    /// This session's ISOLATED carrier-publish store dir. Unique per session, so
    /// no carrier-store state leaks between tests sharing a fixture workspace root.
    #[allow(dead_code)]
    pub(crate) fn carrier_store_dir(&self) -> &std::path::Path {
        &self.carrier_store_dir
    }

    /// Returns `true` when this session uses TSGO.
    pub(crate) fn is_tsgo(&self) -> bool {
        matches!(self.kind, TestProviderKind::Tsgo)
    }

    /// Fail-closed gate for a controlled provider result that came back empty.
    ///
    /// A real-provider regression test that needs a NON-empty result from a
    /// known-good fixture (e.g. member completions for `obj.`) must not treat an
    /// empty result as a silent skip: under require-mode (`VERTER_REQUIRE_{TSGO,
    /// TSSERVER}=1`, set in CI) an empty result is a genuine provider /
    /// materialization regression and PANICS. Off require-mode it returns `false`
    /// so the caller can degrade gracefully (`return`), preserving local
    /// ergonomics on a machine where the provider cannot materialize.
    ///
    /// Returns `true` only in the (non-required) skip case so the call reads
    /// `if session.allow_empty_result_skip(reason) { return; }`.
    #[must_use]
    pub(crate) fn allow_empty_result_skip(&self, reason: &str) -> bool {
        if provider_required(self.kind) {
            panic!(
                "{}=1 but the {} real-provider test got an empty result for a controlled \
                 fixture (provider/materialization regression): {reason}",
                self.kind.require_env(),
                self.kind.label(),
            );
        }
        eprintln!("skipping ({}): {reason}", self.kind.label());
        true
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

    /// Open an on-disk fixture file DIRECTLY in the type provider, reading its
    /// real disk content, and return `(provider_path, content)`.
    ///
    /// This reads a file that PHYSICALLY EXISTS on disk under the fixture
    /// `tsconfig` `include` and opens it with that real content, so it is a
    /// CONFIGURED-PROJECT member on both providers. Contrast [`open_in_provider`],
    /// which opens whatever inline content the caller passes at the given path: a
    /// synthetic path with no on-disk counterpart lands such a buffer in tsserver's
    /// *inferred* project, whose auto-import map excludes configured-project
    /// siblings. Use THIS helper when the use-site must be a real tsconfig member —
    /// the realistic shape for a PLAIN on-disk script use-site of provider-level
    /// features (auto-import resolve) that depend on the use-site being part of the
    /// same tsconfig project as the workspace export it imports. It does NOT model a
    /// framework carrier's generated virtual TSX, whose project membership for
    /// tsserver is a separate concern.
    ///
    /// The disk read is owned HERE (the test-fixture-read boundary) so callers
    /// receive the content without an extra `std::fs` read of their own.
    pub(crate) async fn open_fixture_in_provider(&self, relative_path: &str) -> (String, String) {
        let abs_path = format!("{}/{relative_path}", self.workspace_id);
        let content = std::fs::read_to_string(&abs_path)
            .unwrap_or_else(|e| panic!("fixture file should exist on disk: {abs_path}: {e}"));
        self.provider
            .open_file(&abs_path, &content)
            .await
            .expect("provider open_file should succeed");
        (abs_path, content)
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

    /// The committed carrier [`crate::provider_sync::ProviderSyncState`] for an open
    /// carrier-source URI, or `None` when none has been committed.
    ///
    /// Surfaces the provider-neutral ownership backbone state the carrier-sync
    /// gateway commits when a `.vue`/`.svelte` carrier becomes a configured-project
    /// member, so a real-provider proof can tie a flowing carrier diagnostic to that
    /// membership (an `Owned`, background-loaded state) rather than to a bare
    /// diagnostic appearing by happenstance.
    pub(crate) fn provider_sync_state(
        &self,
        uri: &Uri,
    ) -> Option<crate::provider_sync::ProviderSyncState> {
        self.server().test_provider_sync_state(uri)
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

    /// Get signature help at a position (raw LSP `SignatureHelp`).
    pub(crate) async fn signature_help(
        &self,
        uri: &Uri,
        position: Position,
    ) -> Option<SignatureHelp> {
        let params = SignatureHelpParams {
            context: None,
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        match self.server().signature_help(params).await {
            Ok(help) => help,
            Err(e) => {
                eprintln!("signature_help error: {e}");
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

    /// Fail-closed variant of [`Self::wait_until_ready`] for tests whose
    /// resolution-dependent assertions only run once the provider is warm.
    ///
    /// A plain `if !wait_until_ready(..) { return; }` guard is VACUOUS under the
    /// require gate: on a cold provider the test returns green without ever
    /// reaching its assertions, yet a required-but-cold provider should be a HARD
    /// failure. This helper closes that gap by routing the not-ready case through
    /// the same require-mode policy as an absent provider:
    ///
    /// - ready → `true` (the caller proceeds to its assertions);
    /// - NOT ready AND this provider's REQUIRE env is set
    ///   (`VERTER_REQUIRE_{TSSERVER,TSGO}=1`) → **panic** so the CI gate fails
    ///   loudly instead of reporting a green it never earned;
    /// - NOT ready and NOT required → `false` (the caller does the existing
    ///   graceful local skip — `return`).
    ///
    /// Net effect under the REQUIRE gate: every call site MUST reach its
    /// resolution assertion or panic — there is no silent green.
    pub(crate) async fn require_or_skip_ready(
        &self,
        uri: &Uri,
        needle: &str,
        delta: usize,
        expected_label: &str,
    ) -> bool {
        if self
            .wait_until_ready(uri, needle, delta, expected_label)
            .await
        {
            return true;
        }
        if provider_required(self.kind) {
            panic!(
                "{}=1 but the {} real-provider test never warmed up for {} \
                 (expected completion `{expected_label}` at `{needle}`+{delta}): \
                 a required provider that cannot resolve is a HARD failure, not a skip",
                self.kind.require_env(),
                self.kind.label(),
                uri.as_str(),
            );
        }
        eprintln!(
            "skipping ({}): provider not warmed up for {}",
            self.kind.label(),
            uri.as_str()
        );
        false
    }

    /// MERGED (Verter template/lint + type-provider) diagnostics for an open carrier
    /// document, mapped back onto the carrier source ranges.
    ///
    /// Drives the same `publish_full_diagnostics` merge path the server uses, but
    /// RETURNS the set (the harness drains the client socket, so a pushed set is not
    /// readable). Ensures the carrier is synced first and retries briefly while the
    /// provider's inferred project warms up, so a semantic diagnostic (e.g. a
    /// missing-default-export TS error) that only appears once the program is built
    /// is observed without a flaky first-shot empty read.
    pub(crate) async fn merged_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        let mut last = Vec::new();
        for attempt in 0..8 {
            self.ensure_synced(uri).await;
            let diags = self.server().test_merged_diagnostics(uri).await;
            if !diags.is_empty() {
                return diags;
            }
            last = diags;
            if attempt < 7 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }
        last
    }

    /// Shut down the type provider process and remove this session's isolated
    /// carrier-publish store dir.
    ///
    /// The per-session store tree is owned by THIS session, so removing it on
    /// shutdown keeps the temp dir from accumulating one tree per test across a run
    /// (the production store is long-lived and shared per workspace; the isolated
    /// test tree is not). Best-effort: a removal failure (e.g. the plugin process
    /// still holds a handle on a slow shutdown) is non-fatal — the OS temp dir is
    /// reclaimed eventually and the unique segment guarantees no cross-test reuse.
    pub(crate) async fn shutdown(self) {
        let _ = self.provider.shutdown().await;
        self._drain_handle.abort();
        let _ = std::fs::remove_dir_all(&self.carrier_store_dir);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A UNIQUE carrier-store host-version segment for one test session.
///
/// Used to isolate each real-provider session's on-disk carrier store (see
/// [`TestSessionBuilder::build`]). It keeps the live package-version prefix (so the
/// per-session trees still cluster under the version, matching production layout)
/// and appends a triple that is unique both across PROCESSES (nextest runs one
/// process per test) and WITHIN a process (`cargo test` runs sessions as concurrent
/// threads): the process id, a process-monotonic counter, and a nanosecond clock
/// reading. The result is a portable path segment — only `[0-9a-z.-]`, no
/// NTFS-illegal characters — so it is a valid directory name on every platform.
pub(crate) fn unique_store_segment() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{version}-test-{pid}-{seq}-{nanos}",
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Run `build` with `segment` installed as the active carrier-store host-version
/// override, restoring the prior state afterward.
///
/// The override is read by `default_carrier_store_host_version`, which is the
/// single function BOTH the LSP-side publish backend
/// ([`crate::external_ts::TsserverEngineBackend::with_default_host_version`]) and
/// the tsserver spawn-dir string ([`crate::external_ts::default_carrier_store_dir_string`])
/// call — so installing one segment moves both onto the same per-session ISOLATED
/// dir. `build` MUST be the synchronous server construction (the
/// `LspService::new` whose factory runs `VerterLanguageServer::new`): the install
/// lock is held across it (no `.await`), so a concurrent session never observes
/// this session's segment and the process-global override stays race-free.
///
/// This is the SHARED isolation seam for every real-tsserver test that builds its
/// own server (both [`TestSessionBuilder::build`] and the hand-rolled direct-spawn
/// tests in `server_tests`), so a single derivation keeps the publish side and the
/// plugin side in agreement on the per-session dir.
pub(crate) fn with_isolated_store_segment<T>(segment: &str, build: impl FnOnce() -> T) -> T {
    let _install_guard = crate::external_ts::test_store_dir_override::install_lock();
    crate::external_ts::test_store_dir_override::set(segment);
    let built = build();
    crate::external_ts::test_store_dir_override::clear();
    built
}

/// Resolve an E2E fixture workspace root as a canonical path.
pub(crate) fn fixture_workspace_root(name: &str) -> String {
    let path = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packages/vue-vscode/e2e/fixtures/{name}")),
    )
    .expect("fixture workspace path should canonicalize");
    crate::test_utils::canonical_test_path(&path)
}

/// Materialize the vendored `@pkg/ui` node-modules package (a TS-declared Vue
/// component exported via package `exports`) into a fixture's `node_modules` at
/// runtime, returning the workspace root.
///
/// `node_modules` is gitignored repo-wide, so a vendored package cannot be
/// committed; the import-matrix nodenext test creates its own dependency this
/// way (hermetic, reproducible, no external corpus). The disk write lives here
/// in the (fixture-setup) harness — the same test-fixture-write category as the
/// rest of this module — rather than in the test file. Paths use `PathBuf::join`
/// (cross-platform).
pub(crate) fn materialize_pkg_ui(fixture: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(fixture_workspace_root(fixture));
    let pkg = root.join("node_modules").join("@pkg").join("ui");
    let dist = pkg.join("dist");
    std::fs::create_dir_all(&dist).expect("create @pkg/ui dist dir");
    std::fs::write(
        pkg.join("package.json"),
        r#"{
  "name": "@pkg/ui",
  "version": "1.0.0",
  "type": "module",
  "exports": {
    ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" }
  }
}
"#,
    )
    .expect("write @pkg/ui package.json");
    std::fs::write(
        dist.join("index.d.ts"),
        "import type { DefineComponent } from \"vue\";\n\
         export declare const PkgComp: DefineComponent<{ pkgRootOnly: string }>;\n",
    )
    .expect("write @pkg/ui index.d.ts");
    std::fs::write(dist.join("index.js"), "export const PkgComp = {};\n")
        .expect("write @pkg/ui index.js");
    root
}

/// Materialize the vendored `@pkg/vuecomp` node-modules package (whose only
/// component export is a RAW `.vue` SFC) into a fixture's `node_modules` at
/// runtime, returning the workspace root. Same rationale/category as
/// [`materialize_pkg_ui`].
pub(crate) fn materialize_pkg_vuecomp(fixture: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(fixture_workspace_root(fixture));
    let pkg = root.join("node_modules").join("@pkg").join("vuecomp");
    std::fs::create_dir_all(&pkg).expect("create @pkg/vuecomp dir");
    std::fs::write(
        pkg.join("package.json"),
        r#"{
  "name": "@pkg/vuecomp",
  "version": "1.0.0",
  "type": "module",
  "exports": { "./Vendored.vue": "./Vendored.vue" }
}
"#,
    )
    .expect("write @pkg/vuecomp package.json");
    std::fs::write(
        pkg.join("Vendored.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ vendoredVueOnly: string }>()\n</script>\n\
         <template><div>{{ vendoredVueOnly }}</div></template>\n",
    )
    .expect("write Vendored.vue");
    root
}

/// Link one already-resolved package directory into a fixture's `node_modules`
/// under `link_name`, cross-platform. On Windows a **directory junction**
/// A FLAT, dependency-free `vue` type stub: exactly the surface the generated Vue
/// component IDE/API carriers consume (`defineComponent`, `PublicProps`,
/// `HTMLAttributes`). See [`materialize_external_ts_dx_deps`] for why a stub is
/// preferred over the real pnpm-installed `vue` here.
const VUE_TYPE_STUB_DTS: &str = r#"// Minimal `vue` type surface for the external-TS-DX fixture carriers.
export type PublicProps = { class?: unknown; style?: unknown };
export type HTMLAttributes = Record<string, unknown>;
export declare function defineComponent<P = {}>(options: P): {
  new (...args: any[]): { $props: P };
};
export {};
"#;

/// Make the `external-ts-dx` fixture self-sufficient for the §2.9 plain-`.ts`-
/// imports-`.vue`/`.svelte` enhanced-DX contract: provide a flat dependency-free
/// `vue` type stub and materialise `@verter/types` from the bundled standalone
/// declaration (the same artifact the production server writes).
///
/// `node_modules` is gitignored repo-wide, so these deps cannot be committed and
/// must be materialised at test time (the same fixture-setup category as
/// [`materialize_pkg_ui`]). The generated component carriers reference `vue` and
/// `@verter/types`; without them the `$props` surface degrades to `any` and the
/// types-flow assertions become vacuous. A hand-written flat stub keeps the
/// surface honest and deterministic while staying hermetic — no external corpus,
/// no fragile pnpm-store transitive-symlink resolution.
///
/// The Svelte component carrier is self-contained (Verter synthesises its public
/// instance surface with no `svelte` dependency), so no `svelte` stub is needed.
///
/// Returns the fixture workspace root.
pub(crate) fn materialize_external_ts_dx_deps() -> std::path::PathBuf {
    let root = std::path::PathBuf::from(fixture_workspace_root("external-ts-dx"));
    let node_modules = root.join("node_modules");
    let _ = std::fs::create_dir_all(&node_modules);

    // A FLAT, self-contained `vue` type stub providing exactly the surface the
    // generated component IDE/API carriers consume (`defineComponent`,
    // `PublicProps`, `HTMLAttributes`). Hand-written + dependency-free so the
    // `$props` surface resolves to the REAL declared member type (e.g.
    // `verterDxHeadline: string`) deterministically — without dragging in vue's
    // transitive `@vue/*` + `csstype` closure (whose pnpm-store symlink layout is
    // not junction-resolvable). This is a TYPE stub only; the §2.9 contract is
    // about the carrier-resolved prop surface flowing into the `.ts`, not vue's
    // runtime.
    let vue_dir = node_modules.join("vue");
    let _ = std::fs::create_dir_all(&vue_dir);
    let _ = std::fs::write(vue_dir.join("index.d.ts"), VUE_TYPE_STUB_DTS);
    let _ = std::fs::write(
        vue_dir.join("package.json"),
        r#"{"name":"vue","version":"3.0.0-stub","types":"index.d.ts"}"#,
    );

    // `@verter/types` from the bundled standalone declaration.
    let types_dir = node_modules.join("@verter").join("types");
    let _ = std::fs::create_dir_all(&types_dir);
    let _ = std::fs::write(
        types_dir.join("index.d.ts"),
        verter_session::VERTER_TYPES_STANDALONE_DTS,
    );
    let _ = std::fs::write(
        types_dir.join("package.json"),
        r#"{"name":"@verter/types","types":"index.d.ts"}"#,
    );

    root
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

#[cfg(test)]
#[path = "test_harness_tests.rs"]
mod test_harness_tests;
