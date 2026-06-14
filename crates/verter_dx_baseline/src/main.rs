//! `verter-dx-baseline` — the DX harness differential-baseline bridge.
//!
//! Two entry points, selected by argv:
//!
//! - default (no args): a newline-delimited JSON dispatch loop over
//!   stdin/stdout. The TS DX runner speaks [`protocol::Request`] frames; the
//!   bridge owns provider discovery + spawn, the versioned artifact overlay, and
//!   normalized provider output. The runner never re-implements any of that.
//! - `materialize`: a one-shot that reads a materialization request as JSON on
//!   stdin and writes the report as JSON on stdout. This is the host-driven
//!   baseline materializer the runner uses to produce `.vue.tsx` / `.vue.ts`
//!   artifacts via public `verter_session` APIs.
//!
//! Project-parity ownership (the contract boundary): this crate owns ONLY the
//! tsgo-standalone-on-emitted-TSX half of the known-good `<script setup>` hover
//! parity check (materialize the `.vue.tsx` through the public host, query a real
//! tsgo, assert the known-good type). It deliberately does NOT spawn `verter-lsp`.
//! The opposing verter@tsgo half needs verter's own hover — the merge of
//! template/script position mapping with provider results inside `verter_lsp`,
//! which is not a public API to this crate — and the cross-tool identity
//! comparison that pairs the two halves is owned by the DX harness integration
//! gate (a later harness component), not reproduced here.
//!
//! Auto-import and completion-resolve differential is likewise outside this
//! bridge: producing and validating the resolve/edit shape is owned by the
//! raw-LSP auto-import collector and the extension-host accept gate. The bridge
//! exposes normalized provider output only and runs no completion-resolve route.
//! (`verter_type_runtime::TypeProvider` already exposes `resolve_completion`, so
//! that collector needs no product-trait change when it is built.)
//!
//! All logging goes to stderr — stdout is reserved for the protocol channel.

mod artifact_overlay;
mod materialize;
mod protocol;
mod provider;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use verter_type_runtime::TypeProvider;

use crate::artifact_overlay::{ArtifactOverlay, ProbeStatus};
use crate::protocol::{
    AppliedSync, BaselineFile, DiagnosticsRequest, DiagnosticsResponse, ErrorKind, HelloRequest,
    HelloResponse, NormalizedCompletionItem, NormalizedDiagnostic, NormalizedHover,
    NormalizedLocation, OpenRequest, OpenResponse, ProviderCapabilities, ProviderName, QueryMethod,
    QueryRequest, QueryResponse, QueryResult, Request, Response, ShutdownResponse, SyncAction,
    SyncArtifactsRequest, SyncArtifactsResponse,
};
use crate::provider::Resolution;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("materialize") => {
            if let Err(e) = run_materialize_stdin() {
                eprintln!("materialize error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run_stdio());
        }
    }
}

fn init_tracing() {
    let filter = std::env::var("VERTER_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "warn".to_string());
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init();
}

// ── protocol loop ──────────────────────────────────────────────────────────

async fn run_stdio() {
    init_tracing();
    let reader = BufReader::new(tokio::io::stdin());
    let mut out = tokio::io::stdout();
    let mut bridge = Bridge::new();
    dispatch_loop(reader, &mut out, &mut bridge).await;
}

/// Drive the newline-delimited JSON protocol over `reader`/`out`. Extracted from
/// `run_stdio` so the read-error and EOF branches are unit-testable over an
/// in-memory reader/writer.
async fn dispatch_loop<R, W>(reader: R, out: &mut W, bridge: &mut Bridge)
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Request>(line) {
                    Ok(req) => {
                        let (resp, shutdown) = bridge.handle(req).await;
                        write_line(out, &resp).await;
                        if shutdown {
                            break;
                        }
                    }
                    Err(e) => {
                        let resp =
                            Response::error(ErrorKind::InvalidRequest, format!("bad request: {e}"));
                        write_line(out, &resp).await;
                    }
                }
            }
            // Clean EOF: stdin closed.
            Ok(None) => break,
            // A non-UTF-8 line or an I/O error on stdin emits an explicit reason
            // frame before stopping, rather than terminating the loop silently.
            Err(e) => {
                let resp =
                    Response::error(ErrorKind::InvalidRequest, format!("stdin read error: {e}"));
                write_line(out, &resp).await;
                break;
            }
        }
    }
}

