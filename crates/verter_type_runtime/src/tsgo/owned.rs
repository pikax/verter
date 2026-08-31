//! The OWNED one-instance dual-surface tsgo provider.
//!
//! `TsgoOwnedProvider` is ONE [`TypeProvider`] backed by ONE Verter-spawned
//! `tsgo --lsp` process serving BOTH interfaces on ONE shared `project.Session`:
//!
//! The `--lsp` interface (the inner [`TsgoTypeProvider`]) serves interactive LSP
//! features and raw diagnostics for non-carrier files. Carrier diagnostics are
//! resolved against an explicit configured project through the attached `--api`
//! checker, so a generated companion cannot accidentally bind to an inferred or
//! broader project.
//!
//! The `--api` CHECKER is attached onto the SAME process via
//! `custom/initializeAPISession` and is the project-bound TYPECHECK / membership /
//! reflection ORACLE ([`TsgoOwnedProvider::semantic_diagnostics_for_carrier_in_project`]).
//! The served carrier path combines semantic and syntactic diagnostics from one
//! configured-project snapshot and never unions in the raw `--lsp` pull.
//!
//! This is the binding dual-surface architecture: ONE process, ONE query path, the
//! two surfaces an internal implementation detail of one provider. There is NO
//! second process and NO second feature pipeline.
//!
//! ## Why the inner provider is reused
//!
//! The `--lsp` feature surface (`TsgoTypeProvider`) already implements every
//! `TypeProvider` feature method (and the rich pull diagnostic) over `tsgo --lsp`
//! with its mature transport (priority lanes, position mapping, completion
//! enrichment). Re-implementing those over the attach connection would be a
//! forbidden second feature pipeline. So this provider OWNS one `TsgoTypeProvider`,
//! delegates the `TypeProvider` surface to it, and attaches the `--api` checker to
//! ITS process (`initialize_api_session` over its existing connection — no second
//! spawn) as the project-bound typecheck oracle.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;
use verter_span::path::fs_paths_equal;
use verter_span::Utf16LineIndex;
use verter_tsgo_api::api_attach::ApiAttachClient;
use verter_tsgo_api::client::probe_engine_version_bounded;
use verter_tsgo_api::gate::{self, ObservedEngine};
use verter_tsgo_api::jsonrpc::JsonRpcConnection;
use verter_tsgo_api::proto::types::OpaqueHandle;
use verter_tsgo_api::transport::pipe_attach::connect_attach_pipe;

use crate::protocol::{
    Completion, CompletionResolveData, CompletionResolveResult, CompletionResult, HoverInfo,
    InlayHint, ProviderDiagnosticContext, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity, TypeDocumentHighlight, TypeLocation,
};
use crate::traits::{ProviderFuture, TypeProvider};
use crate::tsgo::ipc::TsgoTypeProvider;

/// The attached `--api` checker. It stores NO configured project: the tsconfig is
/// supplied PER QUERY (the owning project the carrier binding resolved), so ONE
/// `--api` process serves EVERY configured project in the workspace — mirroring the
/// SHARED provider's per-query `--api` core. Held behind a mutex so the snapshot can
/// be refreshed as carriers are opened/changed.
struct ApiSurface {
    /// The `--api` checker client over the minted pipe (same process as `--lsp`).
    client: ApiAttachClient,
    /// The engine version the wire gate channel-validated at attach. Passed to
    /// the first `updateSnapshot` so its integer-handle rail refusal names the
    /// real observed engine.
    engine_version: String,
    /// The current `--api` snapshot context: `(snapshot_handle, project_id)`, refreshed
    /// per query. `None` until the first `updateSnapshot` succeeds.
    snapshot: SyncMutex<Option<(OpaqueHandle, String)>>,
}

const OWNED_VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OWNED_API_SESSION_TIMEOUT: Duration = Duration::from_secs(15);
const OWNED_API_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const OWNED_API_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(5);

