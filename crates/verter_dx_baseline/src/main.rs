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
//! The bridge also exposes the lazy auto-import-on-accept route
//! (`resolveCompletion`): after a `completion` query, the runner picks the item
//! carrying an actionable `resolveData` handle and sends it back so the SAME real
//! provider (tsgo or tsserver) resolves its `additionalTextEdits` through
//! `verter_type_runtime::TypeProvider::resolve_completion`. This is the
//! bridge-side surface the differential uses to prove tsserver and tsgo return
//! the SAME resolved import edits — provider parity for auto-import. The bridge
//! still normalizes provider output only; the carrier `.vue` re-anchor of those
//! edits is the LSP layer's job and is exercised by the VS Code E2E gate.
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
    NormalizedLocation, NormalizedResolvedTextEdit, OpenRequest, OpenResponse,
    ProviderCapabilities, ProviderName, QueryMethod, QueryRequest, QueryResponse, QueryResult,
    Request, ResolveCompletionRequest, ResolveCompletionResponse, Response, ShutdownResponse,
    SyncAction, SyncArtifactsRequest, SyncArtifactsResponse,
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
            Request::ResolveCompletion(r) => (self.on_resolve_completion(r).await, false),
            Request::Diagnostics(d) => (self.on_diagnostics(d).await, false),
            Request::Shutdown => (self.on_shutdown().await, true),
        }
    }

    async fn on_hello(&mut self, h: HelloRequest) -> Response {
        let resolution =
            match provider::resolve(h.provider, &h.tool_root, &h.workspace_root, h.strict_ci).await
            {
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
            Err(resp) => return *resp,
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

    async fn on_resolve_completion(&mut self, r: ResolveCompletionRequest) -> Response {
        let (provider, caps) = match self.ready_provider() {
            Ok(rp) => rp,
            Err(resp) => return *resp,
        };
        // Path-precise staleness: gate on the SPECIFIC generated artifact being
        // resolved, the same gate `on_query` applies.
        if let ProbeStatus::Stale { have } = self.overlay.probe_path_status(&r.path, r.version) {
            return Response::stale(&r.uri, r.version, have);
        }
        match provider.resolve_completion(&r.path, r.data).await {
            Ok(resolved) => {
                self.baseline_ran += 1;
                let resolved = resolved.unwrap_or_default();
                Response::ResolveCompletion(ResolveCompletionResponse {
                    uri: r.uri,
                    version: r.version,
                    additional_text_edits: resolved
                        .additional_text_edits
                        .iter()
                        .map(NormalizedResolvedTextEdit::from)
                        .collect(),
                    detail: resolved.detail,
                    documentation: resolved.documentation,
                    capabilities: caps,
                })
            }
            Err(e) => Response::error(ErrorKind::ProviderError, e.to_string()),
        }
    }

    async fn on_diagnostics(&mut self, d: DiagnosticsRequest) -> Response {
        let (provider, caps) = match self.ready_provider() {
            Ok(rp) => rp,
            Err(resp) => return *resp,
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
    // `Box<Response>` keeps the `Err` variant small: `Response` is a large wire
    // enum, so returning it unboxed in a `Result` trips `clippy::result_large_err`.
    fn ready_provider(
        &self,
    ) -> Result<(Arc<dyn TypeProvider>, ProviderCapabilities), Box<Response>> {
        if self.skipped {
            return Err(Box::new(Response::error(
                ErrorKind::NotInitialized,
                "baseline provider not available (skipped)",
            )));
        }
        match &self.ready {
            Some(r) => Ok((Arc::clone(&r.provider), r.capabilities.clone())),
            None => Err(Box::new(Response::error(
                ErrorKind::NotInitialized,
                "probe before hello",
            ))),
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
mod main_tests;