async fn write_line<W: tokio::io::AsyncWrite + Unpin>(out: &mut W, resp: &Response) {
    let mut line = serde_json::to_string(resp).unwrap_or_else(|e| {
        format!(r#"{{"type":"error","kind":"invalid_request","message":"serialize failed: {e}"}}"#)
    });
    line.push('\n');
    let _ = out.write_all(line.as_bytes()).await;
    let _ = out.flush().await;
}

// ── bridge ───────────────────────────────────────────────────────────────

/// A spawned provider together with its capability metadata. Folding the two
/// into one struct makes "the bridge is ready ⇒ capabilities are present" a
/// structural invariant — there is no provider-without-capabilities state left
/// to `.expect()` on the probe paths.
struct ReadyProvider {
    provider: Arc<dyn TypeProvider>,
    capabilities: ProviderCapabilities,
}

/// The per-session bridge state: the spawned provider (with its capabilities),
/// the versioned artifact overlay, and the probe counter.
struct Bridge {
    ready: Option<ReadyProvider>,
    overlay: ArtifactOverlay,
    skipped: bool,
    baseline_ran: u64,
}

impl Bridge {
    fn new() -> Self {
        Bridge {
            ready: None,
            overlay: ArtifactOverlay::new(),
            skipped: false,
            baseline_ran: 0,
        }
    }

    /// Dispatch one request. Returns the response and whether the loop should
    /// stop (shutdown).
    async fn handle(&mut self, req: Request) -> (Response, bool) {
        match req {
            Request::Hello(h) => (self.on_hello(h).await, false),
            Request::Open(o) => (self.on_open(o).await, false),
            Request::SyncArtifacts(s) => (self.on_sync(s).await, false),
            Request::Query(q) => (self.on_query(q).await, false),
            Request::Diagnostics(d) => (self.on_diagnostics(d).await, false),
            Request::Shutdown => (self.on_shutdown().await, true),
        }
    }

    async fn on_hello(&mut self, h: HelloRequest) -> Response {
        let resolution =
            match provider::resolve(h.provider, &h.tool_root, &h.workspace_root, h.strict_ci) {
                Ok(r) => r,
                Err(e) => return Response::error(e.kind(), e.to_string()),
            };

        let (tool_root_used, plan) = match resolution {
            Resolution::Ready {
                tool_root_used,
                plan,
            } => (tool_root_used, plan),
            Resolution::Skipped { reason } => return self.mark_skipped(h.provider, reason),
        };

        match provider::spawn(h.provider, &plan, &h.workspace_root).await {
            Ok(p) => {
                let capabilities = ProviderCapabilities::for_provider(h.provider);
                self.ready = Some(ReadyProvider {
                    provider: p,
                    capabilities: capabilities.clone(),
                });
                Response::Hello(HelloResponse {
                    ok: true,
                    provider: h.provider,
                    skipped: false,
                    skip_reason: None,
                    baseline_tool_root_used: Some(tool_root_used),
                    capabilities: Some(capabilities),
                })
            }
            Err(e) if h.strict_ci => Response::error(
                ErrorKind::ProviderError,
                format!("provider spawn failed: {e}"),
            ),
            Err(e) => self.mark_skipped(h.provider, format!("provider spawn failed: {e}")),
        }
    }

    fn mark_skipped(&mut self, provider: ProviderName, reason: String) -> Response {
        self.skipped = true;
        Response::Hello(HelloResponse {
            ok: true,
            provider,
            skipped: true,
            skip_reason: Some(reason),
            baseline_tool_root_used: None,
            capabilities: None,
        })
    }

    async fn on_open(&mut self, o: OpenRequest) -> Response {
        if self.skipped {
            return Response::Open(OpenResponse {
                ok: true,
                opened: vec![],
                version: o.version,
            });
        }
        if self.ready.is_none() {
            return Response::error(ErrorKind::NotInitialized, "open before hello");
        }
        // Plan the provider actions, apply them FIRST, and only then stamp the
        // overlay — a provider failure must leave the overlay un-advanced.
        let applied = self.overlay.plan(&o.files);
        if let Err(resp) = self.apply_files(&o.files, &applied).await {
            return resp;
        }
        self.overlay.commit_open(&o.files, &applied, o.version);
        let opened = o.files.into_iter().map(|f| f.path).collect();
        Response::Open(OpenResponse {
            ok: true,
            opened,
            version: o.version,
        })
    }

    async fn on_sync(&mut self, s: SyncArtifactsRequest) -> Response {
        if self.skipped {
            return Response::SyncArtifacts(SyncArtifactsResponse {
                ok: true,
                uri: s.uri,
                version: s.version,
                applied: vec![],
            });
        }
        if self.ready.is_none() {
            return Response::error(ErrorKind::NotInitialized, "syncArtifacts before hello");
        }
        // Apply the provider updates FIRST; stamp the overlay version ONLY after
        // every update for this sync succeeds. If a provider update fails the
        // overlay must NOT advance, or a later probe at this version would wrongly
        // skip the stale-baseline refusal.
        let applied = self.overlay.plan(&s.files);
        if let Err(resp) = self.apply_files(&s.files, &applied).await {
            return resp;
        }
        self.overlay.commit_sync(
            &s.uri,
            s.version,
            &s.files,
            &applied,
            &s.changed_public_api_twins,
            s.source_map_identity.clone(),
        );
        Response::SyncArtifacts(SyncArtifactsResponse {
            ok: true,
            uri: s.uri,
            version: s.version,
            applied,
        })
    }

    /// Push each file to the provider with the action the overlay planned.
    async fn apply_files(
        &self,
        files: &[BaselineFile],
        applied: &[AppliedSync],
    ) -> Result<(), Response> {
        let provider = match &self.ready {
            Some(r) => Arc::clone(&r.provider),
            None => return Err(Response::error(ErrorKind::NotInitialized, "no provider")),
        };
        for (f, a) in files.iter().zip(applied.iter()) {
            let result = match a.action {
                SyncAction::Opened => provider.open_file(&f.path, &f.content).await,
                SyncAction::Loaded => provider.load_file(&f.path, &f.content).await,
                SyncAction::Updated => provider.update_file(&f.path, &f.content).await,
            };
            if let Err(e) = result {
                return Err(Response::error(
                    ErrorKind::ProviderError,
                    format!("apply {} failed: {e}", f.path),
                ));
            }
        }
        Ok(())
    }

    async fn on_query(&mut self, q: QueryRequest) -> Response {
        let (provider, caps) = match self.ready_provider() {
            Ok(rp) => rp,
            Err(resp) => return resp,
        };
        // Path-precise staleness: gate on the SPECIFIC generated artifact being
        // probed, not the authored-URI rollup. A sync that refreshed only a
        // sibling artifact of this document (e.g. the `.vue.ts` twin) must not
        // clear a probe for this still-stale generated path.
        if let ProbeStatus::Stale { have } = self.overlay.probe_path_status(&q.path, q.version) {
            return Response::stale(&q.uri, q.version, have);
        }
        // A probe that requires a source map is refused when none is present for
        // the targeted artifact (the `$/getCompiledCode` map-absent contract).
        if q.requires_source_map && !self.overlay.source_map_present(&q.uri) {
            return Response::map_absent(&q.uri, q.version);
        }

        let result = match q.method {
            QueryMethod::Hover => match provider.get_hover(&q.path, q.offset).await {
                Ok(h) => QueryResult::Hover {
                    hover: h.as_ref().map(NormalizedHover::from),
                },
                Err(e) => return Response::error(ErrorKind::ProviderError, e.to_string()),
            },
            QueryMethod::Completion => {
                match provider
                    .get_completions(&q.path, q.offset, q.trigger_character.as_deref())
                    .await
                {
                    Ok(c) => QueryResult::Completion {
                        items: c.items.iter().map(NormalizedCompletionItem::from).collect(),
                        is_incomplete: c.is_incomplete,
                    },
                    Err(e) => return Response::error(ErrorKind::ProviderError, e.to_string()),
                }
            }
            QueryMethod::Definition => match provider.get_definition(&q.path, q.offset).await {
                Ok(locs) => QueryResult::Definition {
                    locations: locs.iter().map(NormalizedLocation::from).collect(),
                },
                Err(e) => return Response::error(ErrorKind::ProviderError, e.to_string()),
            },
            QueryMethod::TypeDefinition => {
                match provider.get_type_definition(&q.path, q.offset).await {
                    Ok(locs) => QueryResult::Definition {
                        locations: locs.iter().map(NormalizedLocation::from).collect(),
                    },
                    Err(e) => return Response::error(ErrorKind::ProviderError, e.to_string()),
                }
            }
            QueryMethod::References => match provider.get_references(&q.path, q.offset).await {
                Ok(locs) => QueryResult::Definition {
                    locations: locs.iter().map(NormalizedLocation::from).collect(),
                },
                Err(e) => return Response::error(ErrorKind::ProviderError, e.to_string()),
            },
        };

        self.baseline_ran += 1;
        Response::Query(QueryResponse {
            method: q.method,
            uri: q.uri,
            version: q.version,
            result,
            capabilities: caps,
        })
    }

    async fn on_diagnostics(&mut self, d: DiagnosticsRequest) -> Response {
        let (provider, caps) = match self.ready_provider() {
            Ok(rp) => rp,
            Err(resp) => return resp,
        };
        // Path-precise staleness: gate on the SPECIFIC generated artifact being
        // probed, not the authored-URI rollup (the same gate `on_query` applies).
        if let ProbeStatus::Stale { have } = self.overlay.probe_path_status(&d.path, d.version) {
            return Response::stale(&d.uri, d.version, have);
        }
        // A diagnostics probe that requires a source map is refused when none is
        // present for the targeted artifact — the same map-presence gate `query`
        // applies, so map-absent enforcement is consistent across probe kinds.
        if d.requires_source_map && !self.overlay.source_map_present(&d.uri) {
            return Response::map_absent(&d.uri, d.version);
        }
        match provider.get_diagnostics(&d.path).await {
            Ok(diags) => {
                self.baseline_ran += 1;
                Response::Diagnostics(DiagnosticsResponse {
                    uri: d.uri,
                    version: d.version,
                    diagnostics: diags.iter().map(NormalizedDiagnostic::from).collect(),
                    capabilities: caps,
                })
            }
            Err(e) => Response::error(ErrorKind::ProviderError, e.to_string()),
        }
    }

    async fn on_shutdown(&mut self) -> Response {
        if let Some(r) = &self.ready {
            let _ = r.provider.shutdown().await;
        }
        Response::Shutdown(ShutdownResponse {
            ok: true,
            baseline_ran: self.baseline_ran,
        })
    }

    /// Clone the provider Arc and its capabilities if the bridge is ready to
    /// probe, else the refusal response. Capabilities ride along structurally
    /// (no separate `Option` to unwrap).
    fn ready_provider(&self) -> Result<(Arc<dyn TypeProvider>, ProviderCapabilities), Response> {
        if self.skipped {
            return Err(Response::error(
                ErrorKind::NotInitialized,
                "baseline provider not available (skipped)",
            ));
        }
        match &self.ready {
            Some(r) => Ok((Arc::clone(&r.provider), r.capabilities.clone())),
            None => Err(Response::error(
                ErrorKind::NotInitialized,
                "probe before hello",
            )),
        }
    }
}

// ── materialize one-shot ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeCli {
    workspace_root: String,
    #[serde(default)]
    entries: Vec<String>,
    #[serde(default)]
    vendor_node_modules: Option<String>,
    /// The resolved Vue line the vendored `vue`/`@vue/*` declarations must match.
    #[serde(default)]
    expected_vue_version: Option<String>,
    /// Strict CI hard-fails on a vendored-Vue version mismatch; non-strict records
    /// a structured warning.
    #[serde(default)]
    strict_vue_version: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactDto {
    source_vue: String,
    generated_path: String,
    source_map_present: bool,
    /// The artifact's V3 source map, ALREADY shifted to match the rewritten
    /// `.vue`→`.vue.ts` generated code (identical to
    /// [`materialize::MaterializedArtifact::source_map`]). Carried across the
    /// CLI/DTO boundary so the TS runner projects generated positions back to Vue
    /// against the SAME map verter emitted — never the host's pre-rewrite map.
    /// Omitted when the host produced no map.
    #[serde(skip_serializing_if = "Option::is_none")]
    source_map: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompileErrorDto {
    canonical: String,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VueVersionWarningDto {
    package: String,
    expected: String,
    found: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeCliResult {
    ide_artifacts: Vec<ArtifactDto>,
    public_api_twins: Vec<ArtifactDto>,
    verter_types_dts: Option<String>,
    map_absent: Vec<String>,
    source_map_identities: BTreeMap<String, String>,
    compile_errors: Vec<CompileErrorDto>,
    tsconfig_path: Option<String>,
    synthesized_tsconfig: bool,
    support_rewrites: Vec<String>,
    vue_version_warnings: Vec<VueVersionWarningDto>,
}

fn run_materialize_stdin() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let cli: MaterializeCli = serde_json::from_str(&input)?;

    let req = materialize::MaterializeRequest {
        workspace_root: PathBuf::from(&cli.workspace_root),
        entries: cli.entries.iter().map(PathBuf::from).collect(),
        vendor_node_modules: cli.vendor_node_modules.as_deref().map(PathBuf::from),
        expected_vue_version: cli.expected_vue_version.clone(),
        strict_vue_version: cli.strict_vue_version,
    };
    let report = materialize::materialize(&req)?;

    let dto = MaterializeCliResult {
        ide_artifacts: report.ide_artifacts.iter().map(artifact_dto).collect(),
        public_api_twins: report.public_api_twins.iter().map(artifact_dto).collect(),
        verter_types_dts: report
            .verter_types_dts
            .as_ref()
            .map(|p| p.display().to_string()),
        map_absent: report.map_absent,
        source_map_identities: report.source_map_identities,
        compile_errors: report
            .compile_errors
            .into_iter()
            .map(|(canonical, message)| CompileErrorDto { canonical, message })
            .collect(),
        tsconfig_path: report
            .tsconfig_path
            .as_ref()
            .map(|p| p.display().to_string()),
        synthesized_tsconfig: report.synthesized_tsconfig,
        support_rewrites: report
            .support_rewrites
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        vue_version_warnings: report
            .vue_version_warnings
            .into_iter()
            .map(|w| VueVersionWarningDto {
                package: w.package,
                expected: w.expected,
                found: w.found,
            })
            .collect(),
    };
    println!("{}", serde_json::to_string(&dto)?);
    Ok(())
}

fn artifact_dto(a: &materialize::MaterializedArtifact) -> ArtifactDto {
    ArtifactDto {
        source_vue: a.source_vue.clone(),
        generated_path: a.generated_path.display().to_string(),
        source_map_present: a.source_map_present,
        source_map: a.source_map.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use verter_type_runtime::protocol::InlayHint;
    use verter_type_runtime::protocol::{
        CompletionResult, HoverInfo, TypeDiagnostic, TypeLocation, TypeProviderError,
    };
    use verter_type_runtime::protocol::{
        RenameLocation, SemanticToken, SignatureHelp, TypeCodeAction, TypeDocumentHighlight,
    };
    use verter_type_runtime::ProviderFuture;

    /// A hermetic in-process provider that records file operations and returns
    /// canned hover/diagnostics — lets the bridge dispatch be tested without a
    /// live tsgo/tsserver.
    #[derive(Default)]
    struct MockProvider {
        log: Mutex<Vec<String>>,
        hover: Option<HoverInfo>,
        /// Byte offsets the bridge fed to `get_hover`, in call order — lets a
        /// test assert the bridge passes the offset through verbatim.
        hover_offsets: Mutex<Vec<u32>>,
        /// When `true`, every apply (`open`/`load`/`update_file`) returns an
        /// error — lets a test force a provider failure mid-sync. Query methods
        /// still succeed, so a probe is gated by the overlay, not the apply.
        fail_apply: bool,
    }

    impl MockProvider {
        fn record(&self, entry: String) {
            self.log.lock().unwrap().push(entry);
        }
        fn calls(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
        fn hover_offsets(&self) -> Vec<u32> {
            self.hover_offsets.lock().unwrap().clone()
        }
        fn apply_outcome(&self) -> Result<(), TypeProviderError> {
            if self.fail_apply {
                Err(TypeProviderError::new("mock apply failure"))
            } else {
                Ok(())
            }
        }
    }

    macro_rules! ready {
        ($v:expr) => {
            Box::pin(async move { Ok($v) })
        };
    }

    impl TypeProvider for MockProvider {
        fn open_file(&self, path: &str, _c: &str) -> ProviderFuture<'_, ()> {
            self.record(format!("open:{path}"));
            let r = self.apply_outcome();
            Box::pin(async move { r })
        }
        fn load_file(&self, path: &str, _c: &str) -> ProviderFuture<'_, ()> {
            self.record(format!("load:{path}"));
            let r = self.apply_outcome();
            Box::pin(async move { r })
        }
        fn update_file(&self, path: &str, _c: &str) -> ProviderFuture<'_, ()> {
            self.record(format!("update:{path}"));
            let r = self.apply_outcome();
            Box::pin(async move { r })
        }
        fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
            self.record(format!("close:{path}"));
            ready!(())
        }
        fn get_completions(
            &self,
            _p: &str,
            _o: u32,
            _t: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            ready!(CompletionResult {
                items: vec![],
                is_incomplete: false
            })
        }
        fn get_hover(&self, _p: &str, o: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
            self.hover_offsets.lock().unwrap().push(o);
            let h = self.hover.clone();
            ready!(h)
        }
        fn get_diagnostics(&self, _p: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            ready!(vec![])
        }
        fn get_definition(&self, _p: &str, _o: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
            ready!(vec![])
        }
        fn get_type_definition(&self, _p: &str, _o: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
            ready!(vec![])
        }
        fn get_references(&self, _p: &str, _o: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
            ready!(vec![])
        }
        fn get_rename_locations(
            &self,
            _p: &str,
            _o: u32,
        ) -> ProviderFuture<'_, Vec<RenameLocation>> {
            ready!(vec![])
        }
        fn get_signature_help(
            &self,
            _p: &str,
            _o: u32,
        ) -> ProviderFuture<'_, Option<SignatureHelp>> {
            ready!(None)
        }
        fn get_code_actions(
            &self,
            _p: &str,
            _s: u32,
            _e: u32,
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            ready!(vec![])
        }
        fn get_semantic_tokens(&self, _p: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
            ready!(vec![])
        }
        fn get_document_highlights(
            &self,
            _p: &str,
            _o: u32,
        ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
            ready!(vec![])
        }
        fn get_inlay_hints(
            &self,
            _p: &str,
            _s: u32,
            _e: u32,
        ) -> ProviderFuture<'_, Vec<InlayHint>> {
            ready!(vec![])
        }
    }

    fn ready_bridge(mock: Arc<MockProvider>) -> Bridge {
        let mut b = Bridge::new();
        b.ready = Some(ReadyProvider {
            provider: mock,
            capabilities: ProviderCapabilities::for_provider(ProviderName::Tsgo),
        });
        b
    }

    fn entry(path: &str) -> BaselineFile {
        BaselineFile {
            path: path.to_string(),
            content: "x".to_string(),
            role: crate::protocol::FileRole::Entry,
            source_map_identity: None,
        }
    }

    fn entry_with_map(path: &str, map: Option<&str>) -> BaselineFile {
        BaselineFile {
            path: path.to_string(),
            content: "x".to_string(),
            role: crate::protocol::FileRole::Entry,
            source_map_identity: map.map(String::from),
        }
    }

    fn api(path: &str) -> BaselineFile {
        BaselineFile {
            path: path.to_string(),
            content: "x".to_string(),
            role: crate::protocol::FileRole::Api,
            source_map_identity: None,
        }
    }

    #[tokio::test]
    async fn probe_before_hello_is_not_initialized() {
        let mut b = Bridge::new();
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "file:///A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 1,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => assert_eq!(e.kind, ErrorKind::NotInitialized),
            other => panic!("expected NotInitialized, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn query_at_version_below_overlay_is_refused_as_stale() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));

        // open at v1.
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx")],
            version: 1,
        })
        .await;

        // A probe for the SAME uri at v2 is refused: the bridge only has v1
        // artifacts — never compare verter@editN with baseline@edit0.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                assert_eq!(e.requested_version, Some(2));
                assert_eq!(e.have_version, Some(1));
            }
            other => panic!("expected stale refusal, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 0, "a refused probe must not count as ran");
    }

    #[tokio::test]
    async fn sync_artifacts_applies_update_file_then_probe_is_fresh() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));

        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx")],
            version: 1,
        })
        .await;

        // Edit -> v2 -> syncArtifacts. The already-open entry must be applied
        // through TypeProvider::update_file.
        let sync = b
            .on_sync(SyncArtifactsRequest {
                uri: "/A.vue".to_string(),
                version: 2,
                files: vec![entry("/A.vue.tsx")],
                source_map_identity: Some("map-2".to_string()),
                changed_public_api_twins: vec![],
            })
            .await;
        match sync {
            Response::SyncArtifacts(s) => {
                assert_eq!(s.applied[0].action, SyncAction::Updated);
            }
            other => panic!("expected syncArtifacts, got {other:?}"),
        }
        assert!(
            mock.calls().contains(&"update:/A.vue.tsx".to_string()),
            "syncArtifacts must call TypeProvider::update_file; calls={:?}",
            mock.calls()
        );

        // Now a probe at v2 is fresh and runs against the provider.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Query(q) => match q.result {
                QueryResult::Hover { hover } => {
                    assert_eq!(hover.unwrap().contents, "const x: string")
                }
                other => panic!("expected hover result, got {other:?}"),
            },
            other => panic!("expected query response, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 1, "the fresh probe counts as ran");

        // shutdown reports the authoritative baseline-ran count (> 0).
        match b.on_shutdown().await {
            Response::Shutdown(s) => assert_eq!(s.baseline_ran, 1),
            other => panic!("expected shutdown, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sync_provider_failure_does_not_advance_overlay_then_probe_is_stale() {
        // A provider that rejects every apply (open/load/update).
        let mock = Arc::new(MockProvider {
            fail_apply: true,
            // Query still resolves, proving the later refusal is the overlay
            // gate, not the failing provider.
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));

        // Sync A at v2; the provider rejects the apply, so the bridge errors AND
        // must not stamp v2 as fresh.
        let resp = b
            .on_sync(SyncArtifactsRequest {
                uri: "/A.vue".to_string(),
                version: 2,
                files: vec![entry("/A.vue.tsx")],
                source_map_identity: Some("map-2".to_string()),
                changed_public_api_twins: vec![],
            })
            .await;
        match resp {
            Response::Error(e) => assert_eq!(e.kind, ErrorKind::ProviderError),
            other => panic!("expected provider error, got {other:?}"),
        }

        // The overlay did NOT advance: a probe at v2 is refused stale with NO
        // recorded version — the failed sync never marked v2 fresh.
        let probe = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match probe {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                // Negative: not Fresh, and nothing was ever stamped for this URI.
                assert_eq!(e.have_version, None);
            }
            other => panic!("expected stale refusal, got {other:?}"),
        }
        // A failed apply (and the refused probe) never counts as a baseline run.
        assert_eq!(b.baseline_ran, 0);
        // Source-map state likewise did not advance from the failed sync.
        assert!(!b.overlay.source_map_present("/A.vue"));
    }

    #[tokio::test]
    async fn empty_sync_does_not_mark_authored_uri_fresh() {
        // syncArtifacts with an EMPTY payload at vN must NOT mark the authored
        // URI fresh: nothing for that URI was applied, so a later vN probe still
        // holds edit-0 content and must be refused stale.
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx")],
            version: 1,
        })
        .await;
        // Edit -> v2 -> sync, but the payload carries NO artifact for A.
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 2,
            files: vec![],
            source_map_identity: None,
            changed_public_api_twins: vec![],
        })
        .await;
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                // The overlay still holds only the v1 open artifact for A.
                assert_eq!(e.have_version, Some(1));
            }
            other => panic!("expected stale refusal after empty sync, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 0, "a refused probe must not count as ran");
    }

    #[tokio::test]
    async fn sibling_only_sync_does_not_mark_authored_uri_fresh() {
        // syncArtifacts whose payload carries only a SIBLING artifact must not
        // mark the queried (un-applied) authored URI fresh.
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx")],
            version: 1,
        })
        .await;
        // Sync names uri=A at v2 but pushes only B's artifact (named as a twin).
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 2,
            files: vec![entry("/B.vue.tsx")],
            source_map_identity: None,
            changed_public_api_twins: vec![crate::protocol::ChangedTwin {
                path: "/B.vue.tsx".to_string(),
                version: 2,
            }],
        })
        .await;
        // A probe for A at v2 is refused: A's own artifact was never applied.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                assert_eq!(e.have_version, Some(1));
            }
            other => panic!("expected stale refusal for un-applied A, got {other:?}"),
        }
        // Guard: the sibling B that WAS applied (and named) is fresh at v2.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/B.vue".to_string(),
                path: "/B.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        assert!(
            matches!(resp, Response::Query(_)),
            "the applied+named sibling must be fresh: {resp:?}"
        );
    }

    #[tokio::test]
    async fn named_twin_absent_from_synced_files_is_not_marked_fresh() {
        // A sync naming a changed twin in `changedPublicApiTwins` but carrying NO
        // files (files: []) must NOT mark that twin fresh — its generated file was
        // never applied to the provider, so a probe at its named version still
        // hits pre-edit content and must be refused stale.
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_sync(SyncArtifactsRequest {
            uri: "/Parent.vue".to_string(),
            version: 5,
            files: vec![],
            source_map_identity: None,
            changed_public_api_twins: vec![crate::protocol::ChangedTwin {
                path: "/Child.vue.ts".to_string(),
                version: 4,
            }],
        })
        .await;
        // The named-but-unapplied twin is refused stale with no recorded version.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/Child.vue".to_string(),
                path: "/Child.vue.ts".to_string(),
                offset: 0,
                version: 4,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                assert_eq!(e.have_version, None);
            }
            other => panic!("expected stale refusal for the unapplied twin, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 0, "a refused probe must not count as ran");
    }

    // ── the staleness gate is path-precise, not authored-URI-coarse ──────────
    //
    // Two artifacts of ONE authored document — the `.vue.tsx` entry and the
    // `.vue.ts` api twin — can advance at different versions. A sync that
    // refreshes only the twin must NOT clear a probe for the still-stale entry:
    // the entry path still holds the pre-edit content, so running a probe on it
    // would compare baseline@edit0 with verter@editN — exactly the cross-version
    // comparison the overlay exists to prevent. An authored-URI-coarse gate
    // wrongly passes the entry probe because the URI is "fresh" via the twin.

    #[tokio::test]
    async fn entry_probe_refused_when_only_twin_synced_at_new_version() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        // Open BOTH the entry and its api twin at v1.
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx"), api("/A.vue.ts")],
            version: 1,
        })
        .await;
        // Edit -> v2 -> sync carrying ONLY the twin (.vue.ts). The entry
        // (.vue.tsx) is absent, so the provider still holds its v1 content.
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 2,
            files: vec![api("/A.vue.ts")],
            source_map_identity: None,
            changed_public_api_twins: vec![],
        })
        .await;
        // A v2 hover on the STALE entry path must be REFUSED: the entry path was
        // not itself synced at v2 (it stands at the v1 open). Under a URI-coarse
        // gate this wrongly passes because /A.vue is "fresh at v2" via the twin.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                // The entry path itself only advanced to v1 (the initial open).
                assert_eq!(e.have_version, Some(1));
            }
            other => panic!("stale entry probe must be refused, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 0, "a refused probe must not count as ran");

        // The twin path, which WAS synced at v2, is allowed at v2.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.ts".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        assert!(
            matches!(resp, Response::Query(_)),
            "the twin synced at v2 must be allowed: {resp:?}"
        );
        assert_eq!(b.baseline_ran, 1);
    }

    #[tokio::test]
    async fn entry_probe_allowed_when_entry_itself_synced_at_new_version() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx"), api("/A.vue.ts")],
            version: 1,
        })
        .await;
        // Sync at v2 carrying the ENTRY itself → a v2 probe on the entry runs.
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 2,
            files: vec![entry("/A.vue.tsx")],
            source_map_identity: None,
            changed_public_api_twins: vec![],
        })
        .await;
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        assert!(
            matches!(resp, Response::Query(_)),
            "the entry synced at v2 must be allowed: {resp:?}"
        );
        assert_eq!(b.baseline_ran, 1);
        // Negative + path-precision: the twin was NOT synced at v2 (it stands at
        // the v1 open), so a v2 probe on the twin is refused even though the
        // entry advanced the shared URI to v2.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.ts".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                assert_eq!(e.have_version, Some(1));
            }
            other => panic!("the un-synced twin must be refused at v2, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn diagnostics_entry_probe_refused_when_only_twin_synced_at_new_version() {
        // The symmetric diagnostics-probe case: on_diagnostics applies the same
        // path-precise staleness gate as on_query.
        let mock = Arc::new(MockProvider::default());
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx"), api("/A.vue.ts")],
            version: 1,
        })
        .await;
        // Sync carrying ONLY the twin at v2.
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 2,
            files: vec![api("/A.vue.ts")],
            source_map_identity: None,
            changed_public_api_twins: vec![],
        })
        .await;
        // A v2 diagnostics probe on the stale entry path is refused (path-precise).
        let resp = b
            .on_diagnostics(DiagnosticsRequest {
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                version: 2,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Error(e) => {
                assert_eq!(e.kind, ErrorKind::BaselineArtifactStale);
                assert_eq!(e.have_version, Some(1));
            }
            other => panic!("stale entry diagnostics must be refused, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 0, "a refused probe must not count as ran");
        // The twin path synced at v2 is allowed.
        let resp = b
            .on_diagnostics(DiagnosticsRequest {
                uri: "/A.vue".to_string(),
                path: "/A.vue.ts".to_string(),
                version: 2,
                requires_source_map: false,
            })
            .await;
        assert!(
            matches!(resp, Response::Diagnostics(_)),
            "the twin synced at v2 must be allowed: {resp:?}"
        );
        assert_eq!(b.baseline_ran, 1);
    }

    #[tokio::test]
    async fn open_loads_support_files_and_opens_entries() {
        let mock = Arc::new(MockProvider::default());
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![
                entry("/A.vue.tsx"),
                BaselineFile {
                    path: "/A.vue.ts".to_string(),
                    content: "y".to_string(),
                    role: crate::protocol::FileRole::Api,
                    source_map_identity: None,
                },
                BaselineFile {
                    path: "/node_modules/vue/index.d.ts".to_string(),
                    content: "z".to_string(),
                    role: crate::protocol::FileRole::Support,
                    source_map_identity: None,
                },
            ],
            version: 1,
        })
        .await;
        let calls = mock.calls();
        assert!(calls.contains(&"open:/A.vue.tsx".to_string()), "{calls:?}");
        assert!(calls.contains(&"load:/A.vue.ts".to_string()), "{calls:?}");
        assert!(
            calls.contains(&"load:/node_modules/vue/index.d.ts".to_string()),
            "{calls:?}"
        );
    }

    #[tokio::test]
    async fn non_strict_hello_skips_missing_tsserver_tool_root_with_reason() {
        // Non-strict + tsserver provider + empty tool root => skip, recorded
        // reason, no provider — deterministic (field check precedes discovery).
        let mut b = Bridge::new();
        let resp = b
            .on_hello(HelloRequest {
                workspace_root: "/ws".to_string(),
                repo_root: "/repo".to_string(),
                provider: ProviderName::Tsserver,
                strict_ci: false,
                tool_root: crate::protocol::ToolRoot::default(),
            })
            .await;
        match resp {
            Response::Hello(h) => {
                assert!(h.skipped);
                assert!(h.skip_reason.unwrap().contains("tsserver"));
                assert!(h.baseline_tool_root_used.is_none());
            }
            other => panic!("expected skipped hello, got {other:?}"),
        }
        assert!(b.skipped);
        // A probe after a skip is refused, not silently passed.
        let q = b
            .on_diagnostics(DiagnosticsRequest {
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                version: 1,
                requires_source_map: false,
            })
            .await;
        assert!(matches!(q, Response::Error(_)));
    }

    #[tokio::test]
    async fn strict_hello_missing_tsserver_tool_root_is_hard_error() {
        let mut b = Bridge::new();
        let resp = b
            .on_hello(HelloRequest {
                workspace_root: "/ws".to_string(),
                repo_root: "/repo".to_string(),
                provider: ProviderName::Tsserver,
                strict_ci: true,
                tool_root: crate::protocol::ToolRoot::default(),
            })
            .await;
        match resp {
            Response::Error(e) => assert_eq!(e.kind, ErrorKind::BaselineToolRootMissing),
            other => panic!("expected hard error, got {other:?}"),
        }
    }

    // ── known-good `<script setup>` hover parity gate ────────────────────────

    /// What an absent tsgo means for a tsgo-gated test: a hard failure when the
    /// run requires tsgo (`DX_REQUIRE_TSGO=1`, e.g. strict CI), else an explicit
    /// recorded skip. Pure, so both branches are unit-tested regardless of
    /// whether tsgo happens to be installed on the running machine.
    #[derive(Debug, PartialEq, Eq)]
    enum TsgoAbsence {
        HardFail,
        SkipWithReason,
    }

    fn tsgo_absence_outcome(require_tsgo: bool) -> TsgoAbsence {
        if require_tsgo {
            TsgoAbsence::HardFail
        } else {
            TsgoAbsence::SkipWithReason
        }
    }

    fn require_tsgo() -> bool {
        std::env::var("DX_REQUIRE_TSGO")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// A structured, machine-detectable record that a tsgo-gated parity check was
    /// SKIPPED — never silently passed. It is emitted on stderr behind a stable
    /// marker so the DX harness can scan provider-test output, detect a skipped
    /// gate, and refuse to count it as a real pass. In strict CI an absent tsgo
    /// is a HARD failure, so a skip record is only ever produced in a non-strict
    /// run (`requires_tsgo` is therefore always `false`).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct GateSkipRecord {
        /// The gated test that was skipped.
        gate: String,
        /// Always `true` — a skip is never a pass.
        skipped: bool,
        /// Why the gate could not run (e.g. tsgo not discoverable).
        reason: String,
        /// Whether the run required tsgo. A skip is only produced when this is
        /// `false`; a strict (`requires_tsgo == true`) run hard-fails instead.
        requires_tsgo: bool,
    }

    /// Stable stderr marker prefixing a [`GateSkipRecord`] JSON payload.
    const GATE_SKIP_MARKER: &str = "DX_GATE_SKIP ";

    /// Build the structured skip record for a non-strict gate that could not run.
    fn build_gate_skip_record(gate: &str, reason: &str) -> GateSkipRecord {
        GateSkipRecord {
            gate: gate.to_string(),
            skipped: true,
            reason: reason.to_string(),
            requires_tsgo: false,
        }
    }

    /// Serialize a skip record into its machine-parseable marker line
    /// (`DX_GATE_SKIP {json}`).
    fn gate_skip_marker_line(rec: &GateSkipRecord) -> String {
        format!(
            "{GATE_SKIP_MARKER}{}",
            serde_json::to_string(rec).unwrap_or_default()
        )
    }

    /// Record a non-strict gate skip: emit the structured marker line to stderr
    /// and return the typed record. This replaces a freeform `SKIP …` print so a
    /// skipped gate is detectable by the harness, never a vacuous pass.
    fn record_gate_skip(gate: &str, reason: &str) -> GateSkipRecord {
        let rec = build_gate_skip_record(gate, reason);
        eprintln!("{}", gate_skip_marker_line(&rec));
        rec
    }

    /// Discover tsgo for a gated test, or apply [`tsgo_absence_outcome`]: a hard
    /// failure under `DX_REQUIRE_TSGO=1`, else a structured, recorded skip
    /// returning `None` so the caller returns early. A skip is never reported as
    /// success — [`record_gate_skip`] emits a typed marker the harness detects.
    fn find_tsgo_for_gate(test_name: &str) -> Option<String> {
        match verter_type_runtime::tsgo::find_tsgo_binary() {
            Ok(bin) => Some(bin),
            Err(e) => {
                let reason = format!("tsgo not discoverable: {e:?}");
                match tsgo_absence_outcome(require_tsgo()) {
                    TsgoAbsence::HardFail => {
                        panic!("DX_REQUIRE_TSGO=1 but {test_name} cannot run: {reason}")
                    }
                    TsgoAbsence::SkipWithReason => {
                        record_gate_skip(test_name, &reason);
                        None
                    }
                }
            }
        }
    }

    #[test]
    fn tsgo_absent_is_hard_fail_when_required_else_recorded_skip() {
        // The strict hook is non-vacuous: requiring tsgo turns an absent tsgo
        // into a HARD failure, so a tsgo-absent CI run can never report the gate
        // green by skipping.
        assert_eq!(tsgo_absence_outcome(true), TsgoAbsence::HardFail);
        // The default (non-strict) is an explicit recorded skip, never a silent
        // pass.
        assert_eq!(tsgo_absence_outcome(false), TsgoAbsence::SkipWithReason);
    }

    #[test]
    fn gate_skip_outcome_is_structured_and_detectable_not_vacuous() {
        // A non-strict gate that cannot run produces a TYPED, serializable skip
        // record — not a freeform print and not a vacuous pass.
        let rec = build_gate_skip_record(
            "known_good_script_setup_hover_resolves_through_tsgo_on_emitted_tsx",
            "tsgo not discoverable: NotFound",
        );
        assert!(rec.skipped, "a skip record must mark skipped=true");
        assert!(
            !rec.requires_tsgo,
            "a skip is only produced in a non-strict run"
        );
        assert!(rec.reason.contains("tsgo not discoverable"));
        assert!(rec.gate.contains("known_good_script_setup_hover"));

        // It serializes to a stable, machine-parseable marker line the harness
        // can scan for: `DX_GATE_SKIP {json}` with the JSON carrying skipped=true.
        let line = gate_skip_marker_line(&rec);
        assert!(line.starts_with(GATE_SKIP_MARKER), "marker prefix: {line}");
        let payload = line.strip_prefix(GATE_SKIP_MARKER).expect("prefix present");
        let parsed: serde_json::Value =
            serde_json::from_str(payload).expect("marker payload is valid JSON");
        assert_eq!(parsed["skipped"], serde_json::json!(true));
        assert_eq!(parsed["requiresTsgo"], serde_json::json!(false));
        assert_eq!(parsed["gate"], serde_json::json!(rec.gate));
        // Negative: NOT the old freeform `SKIP {name}: {reason}` line — that
        // started with `SKIP `; the structured line starts with the marker.
        assert!(
            !line.starts_with("SKIP "),
            "skip output must be the structured marker, not a freeform SKIP line: {line}"
        );
    }

    /// Reproduce ONE known-good `<script setup>` hover, at a non-template,
    /// non-synthetic source position, on verter's emitted `.vue.tsx` driven
    /// through a real tsgo, and assert the known-good `string` type.
    ///
    /// Contract boundary: this crate owns ONLY the tsgo-standalone-on-emitted-TSX
    /// HALF of the known-good parity check — it materializes the `.vue.tsx`
    /// through the public host and proves a real tsgo returns the known-good type
    /// at the authored script-setup const. It deliberately does NOT spawn
    /// `verter-lsp`. The opposing verter@tsgo half needs verter's own hover (the
    /// `verter_lsp` merge of template/script position mapping with provider
    /// results), which is not a public API to this crate, and the cross-tool
    /// identity comparison that pairs the two halves is owned by the DX harness
    /// integration gate (a later harness component), not reproduced here.
    ///
    /// Non-vacuity: when tsgo is absent the gate is a HARD failure under
    /// `DX_REQUIRE_TSGO=1` (strict CI) and otherwise a structured recorded skip —
    /// never a silent green.
    #[tokio::test]
    async fn known_good_script_setup_hover_resolves_through_tsgo_on_emitted_tsx() {
        use crate::materialize::{materialize, MaterializeRequest};
        use crate::protocol::ToolRoot;
        use crate::provider::{resolve, spawn};

        let Some(tsgo_bin) = find_tsgo_for_gate(
            "known_good_script_setup_hover_resolves_through_tsgo_on_emitted_tsx",
        ) else {
            return;
        };

        // Self-contained hermetic fixture: a typed `<script setup>` const with a
        // hover anchor. All-ASCII so the byte offset equals tsgo's UTF-16 offset.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let entry = root.join("Hello.vue");
        std::fs::write(
            &entry,
            "<script setup lang=\"ts\">\nconst greeting: string = 'hello world'\n</script>\n<template><div>{{ greeting }}</div></template>\n",
        )
        .expect("write fixture");

        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![entry.clone()],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .expect("materialize fixture");

        let tsx = report
            .ide_artifacts
            .iter()
            .find(|a| a.generated_path.ends_with("Hello.vue.tsx"))
            .expect("emitted Hello.vue.tsx");

        // Non-template, non-synthetic position: the authored `greeting` token in
        // the script-setup const declaration, emitted verbatim into the TSX.
        let decl = tsx
            .content
            .find("const greeting")
            .expect("script-setup const emitted into TSX");
        let offset = (decl + "const ".len()) as u32;

        // Resolve + spawn a real tsgo against the materialized root (strict: an
        // explicit existing bin must resolve Ready).
        let tool_root = ToolRoot {
            tsgo_bin: Some(tsgo_bin),
            ..ToolRoot::default()
        };
        let root_str = root.to_string_lossy().to_string();
        let plan = match resolve(ProviderName::Tsgo, &tool_root, &root_str, true)
            .expect("tsgo resolves with an explicit bin")
        {
            Resolution::Ready { plan, .. } => plan,
            Resolution::Skipped { reason } => panic!("tsgo unexpectedly skipped: {reason}"),
        };
        let provider = spawn(ProviderName::Tsgo, &plan, &root_str)
            .await
            .expect("spawn tsgo");

        let tsx_path = tsx.generated_path.to_string_lossy().to_string();
        provider
            .open_file(&tsx_path, &tsx.content)
            .await
            .expect("open emitted tsx in tsgo");
        let hover = provider
            .get_hover(&tsx_path, offset)
            .await
            .expect("tsgo hover");
        let _ = provider.shutdown().await;

        let hover = hover.expect("tsgo returns a hover for the script-setup const");
        assert!(
            hover.contents.contains("string"),
            "known-good hover must report the `string` type; got: {}",
            hover.contents
        );
        // Negative: the explicitly-typed const must not degrade to `any`.
        assert!(
            !hover.contents.contains(": any"),
            "script-setup const must not degrade to any; got: {}",
            hover.contents
        );
    }

    /// Provider-level proof of the barrel rewrite: a real tsgo resolves a
    /// reexport THROUGH the rewritten `.vue.ts` twin, and fails to resolve the
    /// raw `./Child.vue` specifier — so the rewrite is load-bearing.
    #[tokio::test]
    async fn provider_resolves_barrel_reexport_through_rewritten_twin() {
        use crate::materialize::{materialize, MaterializeRequest};
        use crate::protocol::ToolRoot;
        use crate::provider::{resolve, spawn};

        let Some(tsgo_bin) =
            find_tsgo_for_gate("provider_resolves_barrel_reexport_through_rewritten_twin")
        else {
            return;
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(
            root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label: string = 'child'\n</script>\n<template><span>{{ label }}</span></template>\n",
        )
        .expect("write child");
        // The fixture barrel reexports the child by its raw `./Child.vue` path;
        // materialization must rewrite it to the `.vue.ts` twin.
        std::fs::write(
            root.join("barrel.ts"),
            "export { default as Child } from './Child.vue'\n",
        )
        .expect("write barrel");
        // A control barrel that is NOT rewritten (kept raw on disk).
        std::fs::write(
            root.join("raw_barrel.txt"),
            "export { default as Child } from './Child.vue'\n",
        )
        .expect("write raw control");

        let _report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![root.join("Child.vue")],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .expect("materialize");

        let tool_root = ToolRoot {
            tsgo_bin: Some(tsgo_bin),
            ..ToolRoot::default()
        };
        let root_str = root.to_string_lossy().to_string();
        let plan = match resolve(ProviderName::Tsgo, &tool_root, &root_str, true)
            .expect("tsgo resolves with an explicit bin")
        {
            Resolution::Ready { plan, .. } => plan,
            Resolution::Skipped { reason } => panic!("tsgo unexpectedly skipped: {reason}"),
        };
        let provider = spawn(ProviderName::Tsgo, &plan, &root_str)
            .await
            .expect("spawn tsgo");

        // The materialized barrel is rewritten to `./Child.vue.ts` on disk.
        let good_barrel = root.join("barrel.ts");
        let good_src = std::fs::read_to_string(&good_barrel).expect("read barrel");
        assert!(
            good_src.contains("./Child.vue.ts"),
            "materialized barrel must be rewritten to the twin: {good_src:?}"
        );
        assert!(
            !good_src.contains("./Child.vue'"),
            "raw .vue reexport must not survive: {good_src:?}"
        );

        // The raw control still points at `./Child.vue` — write it as a sibling
        // `.ts` so tsgo will try to resolve the unrewritten specifier.
        let raw_barrel = root.join("raw_barrel.ts");
        std::fs::write(
            &raw_barrel,
            "export { default as Child } from './Child.vue'\n",
        )
        .expect("write raw barrel ts");

        let good_path = good_barrel.to_string_lossy().to_string();
        let raw_path = raw_barrel.to_string_lossy().to_string();
        provider
            .open_file(&good_path, &good_src)
            .await
            .expect("open good barrel");
        provider
            .open_file(
                &raw_path,
                "export { default as Child } from './Child.vue'\n",
            )
            .await
            .expect("open raw barrel");

        let good_diags = provider
            .get_diagnostics(&good_path)
            .await
            .expect("good diags");
        let raw_diags = provider
            .get_diagnostics(&raw_path)
            .await
            .expect("raw diags");
        let _ = provider.shutdown().await;

        // Scoped to the barrel's OWN `./Child.vue` specifier — ignore unrelated
        // module resolution (e.g. a `vue` import inside the twin), so the test
        // isolates exactly the barrel-rewrite effect.
        let cannot_find_raw_child = |d: &TypeDiagnostic| {
            let is_unresolved =
                d.code.as_deref() == Some("2307") || d.message.contains("Cannot find module");
            is_unresolved && d.message.contains("Child.vue") && !d.message.contains("Child.vue.ts")
        };
        // The rewritten barrel resolves the reexport through the twin — no
        // unresolved `./Child.vue` specifier.
        assert!(
            !good_diags.iter().any(cannot_find_raw_child),
            "rewritten barrel must resolve through the twin; diags: {good_diags:?}"
        );
        // Negative/control: the raw `./Child.vue` specifier does NOT resolve, so
        // the rewrite is what makes the barrel resolvable.
        assert!(
            raw_diags.iter().any(cannot_find_raw_child),
            "raw .vue reexport must fail to resolve; diags: {raw_diags:?}"
        );
    }

    // ── the seam feeds byte offsets, not utf-16 ──────────────────────────────

    #[tokio::test]
    async fn query_feeds_byte_offset_to_provider_not_utf16() {
        // A multi-byte char (`é`, 2 UTF-8 bytes / 1 UTF-16 code unit) before the
        // queried token makes the byte offset differ from the utf-16 offset.
        let content = "const café = 1; const greeting = 'x'";
        let byte_offset = content.find("greeting").expect("token present") as u32;
        let utf16_offset = content[..byte_offset as usize].encode_utf16().count() as u32;
        assert_ne!(
            byte_offset, utf16_offset,
            "fixture must distinguish byte vs utf-16 offsets"
        );

        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const greeting: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx")],
            version: 1,
        })
        .await;

        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: byte_offset,
                version: 1,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Query(q) => {
                assert_eq!(q.capabilities.position_encoding, "utf-8");
                assert_ne!(q.capabilities.position_encoding, "utf-16");
            }
            other => panic!("expected query response, got {other:?}"),
        }
        // The bridge fed the provider the BYTE offset verbatim — no utf-16
        // conversion crosses this seam.
        assert_eq!(mock.hover_offsets(), vec![byte_offset]);
        // Negative: it did NOT feed the utf-16 code-unit offset.
        assert_ne!(mock.hover_offsets(), vec![utf16_offset]);
    }

    // ── file:// URI and generated path reconcile to one key ──────────────────

    #[tokio::test]
    async fn probe_by_file_uri_matches_open_by_generated_path() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        // Open by generated PATH (no file:// scheme) at v1.
        b.on_open(OpenRequest {
            files: vec![entry("/abs/Foo.vue.tsx")],
            version: 1,
        })
        .await;
        // Probe by the file:// AUTHORED URI at v1 → must be FRESH (it ran), not a
        // false `baseline_artifact_stale { have: None }`.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "file:///abs/Foo.vue".to_string(),
                path: "/abs/Foo.vue.tsx".to_string(),
                offset: 0,
                version: 1,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        match resp {
            Response::Query(_) => {}
            Response::Error(e) => {
                panic!("file:// probe wrongly refused: {:?} {}", e.kind, e.message)
            }
            other => panic!("expected query, got {other:?}"),
        }
        assert_eq!(b.baseline_ran, 1);
    }

    // ── requiresSourceMap refuses when the map is absent ─────────────────────

    #[tokio::test]
    async fn requires_source_map_probe_refused_when_map_absent_else_runs() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        b.on_open(OpenRequest {
            files: vec![entry("/A.vue.tsx")],
            version: 1,
        })
        .await;

        // Sync at v2 with NO source map (map-absent).
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 2,
            files: vec![entry("/A.vue.tsx")],
            source_map_identity: None,
            changed_public_api_twins: vec![],
        })
        .await;

        // A requiresSourceMap probe at v2 is refused `compiled_code_map_absent`.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 2,
                trigger_character: None,
                requires_source_map: true,
            })
            .await;
        match resp {
            Response::Error(e) => assert_eq!(e.kind, ErrorKind::CompiledCodeMapAbsent),
            other => panic!("expected map-absent refusal, got {other:?}"),
        }
        assert_eq!(
            b.baseline_ran, 0,
            "a map-absent refusal must not count as ran"
        );

        // Sync at v3 WITH a source map present → the same probe now runs.
        b.on_sync(SyncArtifactsRequest {
            uri: "/A.vue".to_string(),
            version: 3,
            files: vec![entry("/A.vue.tsx")],
            source_map_identity: Some("map-3".to_string()),
            changed_public_api_twins: vec![],
        })
        .await;
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 3,
                trigger_character: None,
                requires_source_map: true,
            })
            .await;
        assert!(
            matches!(resp, Response::Query(_)),
            "map present → runs: {resp:?}"
        );
        assert_eq!(b.baseline_ran, 1);

        // Negative: a `requires_source_map: false` probe is never map-absent-refused.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 3,
                trigger_character: None,
                requires_source_map: false,
            })
            .await;
        assert!(matches!(resp, Response::Query(_)));
    }

    // ── edit-0 requiresSourceMap is not falsely refused ──────────────────────

    #[tokio::test]
    async fn open_records_source_map_so_edit0_requires_source_map_succeeds() {
        let mock = Arc::new(MockProvider {
            hover: Some(HoverInfo {
                contents: "const x: string".to_string(),
                range_start: None,
                range_end: None,
            }),
            ..Default::default()
        });
        let mut b = ready_bridge(Arc::clone(&mock));
        // Open a map-HAVING entry AND a genuinely map-absent entry at edit-0.
        b.on_open(OpenRequest {
            files: vec![
                entry_with_map("/A.vue.tsx", Some("map-0")),
                entry_with_map("/B.vue.tsx", None),
            ],
            version: 1,
        })
        .await;

        // The map-having artifact's requiresSourceMap probe at v1 SUCCEEDS — the
        // initial materialized artifact DOES have a map, so it is not refused.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                offset: 0,
                version: 1,
                trigger_character: None,
                requires_source_map: true,
            })
            .await;
        assert!(
            matches!(resp, Response::Query(_)),
            "edit-0 map-having artifact must not be falsely refused: {resp:?}"
        );

        // Negative: a genuinely map-absent entry is still refused.
        let resp = b
            .on_query(QueryRequest {
                method: QueryMethod::Hover,
                uri: "/B.vue".to_string(),
                path: "/B.vue.tsx".to_string(),
                offset: 0,
                version: 1,
                trigger_character: None,
                requires_source_map: true,
            })
            .await;
        match resp {
            Response::Error(e) => assert_eq!(e.kind, ErrorKind::CompiledCodeMapAbsent),
            other => panic!("map-absent entry must still be refused, got {other:?}"),
        }
    }

    // ── diagnostics applies the same map-presence gate as query ──────────────

    #[tokio::test]
    async fn diagnostics_requires_source_map_gate_matches_query() {
        let mock = Arc::new(MockProvider::default());
        let mut b = ready_bridge(Arc::clone(&mock));
        // A (has map) and B (no map), both opened at v1.
        b.on_open(OpenRequest {
            files: vec![
                entry_with_map("/A.vue.tsx", Some("map-0")),
                entry_with_map("/B.vue.tsx", None),
            ],
            version: 1,
        })
        .await;

        // requiresSourceMap diagnostics vs the map-ABSENT artifact → refused.
        let resp = b
            .on_diagnostics(DiagnosticsRequest {
                uri: "/B.vue".to_string(),
                path: "/B.vue.tsx".to_string(),
                version: 1,
                requires_source_map: true,
            })
            .await;
        match resp {
            Response::Error(e) => assert_eq!(e.kind, ErrorKind::CompiledCodeMapAbsent),
            other => panic!("map-absent diagnostics must be refused, got {other:?}"),
        }
        assert_eq!(
            b.baseline_ran, 0,
            "a map-absent refusal must not count as ran"
        );

        // requiresSourceMap diagnostics vs the map-PRESENT artifact → runs.
        let resp = b
            .on_diagnostics(DiagnosticsRequest {
                uri: "/A.vue".to_string(),
                path: "/A.vue.tsx".to_string(),
                version: 1,
                requires_source_map: true,
            })
            .await;
        assert!(
            matches!(resp, Response::Diagnostics(_)),
            "map-present diagnostics must run: {resp:?}"
        );
        assert_eq!(b.baseline_ran, 1);

        // Negative: a requires_source_map:false diagnostics is never map-refused.
        let resp = b
            .on_diagnostics(DiagnosticsRequest {
                uri: "/B.vue".to_string(),
                path: "/B.vue.tsx".to_string(),
                version: 1,
                requires_source_map: false,
            })
            .await;
        assert!(matches!(resp, Response::Diagnostics(_)));
    }

    // ── stdin read error emits an explicit frame, not a silent exit ──────────

    #[tokio::test]
    async fn dispatch_loop_emits_explicit_frame_on_stdin_read_error() {
        // Invalid UTF-8 on stdin makes `next_line()` return an Err; the loop must
        // emit an explicit reason frame, not terminate silently.
        let input: &[u8] = b"\xff\xfe not valid utf-8\n";
        let reader = BufReader::new(input);
        let mut out: Vec<u8> = Vec::new();
        let mut bridge = Bridge::new();
        dispatch_loop(reader, &mut out, &mut bridge).await;
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#""type":"error""#),
            "expected an error frame: {s}"
        );
        assert!(
            s.contains("stdin read error"),
            "must record the read-error reason: {s}"
        );
    }

    #[tokio::test]
    async fn dispatch_loop_handles_valid_frame_then_clean_eof() {
        // A valid shutdown frame is handled and the loop stops cleanly at EOF,
        // proving the extracted loop drives normal traffic, not only errors.
        let input: &[u8] = b"{\"type\":\"shutdown\"}\n";
        let reader = BufReader::new(input);
        let mut out: Vec<u8> = Vec::new();
        let mut bridge = Bridge::new();
        dispatch_loop(reader, &mut out, &mut bridge).await;
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains(r#""type":"shutdown""#),
            "expected a shutdown frame: {s}"
        );
        // Negative: no error frame for a clean valid-then-EOF run.
        assert!(
            !s.contains(r#""type":"error""#),
            "unexpected error frame: {s}"
        );
    }

    // ── the materialize DTO exposes the shifted source map across the boundary ─

    #[test]
    fn artifact_dto_carries_the_in_memory_shifted_source_map() {
        use crate::materialize::MaterializedArtifact;
        // An artifact WITH a shifted map → the DTO carries that exact content.
        let with_map = MaterializedArtifact {
            source_vue: "/ws/Foo.vue".to_string(),
            generated_path: PathBuf::from("/ws/Foo.vue.tsx"),
            content: "x".to_string(),
            source_map: Some("SHIFTED_MAP_JSON".to_string()),
            source_map_present: true,
        };
        let dto = artifact_dto(&with_map);
        assert_eq!(
            dto.source_map.as_deref(),
            Some("SHIFTED_MAP_JSON"),
            "the DTO must carry the in-memory shifted map content, not drop it"
        );
        assert!(dto.source_map_present);
        // Serializes under the camelCase `sourceMap` key.
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""sourceMap":"SHIFTED_MAP_JSON""#), "{json}");

        // A map-absent artifact → DTO carries None, omitted from the wire.
        let no_map = MaterializedArtifact {
            source_vue: "/ws/Bar.vue".to_string(),
            generated_path: PathBuf::from("/ws/Bar.vue.tsx"),
            content: "y".to_string(),
            source_map: None,
            source_map_present: false,
        };
        let dto = artifact_dto(&no_map);
        assert_eq!(dto.source_map, None);
        let json = serde_json::to_string(&dto).unwrap();
        // The `sourceMap` KEY is omitted (distinct from the `sourceMapPresent`
        // flag, which still serializes).
        assert!(
            !json.contains(r#""sourceMap":"#),
            "an absent map's sourceMap key must be omitted from the DTO: {json}"
        );
        assert!(
            json.contains(r#""sourceMapPresent":false"#),
            "the sourceMapPresent flag still serializes: {json}"
        );
    }

    #[test]
    fn materialize_dto_exposes_shifted_map_that_resolves_a_post_rewrite_offset() {
        use crate::materialize::{materialize, MaterializeRequest, MaterializedArtifact};
        use oxc_sourcemap::{SourceMap, Token};
        use std::borrow::Cow;

        // ── 1) The real host's shifted map flows across the DTO verbatim. ────────
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Entry imports a child `.vue`, so the generated TSX carries a `.vue`
        // specifier the materializer rewrites to `.vue.ts` — shifting the map.
        std::fs::write(
            root.join("Entry.vue"),
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nconst greeting: string = 'hi'\n</script>\n<template><Child />{{ greeting }}</template>\n",
        )
        .unwrap();
        std::fs::write(
            root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span>{{ label }}</span></template>\n",
        )
        .unwrap();
        let report = materialize(&MaterializeRequest {
            workspace_root: root.to_path_buf(),
            entries: vec![root.join("Entry.vue")],
            vendor_node_modules: None,
            expected_vue_version: None,
            strict_vue_version: false,
        })
        .unwrap();
        let entry = report
            .ide_artifacts
            .iter()
            .find(|a| a.generated_path.ends_with("Entry.vue.tsx"))
            .expect("entry tsx");
        let dto = artifact_dto(entry);
        // The DTO map content EQUALS the in-memory shifted map (exposed, not dropped).
        assert_eq!(
            dto.source_map, entry.source_map,
            "the DTO must carry the in-memory shifted map verbatim"
        );
        assert_eq!(dto.source_map_present, entry.source_map_present);
        // The rewrite ran, so the carried map is the SHIFTED one.
        assert!(entry.content.contains("Child.vue.ts"));

        // ── 2) The carried map RESOLVES a known post-rewrite offset (discriminating).
        // The real host map (above) carries no generated token AFTER the insertion
        // on the import line, so it cannot witness the shift by itself. Build a
        // controlled artifact whose map places a probe token at the POST-rewrite
        // `greeting` column (one `.vue`→`.vue.ts` insertion shifted it right by the
        // suffix width), carry it across `artifact_dto`, and REQUIRE the DTO map to
        // resolve that exact generated offset back to the probe's source position.
        // The same query against the PRE-rewrite (unshifted) map lands on the wrong
        // (off-by-suffix) token — the final assertion — so a regression that
        // serialized the pre-rewrite host map through the DTO would fail HERE.
        let rewritten_line = "export { default as X } from './X.vue.ts';export const greeting = 1";
        let greeting_col = rewritten_line.find("greeting").expect("greeting") as u32;
        let suffix_len = ".ts".len() as u32; // the `.vue` → `.vue.ts` insertion width

        // Shifted map: the probe token sits at the POST-rewrite `greeting` column.
        let shifted_tokens =
            vec![Token::new(0, greeting_col, 10, 5, Some(0), None)].into_boxed_slice();
        let shifted_map = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("X.vue")],
            vec![None],
            shifted_tokens,
            None,
        );
        let shifted_json = shifted_map.to_json_string();

        let artifact = MaterializedArtifact {
            source_vue: "/ws/Probe.vue".to_string(),
            generated_path: PathBuf::from("/ws/Probe.vue.tsx"),
            content: rewritten_line.to_string(),
            source_map: Some(shifted_json),
            source_map_present: true,
        };
        let probe_dto = artifact_dto(&artifact);
        // REQUIRED: the DTO must carry a map (no `if let` escape hatch).
        let dto_map_json = probe_dto
            .source_map
            .as_deref()
            .expect("the DTO must carry the shifted map across the boundary");
        let dto_map =
            SourceMap::from_json_string(dto_map_json).expect("DTO map must be valid V3 JSON");
        let lt = dto_map.generate_lookup_table();
        let tok = dto_map
            .lookup_token(&lt, 0, greeting_col)
            .expect("the DTO map resolves the post-rewrite generated offset");
        assert_eq!(
            tok.get_dst_col(),
            greeting_col,
            "the DTO map has a token EXACTLY at the post-rewrite offset (it is the shifted map)"
        );
        assert_eq!(
            (tok.get_src_line(), tok.get_src_col()),
            (10, 5),
            "the post-rewrite offset resolves to its true source position through the DTO"
        );

        // Discrimination: the PRE-rewrite (unshifted) map keeps the probe token at
        // its OLD column, so a query at the post-rewrite offset lands off-by-suffix.
        // A DTO carrying THIS map would fail the exact-column assertion above — which
        // is exactly what makes the boundary test independently discriminating.
        // This tail is ILLUSTRATIVE; the load-bearing assertion is the required-map
        // block above (the DTO carries the shifted map and resolves the exact offset).
        let pre_tokens = vec![Token::new(
            0,
            greeting_col.saturating_sub(suffix_len),
            10,
            5,
            Some(0),
            None,
        )]
        .into_boxed_slice();
        let pre_map = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("X.vue")],
            vec![None],
            pre_tokens,
            None,
        );
        let pre_json = pre_map.to_json_string();
        let pre = SourceMap::from_json_string(&pre_json).expect("pre-rewrite map");
        let plt = pre.generate_lookup_table();
        let pre_tok = pre
            .lookup_token(&plt, 0, greeting_col)
            .expect("floor token");
        assert_ne!(
            pre_tok.get_dst_col(),
            greeting_col,
            "the unshifted host map has NO token at the post-rewrite offset (the regression this guards)"
        );
    }
}