/// The OWNED one-instance dual-surface tsgo provider.
pub struct TsgoOwnedProvider {
    /// The `--lsp` feature surface (the spawned `tsgo --lsp` process). All
    /// feature and file-op methods delegate here; the `--api` checker is attached
    /// to THIS process.
    lsp: Arc<TsgoTypeProvider>,
    /// The attached `--api` checker surface (diagnostics authority).
    api: Arc<ApiSurface>,
}

impl std::fmt::Debug for TsgoOwnedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The provider stores no tsconfig (the configured project is per-query), so
        // report the engine version — the stable attach-time identity.
        f.debug_struct("TsgoOwnedProvider")
            .field("engine_version", &self.api.engine_version)
            .finish_non_exhaustive()
    }
}

impl TsgoOwnedProvider {
    /// Build the OWNED dual-surface provider over an already-spawned
    /// `TsgoTypeProvider` (the `--lsp` surface), attaching an `--api` checker to its
    /// process. The checker stores NO configured project: each diagnostics query
    /// supplies the carrier's OWN owning tsconfig
    /// ([`Self::semantic_diagnostics_for_carrier_in_project`]), so ONE process serves
    /// every configured project in the workspace (per-project OWNED binding).
    ///
    /// ONE process: the `--api` checker rides the inner provider's `tsgo --lsp`
    /// child via `custom/initializeAPISession`. NO second spawn.
    ///
    /// FAIL-CLOSED wire gate: before any `--api` session is opened, the engine at
    /// `tsgo_bin` is probed for its version and validated against the maintained
    /// wire pin ([`verter_tsgo_api::gate`]). A probe failure, version mismatch, or
    /// fingerprint mismatch returns `Err(TypeProviderError)` and the owned provider
    /// is NEVER exposed — a diverged tsgo never serves `--api` traffic. The probe
    /// runs once per attach.
    pub async fn attach(
        lsp: Arc<TsgoTypeProvider>,
        tsgo_bin: impl AsRef<Path>,
    ) -> Result<Self, crate::protocol::TypeProviderError> {
        // Wire gate FIRST — refuse a diverged/unknown engine before opening the
        // `--api` session, connecting the attach pipe, or constructing the client.
        let version = probe_engine_version_bounded(tsgo_bin.as_ref(), OWNED_VERSION_PROBE_TIMEOUT)
            .await
            .map_err(|e| {
                crate::protocol::TypeProviderError::new(format!(
                    "tsgo capability probe failed: {e}"
                ))
            })?;
        let clearance = gate::validate(&ObservedEngine::from_codec_wire(version)).map_err(|e| {
            crate::protocol::TypeProviderError::new(format!("unsupported tsgo --api wire: {e}"))
        })?;
        let engine_version = clearance.observed_version;

        let session = tokio::time::timeout(OWNED_API_SESSION_TIMEOUT, lsp.initialize_api_session())
            .await
            .map_err(|_| {
                crate::protocol::TypeProviderError::new(
                    "timed out initializing the managed TSGO API session",
                )
            })??;
        let (read, write) = tokio::time::timeout(
            OWNED_API_CONNECT_TIMEOUT,
            connect_attach_pipe(&session.pipe),
        )
        .await
        .map_err(|_| {
            crate::protocol::TypeProviderError::new(
                "timed out connecting to the managed TSGO API session",
            )
        })?
        .map_err(|e| crate::protocol::TypeProviderError::new(format!("--api attach: {e}")))?;
        let client = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        tokio::time::timeout(OWNED_API_INITIALIZE_TIMEOUT, client.initialize())
            .await
            .map_err(|_| {
                crate::protocol::TypeProviderError::new(
                    "timed out initializing the managed TSGO API client",
                )
            })?
            .map_err(|e| {
                crate::protocol::TypeProviderError::new(format!("--api initialize: {e}"))
            })?;
        Ok(Self {
            lsp,
            api: Arc::new(ApiSurface {
                client,
                engine_version,
                snapshot: SyncMutex::new(None),
            }),
        })
    }

