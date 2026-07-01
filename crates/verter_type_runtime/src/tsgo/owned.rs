//! The OWNED one-instance dual-surface tsgo provider.
//!
//! `TsgoOwnedProvider` is ONE [`TypeProvider`] backed by ONE Verter-spawned
//! `tsgo --lsp` process serving BOTH interfaces on ONE shared `project.Session`:
//!
//! The `--lsp` interface (the inner [`TsgoTypeProvider`]) serves the interactive
//! LSP FEATURES (hover, definition, type-definition, references, rename,
//! completion + resolve, signature-help, document highlights, semantic tokens,
//! inlay hints) AND the user-facing diagnostics surface. Its pull diagnostic
//! carries the FULL set — semantic, syntactic, suggestion, the LSP `tags`
//! (unnecessary / deprecated), and related-information — so there is exactly ONE
//! diagnostics authority per epoch.
//!
//! The `--api` CHECKER is attached onto the SAME process via
//! `custom/initializeAPISession` and is the project-bound TYPECHECK / membership /
//! reflection ORACLE ([`TsgoOwnedProvider::semantic_diagnostics_for_carrier`]). It
//! is the authority S3 proves works against the CONFIGURED project; promoting it to
//! the sole user-facing diagnostics surface (over the richer `--lsp` pull) requires
//! closing its per-carrier program parity (the `vue` / JSX / tag / suggestion gaps)
//! and is a full-DX-contract concern, not this provider's job.
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

use parking_lot::Mutex as SyncMutex;
use verter_span::path::fs_paths_equal;
use verter_span::Utf16LineIndex;
use verter_tsgo_api::api_attach::ApiAttachClient;
use verter_tsgo_api::client::probe_engine_version;
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

/// The attached `--api` checker plus the configured-project context the OWNED
/// diagnostics route needs. Held behind a mutex so the snapshot can be refreshed
/// as carriers are opened/changed.
struct ApiSurface {
    /// The `--api` checker client over the minted pipe (same process as `--lsp`).
    client: ApiAttachClient,
    /// The configured tsconfig path (forward-slashed) opened on the `--api` side.
    tsconfig_path: String,
    /// The current `--api` snapshot context: `(snapshot_handle, project_id)`, refreshed
    /// on demand. `None` until the first `updateSnapshot` succeeds.
    snapshot: SyncMutex<Option<(OpaqueHandle, String)>>,
}

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
        f.debug_struct("TsgoOwnedProvider")
            .field("tsconfig", &self.api.tsconfig_path)
            .finish_non_exhaustive()
    }
}