    /// The inner `--lsp` feature provider (for tests / inspection).
    #[must_use]
    pub fn lsp_provider(&self) -> &Arc<TsgoTypeProvider> {
        &self.lsp
    }

    /// The `--api` checker's SEMANTIC diagnostics for a carrier in the configured
    /// project `tsconfig` (supplied PER QUERY — the owning tsconfig the carrier's
    /// binding resolved) — the typecheck authority over that CONFIGURED project (the
    /// carrier must be a member, opened as a `--lsp` didOpen overlay the shared
    /// session sees). Returns the engine-native diagnostics mapped to
    /// [`TypeDiagnostic`]; `Ok(vec![])` when the carrier is not a member of that
    /// configured project (fail closed — never a wrong-project result). ONE `--api`
    /// process serves every configured project because `tsconfig` is opened per
    /// query, mirroring the SHARED provider's `overlay_diagnostics_in_project`.
    ///
    /// This semantic-only method is the direct typecheck oracle used by low-level
    /// tests. Production carrier diagnostics use the same project resolution via
    /// [`TypeProvider::get_diagnostics_in_project`] and additionally collect
    /// syntactic diagnostics.
    pub async fn semantic_diagnostics_for_carrier_in_project(
        &self,
        path: &str,
        tsconfig: &str,
    ) -> Result<Vec<TypeDiagnostic>, crate::protocol::TypeProviderError> {
        let carrier = slash(path);
        let Some((snapshot, project, engine_carrier, project_check_js)) =
            self.api.resolve_for(&carrier, tsconfig).await
        else {
            return Ok(Vec::new());
        };
        let content = self.lsp.cached_content(&engine_carrier).await;
        if content.as_deref().is_some_and(|content| {
            !javascript_carrier_semantic_diagnostics_enabled(
                &engine_carrier,
                content,
                project_check_js,
            )
        }) {
            return Ok(Vec::new());
        }
        let diags = self
            .api
            .client
            .get_semantic_diagnostics(&snapshot, &project, &engine_carrier)
            .await
            .map_err(|e| {
                crate::protocol::TypeProviderError::new(format!(
                    "--api getSemanticDiagnostics: {e}"
                ))
            })?;

        // The `--api` diagnostic `pos`/`end` are UTF-16 code units against the
        // carrier's own text; `TypeDiagnostic.start`/`end` is a BYTE contract. Fetch
        // the carrier content the `--lsp` surface already cached (this is a per-file
        // getter, so every returned diagnostic is positioned in `engine_carrier`) and
        // position through the shared `verter_span` UTF-16 line index — built ONCE
        // for the carrier and reused for every diagnostic (not a per-diagnostic
        // walk). A missing content with diagnostics present is a FAIL-CLOSED explicit
        // error (never a forged `(0, 0)` span) — see `position_carrier_diagnostics`.
        position_carrier_diagnostics(&diags, content, &engine_carrier)
    }

    /// Project-bound user-facing diagnostics for a carrier, with an explicit
    /// served signal. Semantic and syntactic diagnostics are read from the same
    /// configured-project snapshot; no raw LSP pull is mixed in because the
    /// companion path may otherwise bind to an inferred or broader project.
    async fn diagnostics_for_carrier_in_project(
        &self,
        path: &str,
        tsconfig: &str,
    ) -> Result<Option<Vec<TypeDiagnostic>>, crate::protocol::TypeProviderError> {
        let carrier = slash(path);
        let Some((snapshot, project, engine_carrier, project_check_js)) =
            self.api.resolve_for(&carrier, tsconfig).await
        else {
            return Ok(None);
        };
        let content = self.lsp.cached_content(&engine_carrier).await;
        let semantic_enabled = content.as_deref().is_none_or(|content| {
            javascript_carrier_semantic_diagnostics_enabled(
                &engine_carrier,
                content,
                project_check_js,
            )
        });
        let mut diagnostics = if semantic_enabled {
            self.api
                .client
                .get_semantic_diagnostics(&snapshot, &project, &engine_carrier)
                .await
                .map_err(|e| {
                    crate::protocol::TypeProviderError::new(format!(
                        "--api getSemanticDiagnostics: {e}"
                    ))
                })?
        } else {
            Vec::new()
        };
        diagnostics.extend(
            self.api
                .client
                .get_syntactic_diagnostics(&snapshot, &project, &engine_carrier)
                .await
                .map_err(|e| {
                    crate::protocol::TypeProviderError::new(format!(
                        "--api getSyntacticDiagnostics: {e}"
                    ))
                })?,
        );

        position_carrier_diagnostics(&diagnostics, content, &engine_carrier).map(Some)
    }
}

/// Whether a carrier should receive semantic diagnostics under its configured
/// project's JavaScript policy.
///
/// TypeScript-family carriers are always checked. JavaScript JSX carriers follow
/// the selected project's `checkJs` value unless an authored leading line pragma
/// overrides it. The compiler lifts genuine authored pragmas to the carrier's
/// leading trivia; block comments, token lookalikes, and comments after the first
/// source token are not file-check pragmas.
#[must_use]
pub fn javascript_carrier_semantic_diagnostics_enabled(
    carrier: &str,
    content: &str,
    project_check_js: bool,
) -> bool {
    let is_javascript = carrier
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("jsx"));
    if !is_javascript {
        return true;
    }

    match leading_file_check_pragma(content) {
        Some(FileCheckPragma::Check) => true,
        Some(FileCheckPragma::NoCheck) => false,
        None => project_check_js,
    }
}

#[derive(Clone, Copy)]
enum FileCheckPragma {
    Check,
    NoCheck,
}

fn leading_file_check_pragma(content: &str) -> Option<FileCheckPragma> {
    let mut leading = content;
    let mut pragma = None;
    loop {
        leading = leading.trim_start_matches(char::is_whitespace);
        if let Some(line) = leading.strip_prefix("//") {
            let (comment, rest) = line
                .split_once('\n')
                .map_or((line, ""), |(comment, rest)| (comment, rest));
            let comment = comment.trim_start();
            for (directive, candidate) in [
                ("@ts-check", FileCheckPragma::Check),
                ("@ts-nocheck", FileCheckPragma::NoCheck),
            ] {
                if let Some(suffix) = comment.strip_prefix(directive) {
                    if suffix
                        .chars()
                        .next()
                        .is_none_or(|character| character.is_ascii_whitespace() || character == ':')
                    {
                        pragma = Some(candidate);
                    }
                }
            }
            leading = rest;
            continue;
        }
        if let Some(block) = leading.strip_prefix("/*") {
            let Some(end) = block.find("*/") else {
                break;
            };
            leading = &block[end + 2..];
            continue;
        }
        break;
    }
    pragma
}