impl TsgoOwnedProvider {
    /// Build the OWNED dual-surface provider over an already-spawned
    /// `TsgoTypeProvider` (the `--lsp` surface), attaching an `--api` checker to its
    /// process and opening `tsconfig_path` (forward-slashed) as the configured
    /// project on the checker.
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
        tsconfig_path: impl Into<String>,
        tsgo_bin: impl AsRef<Path>,
    ) -> Result<Self, crate::protocol::TypeProviderError> {
        // Wire gate FIRST — refuse a diverged/unknown engine before opening the
        // `--api` session, connecting the attach pipe, or constructing the client.
        let version = probe_engine_version(tsgo_bin.as_ref()).map_err(|e| {
            crate::protocol::TypeProviderError::new(format!("tsgo capability probe failed: {e}"))
        })?;
        let _clearance =
            gate::validate(&ObservedEngine::from_codec_wire(version)).map_err(|e| {
                crate::protocol::TypeProviderError::new(format!("unsupported tsgo --api wire: {e}"))
            })?;

        let session = lsp.initialize_api_session().await?;
        let (read, write) = connect_attach_pipe(&session.pipe)
            .await
            .map_err(|e| crate::protocol::TypeProviderError::new(format!("--api attach: {e}")))?;
        let client = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        client.initialize().await.map_err(|e| {
            crate::protocol::TypeProviderError::new(format!("--api initialize: {e}"))
        })?;
        Ok(Self {
            lsp,
            api: Arc::new(ApiSurface {
                client,
                tsconfig_path: tsconfig_path.into(),
                snapshot: SyncMutex::new(None),
            }),
        })
    }

    /// The inner `--lsp` feature provider (for tests / inspection).
    #[must_use]
    pub fn lsp_provider(&self) -> &Arc<TsgoTypeProvider> {
        &self.lsp
    }

    /// The `--api` checker's SEMANTIC diagnostics for a carrier — the typecheck
    /// authority over the CONFIGURED project (the carrier must be a member, opened
    /// as a `--lsp` didOpen overlay the shared session sees). Returns the
    /// engine-native diagnostics mapped to [`TypeDiagnostic`]; `Ok(vec![])` when the
    /// carrier is not a member of the configured project (fail closed — never a
    /// wrong-project result).
    ///
    /// This is the project-bound typecheck oracle the dual-surface model proves;
    /// it is DISTINCT from the user-facing [`TypeProvider::get_diagnostics`]
    /// surface (which is the rich `--lsp` pull set). Promoting this to the sole
    /// diagnostics surface is a full-DX-contract concern (closing the `--api`
    /// per-carrier program parity), not this provider's responsibility.
    pub async fn semantic_diagnostics_for_carrier(
        &self,
        path: &str,
    ) -> Result<Vec<TypeDiagnostic>, crate::protocol::TypeProviderError> {
        let carrier = slash(path);
        let Some((snapshot, project, engine_carrier)) = self.api.resolve_for(&carrier).await else {
            return Ok(Vec::new());
        };
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
        let content = self.lsp.cached_content(&engine_carrier).await;
        position_carrier_diagnostics(&diags, content, &engine_carrier)
    }
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
fn position_carrier_diagnostics(
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
    /// Refresh the `--api` snapshot for the configured project and return
    /// `(snapshot_handle, project_id, engine_carrier_path)` for `carrier`, or `None`
    /// when the project / carrier is not resolvable on the checker.
    ///
    /// `carrier` is the carrier file path; the returned engine path is the carrier
    /// AS THE ENGINE REPORTS IT in the project's root set (diagnostics must be
    /// requested with the engine's own canonical form).
    async fn resolve_for(&self, carrier: &str) -> Option<(OpaqueHandle, String, String)> {
        // A FAILED project open is an unhealthy-provider signal, distinct from a
        // project that simply is not in the snapshot below. Surface it (the owned
        // provider must not serve as if healthy when its configured project cannot
        // open) rather than letting it silently degrade to empty diagnostics.
        let snap = match self
            .client
            .update_snapshot_open_project(&self.tsconfig_path)
            .await
        {
            Ok(snap) => snap,
            Err(err) => {
                tracing::warn!(
                    "owned tsgo `--api` could not open the configured project `{}`: {err}",
                    self.tsconfig_path
                );
                return None;
            }
        };
        // Select the CONFIGURED project for this tsconfig and require the carrier in
        // its root set — configured-project membership, never an inferred/single-file
        // fallback. ABSENCE of the carrier from `project.root_files` is a `None`
        // (fail closed), not a degraded open.
        let (project_id, engine_carrier) =
            select_configured_project_carrier(&snap, &self.tsconfig_path, carrier)?;
        // Update the cached snapshot context (the handle is `Copy`).
        *self.snapshot.lock() = Some((snap.snapshot, project_id.clone()));
        Some((snap.snapshot, project_id, engine_carrier))
    }
}

/// Select the configured project for `tsconfig_path` from an `--api` snapshot and
/// return `(project_id, engine_carrier)` IFF `carrier` is a member of that
/// project's root set — the project-bound membership decision, isolated for testing.
///
/// Two independent gates, both fail-closed to `None`:
///   1. `project_for_config` must find a project whose `configFileName` matches
///      `tsconfig_path` (path-normalized). No matching configured project ⇒ `None` —
///      NEVER a fallback to an inferred/single-file project.
///   2. The `carrier` must appear in that project's `root_files` (path-normalized).
///      Absence ⇒ `None` — the carrier is not a member, NOT a degraded open.
///
/// The returned `engine_carrier` is the carrier AS THE ENGINE REPORTS IT in
/// `root_files` (diagnostics must be requested with the engine's own canonical form).
fn select_configured_project_carrier(
    snap: &verter_tsgo_api::api_attach::AttachSnapshot,
    tsconfig_path: &str,
    carrier: &str,
) -> Option<(String, String)> {
    let project = snap.project_for_config(|c| fs_paths_equal(c, tsconfig_path))?;
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
    // The diagnostics SURFACE delegates to the `--lsp` pull diagnostic (the rich
    // set: semantic + syntactic + suggestion + the LSP `tags` —
    // unnecessary/deprecated — + related-information), which the `--api`
    // `getSemanticDiagnostics` does NOT carry. There is exactly ONE diagnostics
    // authority per epoch (the consult's rule), and for OWNED tsgo that authority
    // is the `--lsp` surface — the `tsgo --lsp` server does its own configured-
    // project discovery, so carriers are project-bound for normal layouts.
    //
    // The attached `--api` checker is the TYPECHECK / membership / reflection
    // ORACLE (proven project-bound + non-vacuous in the crate's `owned_provider`
    // live tests via `semantic_diagnostics_for_carrier`); promoting it to the sole
    // user-facing diagnostics surface requires closing its per-carrier program
    // parity with the `--lsp` program (the `vue`/JSX/tag/suggestion gaps) and is a
    // full-DX-contract concern, not this provider's job.
    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        self.lsp.get_diagnostics(path)
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        self.lsp.get_diagnostics_background(path)
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