/// Position a carrier's `--api` diagnostics into byte-contract [`TypeDiagnostic`]s,
/// or surface an EXPLICIT error if positioning is impossible.
///
/// - No diagnostics ⇒ `Ok(vec![])` (nothing to position; content is irrelevant).
/// - Diagnostics present + `content` available ⇒ build ONE shared
///   [`Utf16LineIndex`] and map every diagnostic through it.
/// - Diagnostics present + `content` UNAVAILABLE ⇒ `Err` — the UTF-16 offsets
///   cannot be converted to bytes, so we NEVER fabricate a `(0, 0)` span (the old
///   degrade silently mis-positioned every diagnostic to the file start).
///
/// Pure (no I/O) so the fail-closed miss decision is hermetically testable.
///
/// Shared with the SHARED editor-attach provider (`verter_lsp`): both the OWNED
/// dual-surface path and the SHARED relay-attach path map `--api` UTF-16 diagnostic
/// offsets to carrier bytes through THIS single implementation, so the carrier-byte
/// contract (and the fail-closed no-forged-span rule) has ONE authority regardless
/// of engine-serving mode.
pub fn position_carrier_diagnostics(
    diags: &[verter_tsgo_api::proto::types::Diagnostic],
    content: Option<Arc<str>>,
    engine_carrier: &str,
) -> Result<Vec<TypeDiagnostic>, crate::protocol::TypeProviderError> {
    if diags.is_empty() {
        return Ok(Vec::new());
    }
    let Some(content) = content else {
        return Err(crate::protocol::TypeProviderError::new(format!(
            "--api getSemanticDiagnostics: carrier content for '{engine_carrier}' is unavailable, \
             so the UTF-16 diagnostic offsets cannot be positioned (fail-closed: no forged span)"
        )));
    };
    let index = Utf16LineIndex::new(content);
    Ok(diags
        .iter()
        .map(|d| map_api_diagnostic(d, &index))
        .collect())
}

/// Forward-slash-normalize a path for engine comparison.
fn slash(p: &str) -> String {
    p.replace('\\', "/")
}

impl ApiSurface {
    /// Refresh the `--api` snapshot for the configured project `tsconfig` (supplied
    /// per query) and return `(snapshot_handle, project_id, engine_carrier_path,
    /// project_check_js)` for `carrier`, or `None` when the project / carrier is not
    /// resolvable on the checker.
    ///
    /// `carrier` is the carrier file path; the returned engine path is the carrier
    /// AS THE ENGINE REPORTS IT in the project's root set (diagnostics must be
    /// requested with the engine's own canonical form).
    async fn resolve_for(
        &self,
        carrier: &str,
        tsconfig: &str,
    ) -> Option<(OpaqueHandle, String, String, bool)> {
        // A FAILED project open is an unhealthy-provider signal, distinct from a
        // project that simply is not in the snapshot below. Surface it (the owned
        // provider must not serve as if healthy when the configured project cannot
        // open) rather than letting it silently degrade to empty diagnostics.
        let snap = match self
            .client
            .update_snapshot_open_project(tsconfig, &self.engine_version)
            .await
        {
            Ok(snap) => snap,
            Err(err) => {
                tracing::warn!(
                    "owned tsgo `--api` could not open the configured project `{tsconfig}`: {err}"
                );
                return None;
            }
        };
        // Select the CONFIGURED project for this tsconfig and require the carrier in
        // its root set — configured-project membership, never an inferred/single-file
        // fallback. ABSENCE of the carrier from `project.root_files` is a `None`
        // (fail closed), not a degraded open.
        let project = snap.project_for_config(|c| fs_paths_equal(c, tsconfig))?;
        let engine_carrier = project
            .root_files
            .iter()
            .find(|file| fs_paths_equal(file, carrier))
            .cloned()?;
        let project_id = project.id.clone();
        let project_check_js = project
            .compiler_options
            .get("checkJs")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        // Update the cached snapshot context (the handle is `Copy`).
        *self.snapshot.lock() = Some((snap.snapshot, project_id.clone()));
        Some((snap.snapshot, project_id, engine_carrier, project_check_js))
    }
}

/// Select the configured project for `tsconfig` from an `--api` snapshot and
/// return `(project_id, engine_carrier)` IFF `carrier` is a member of that
/// project's root set — the project-bound membership decision, isolated for testing.
///
/// Two independent gates, both fail-closed to `None`:
///   1. `project_for_config` must find a project whose `configFileName` matches
///      `tsconfig` (path-normalized). No matching configured project ⇒ `None` —
///      NEVER a fallback to an inferred/single-file project.
///   2. The `carrier` must appear in that project's `root_files` (path-normalized).
///      Absence ⇒ `None` — the carrier is not a member, NOT a degraded open.
///
/// The returned `engine_carrier` is the carrier AS THE ENGINE REPORTS IT in
/// `root_files` (diagnostics must be requested with the engine's own canonical form).
///
/// Shared with the SHARED editor-attach provider (`verter_lsp`): both serving modes
/// decide configured-project membership through THIS single fail-closed selector —
/// no matching configured project ⇒ `None` (never an inferred/single-file fallback),
/// and carrier absence from `root_files` ⇒ `None` (not a degraded open).
pub fn select_configured_project_carrier(
    snap: &verter_tsgo_api::api_attach::AttachSnapshot,
    tsconfig: &str,
    carrier: &str,
) -> Option<(String, String)> {
    let project = snap.project_for_config(|c| fs_paths_equal(c, tsconfig))?;
    let engine_carrier = project
        .root_files
        .iter()
        .find(|f| fs_paths_equal(f, carrier))
        .cloned()?;
    Some((project.id.clone(), engine_carrier))
}

/// Map a tsgo `--api` diagnostic to the runtime `TypeDiagnostic`; `category` maps
/// to severity.
///
/// The `--api` diagnostic `pos`/`end` are tsgo **UTF-16 code-unit** offsets
/// (TypeScript position semantics), while `TypeDiagnostic.start`/`end` is a
/// **byte** contract (`protocol.rs`). `index` is the carrier content's shared
/// [`Utf16LineIndex`] (built ONCE per carrier by the caller), so the offsets are
/// converted UTF-16 → byte through the SINGLE offset-normalization implementation
/// in `verter_span` — positions never drift on non-ASCII content before the
/// diagnostic (e.g. an em-dash `—` in a carrier comment), and the conversion is not
/// a per-diagnostic re-walk from the file start.
///
/// The content-unavailable case is handled by the CALLER (an explicit provider
/// error) before this runs, so there is no forged `(0, 0)` degrade here — a
/// diagnostic is only ever mapped when its carrier content is genuinely present.
fn map_api_diagnostic(
    d: &verter_tsgo_api::proto::types::Diagnostic,
    index: &Utf16LineIndex,
) -> TypeDiagnostic {
    // The conversion is infallible BY INVARIANT: `Utf16LineIndex::new` always records
    // line 1 at offset 0, so `byte_for_utf16` can never yield `OffsetError::EmptyIndex`.
    // A violation is a corrupt-index bug, not a recoverable content miss (the
    // recoverable miss — a carrier with no content — is caught fail-closed by the
    // caller `position_carrier_diagnostics`), so it fails LOUD rather than forging a
    // silent `(0, 0)` position.
    let start = index
        .byte_for_utf16(d.pos)
        .expect("Utf16LineIndex::new always records line 1 at offset 0, so byte_for_utf16 never yields EmptyIndex")
        as u32;
    let end = index
        .byte_for_utf16(d.end)
        .expect("Utf16LineIndex::new always records line 1 at offset 0, so byte_for_utf16 never yields EmptyIndex")
        as u32;
    TypeDiagnostic {
        message: d.text.clone(),
        // tsgo DiagnosticCategory: 0=Warning, 1=Error, 2=Suggestion, 3=Message.
        severity: match d.category {
            1 => TypeDiagnosticSeverity::Error,
            0 => TypeDiagnosticSeverity::Warning,
            2 => TypeDiagnosticSeverity::Hint,
            _ => TypeDiagnosticSeverity::Info,
        },
        start,
        end,
        code: Some(d.code.to_string()),
        tags: Vec::new(),
        related_information: Vec::new(),
    }
}

impl TypeProvider for TsgoOwnedProvider {
    fn provider_id(&self) -> &'static str {
        // The OWNED dual-surface provider IS the tsgo provider — the `--api` attach
        // is an internal implementation detail of the ONE provider (the consult's
        // "the two surfaces an internal implementation detail"). It reports the same
        // id as the bare provider so every engine-identifying branch / test treats
        // it transparently as tsgo.
        "tsgo"
    }

    fn supports_completion_resolve(&self) -> bool {
        self.lsp.supports_completion_resolve()
    }

    // ── File lifecycle: delegate to --lsp (the --api checker shares the session
    //    and sees the didOpen overlays). After a content change we issue the --lsp
    //    diagnostic barrier so the --api side observes the overlay before its next
    //    updateSnapshot (the two surfaces ride different transports). ──

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let lsp = Arc::clone(&self.lsp);
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            lsp.open_file(&path, &content).await?;
            // Barrier: force the --lsp server to process the didOpen before any
            // --api updateSnapshot enumerates roots on the shared session.
            let _ = lsp.get_diagnostics(&path).await;
            Ok(())
        })
    }

    /// The BACKGROUND load: cache content for import resolution only. It delivers
    /// no `didOpen`, so it takes no diagnostic barrier — the inner `--lsp` provider's
    /// `load_file` is local-only and the `--api` checker reads the same session.
    ///
    /// This override is load-bearing, not a courtesy delegation: [`Self::open_file`]
    /// adds a synchronous `get_diagnostics` barrier, so inheriting the trait's
    /// `load_file → open_file` default would turn every background load (the whole
    /// workspace scan) into an editor open plus a blocking diagnostic round trip.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.load_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let lsp = Arc::clone(&self.lsp);
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            lsp.update_file(&path, &content).await?;
            let _ = lsp.get_diagnostics(&path).await;
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.lsp.close_file(path)
    }

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.open_file_background(path, content)
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.load_file_background(path, content)
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.update_file_background(path, content)
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.lsp.close_file_background(path)
    }

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.open_file_normal(path, content)
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.load_file_normal(path, content)
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.lsp.update_file_normal(path, content)
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.lsp.close_file_normal(path)
    }

    // ── Diagnostics ──
    //
    // Raw diagnostics remain available for non-carrier callers. The LSP composite
    // resolves carrier ownership first and calls `get_diagnostics_in_project`
    // instead, which never delegates to this raw surface.
    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        self.lsp.get_diagnostics(path)
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        self.lsp.get_diagnostics_background(path)
    }

    fn get_diagnostics_in_project<'a>(
        &'a self,
        path: &'a str,
        configured_project: &'a str,
    ) -> ProviderFuture<'a, Option<Vec<TypeDiagnostic>>> {
        Box::pin(self.diagnostics_for_carrier_in_project(path, configured_project))
    }

    // ── Features: delegate to the --lsp surface (the authoritative language-service
    //    provider). These ride the ONE process; full per-feature DX verification is
    //    a separate concern, but each answers through this one provider. ──

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        self.lsp.get_completions(path, offset, trigger_character)
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        self.lsp.get_completion_details(path, offset, items)
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        self.lsp.resolve_completion(path, data)
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        self.lsp.get_hover(path, offset)
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.lsp.get_definition(path, offset)
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.lsp.get_type_definition(path, offset)
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.lsp.get_references(path, offset)
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        self.lsp.get_rename_locations(path, offset)
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        self.lsp.get_signature_help(path, offset)
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        self.lsp
            .get_code_actions(path, start_offset, end_offset, diagnostics)
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        self.lsp.get_semantic_tokens(path)
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        self.lsp.get_document_highlights(path, offset)
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        self.lsp.get_inlay_hints(path, start_offset, end_offset)
    }

    // ── Config / workspace / lifecycle: delegate to --lsp. ──

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        self.lsp.configure_paths(base_url, paths)
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        self.lsp.resync_open_files()
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.lsp.update_workspace_folders(added, removed)
    }

    fn child_pid(&self) -> Option<u32> {
        self.lsp.child_pid()
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        let lsp = Arc::clone(&self.lsp);
        let api = Arc::clone(&self.api);
        Box::pin(async move {
            let _ = api.client.close().await;
            lsp.shutdown().await
        })
    }
}

#[cfg(test)]
#[path = "owned_tests.rs"]
mod tests;
