//! TypeProvider trait and priority tiers.
//!
//! Moved from `verter_lsp::tsgo::traits` to be shared between LSP and
//! component-meta consumers.

use std::future::Future;
use std::pin::Pin;

use crate::protocol::*;

/// Priority tiers for type provider operations.
///
/// Interactive > Normal > Background — the transport drains higher-priority
/// lanes first and preempts lower-priority flushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderPriority {
    /// Hover, completion, definition, type_definition queries;
    /// active-file IDE sync in `ensure_current_file_synced`.
    Interactive,
    /// Imported Vue API warmup, tsconfig path config, deferred same-file API sync.
    Normal,
    /// Workspace scanner sync, non-Vue shadow graph loading,
    /// post-init workspace-folder updates, debounced diagnostics.
    Background,
}

/// TypeScript parsing mode for a published carrier's interactive projection.
/// Kept provider-neutral so activation never has to infer semantics from a file
/// suffix or depend on the higher-level session crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierScriptKind {
    Ts,
    Tsx,
    Js,
    Jsx,
}

/// One already-published framework source to promote into a provider's
/// interactive/project working set. The descriptor contains control-plane
/// identities only; generated carrier bytes remain store-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierActivation {
    pub source_path: String,
    pub companion_path: String,
    pub project_file_name: String,
    pub script_kind: CarrierScriptKind,
}

impl CarrierScriptKind {
    #[must_use]
    pub const fn tsserver_name(self) -> &'static str {
        match self {
            Self::Ts => "TS",
            Self::Tsx => "TSX",
            Self::Js => "JS",
            Self::Jsx => "JSX",
        }
    }
}

/// A boxed, Send future — the return type for all TypeProvider methods.
pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TypeProviderError>> + Send + 'a>>;

/// Abstraction over a TypeScript type provider (e.g., tsserver, TSGO).
///
/// All methods operate on generated file paths and byte offsets (not Vue source
/// positions). Position mapping between Vue and generated files is handled by
/// the caller (LSP layer or resolver layer) before/after calling the provider.
///
/// Uses boxed futures instead of `async fn` to allow `dyn TypeProvider` usage.
pub trait TypeProvider: Send + Sync {
    /// Stable identity of this provider implementation.
    ///
    /// Returns one of `"tsgo"`, `"tsserver"`, `"extension"`. The LSP layer
    /// stamps this onto the resolve envelope and validates it on the way back
    /// in, so a completion item minted by one provider can never be resolved
    /// against a different provider after a mid-session provider swap.
    fn provider_id(&self) -> &'static str;

    /// Whether this provider implements `completionItem/resolve` (auto-import
    /// `additionalTextEdits` / lazy detail enrichment).
    ///
    /// Drives the honest `resolve_provider` server capability — a provider that
    /// cannot resolve must not advertise that it can. Defaults to `false`;
    /// providers with a real [`TypeProvider::resolve_completion`] override it.
    fn supports_completion_resolve(&self) -> bool {
        false
    }

    /// Open a file in the type provider (marks it as "editor-open" — triggers diagnostics).
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;

    /// Load a file into the type provider for import resolution only.
    /// Unlike `open_file`, this does NOT mark the file as editor-open and
    /// does NOT trigger diagnostics. Used for background-synced carrier files.
    ///
    /// REQUIRED, deliberately without a default. A `self.open_file(...)` default
    /// is silent: a wrapper that adds work to `open_file` (a diagnostic barrier, a
    /// snapshot refresh) inherits that work for every background load without a
    /// single line of code saying so, and a whole-workspace scan then runs on the
    /// interactive lane. A provider that genuinely does not distinguish the two
    /// states says so explicitly by forwarding to [`TypeProvider::open_file`] here.
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>;

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult>;

    /// Enrich a completion list with provider-specific detail/documentation.
    ///
    /// The default implementation returns the original items unchanged.
    fn get_completion_details<'a>(
        &'a self,
        _path: &'a str,
        _offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        let cloned = items.to_vec();
        Box::pin(async move { Ok(cloned) })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>>;

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>>;

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>;

    fn get_type_definition(&self, path: &str, offset: u32)
        -> ProviderFuture<'_, Vec<TypeLocation>>;

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>>;

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>>;

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>>;

    /// Code actions (quick fixes / refactors) for the carrier range
    /// `[start_offset, end_offset)` in the queried TSX file.
    ///
    /// `diagnostics` carries the codes + TSX-mapped ranges of the diagnostics the
    /// editor sent with the request (`params.context.diagnostics`), already parsed
    /// and fail-closed by the LSP handler. The tsserver-family providers feed the
    /// codes into `getCodeFixes` `errorCodes`; TSGO synthesizes the
    /// `textDocument/codeAction` `context.diagnostics` array from them. An empty
    /// slice means the request carried no numeric-coded diagnostics — providers
    /// short-circuit to an empty result rather than issuing a useless round-trip.
    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>>;

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>>;

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>>;

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>>;

    /// Resolve a completion item to get additional text edits (e.g., auto-import).
    ///
    /// `data` is the provider's OWN typed resolve key ([`CompletionResolveData`])
    /// minted on the originating [`Completion`] — never an arbitrary JSON blob.
    /// The default returns `None` (provider does not implement resolve).
    fn resolve_completion(
        &self,
        _path: &str,
        _data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        Box::pin(async { Ok(None) })
    }

    /// Gracefully shut down the type provider.
    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Configure the project's compiler options with `paths`/`baseUrl` mappings.
    /// Implemented by engines whose carrier membership is config-injection-based
    /// (tsgo); the tsserver provider relies on its `@verter/typescript-plugin`
    /// making carriers configured-project members, so it uses the default no-op.
    fn configure_paths(
        &self,
        _base_url: &str,
        _paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Evict a carrier companion's stale resolution and warm ScriptInfo from the
    /// engine's caches AFTER the carrier-publish store advances its content.
    ///
    /// A synchronous-host engine (tsserver, via the plugin) caches negative
    /// resolution results — including a `fileExists(companion) == false` probed
    /// while the companion's content was not yet on disk. Once the store warms the
    /// companion, the engine must be told the file changed so it re-resolves;
    /// otherwise a cold-probed import stays pinned to a sticky `TS2307`. The
    /// A previously loaded external companion also has no disk watcher to replace
    /// its warm ScriptInfo after the content-addressed blob changes. The default is
    /// a no-op (an engine whose membership is not store-backed needs no eviction);
    /// `TsserverTypeProvider` overrides it to advance the Verter plugin's
    /// `carrierStoreRefreshToken`; the plugin reloads changed ScriptInfos, clears
    /// the owning project's resolution cache, and reconciles authored-source
    /// roots through TypeScript's project API.
    fn notify_carrier_changed(&self, _companion_path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Evict several freshly-published carrier companions as one provider
    /// refresh. Store-backed engines should override this to coalesce graph
    /// invalidation and its ordering fence; the default preserves compatibility
    /// by delegating to the single-companion operation.
    fn notify_carriers_changed<'a>(
        &'a self,
        companion_paths: &'a [String],
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            for path in companion_paths {
                self.notify_carrier_changed(path).await?;
            }
            Ok(())
        })
    }

    /// Register a published carrier companion with the provider so subsequent
    /// queries route to the carrier's OWNING configured project and convert
    /// positions against the carrier's content. The plugin remains the sole
    /// membership/snapshot authority and generated bytes are never sent to the
    /// engine. A provider may issue one transient contentless companion bootstrap
    /// per configured project so a cold project invokes its external-file hook,
    /// then retain editor-active authored sources as contentless opens; it must
    /// not open each generated companion independently.
    ///
    /// `source_path` is the configured-project carrier identity (`{name}.vue` /
    /// `{name}.svelte`); `companion_path` is its generated companion path
    /// (`{name}.vue.tsx` / `{name}.vue.verter.ts`); `content` is the exact bytes the publish store
    /// holds for it (used ONLY for the provider's local position conversion —
    /// `byte_offset_to_tsserver_pos` / `parse_tsserver_location` — never forwarded
    /// to the engine); `project_file_name` is the owning project's tsconfig path
    /// (resolved by the publish path's `ProjectBinding`), threaded into the
    /// `projectFileName` of carrier diagnostics/definition/hover/completion requests
    /// so the companion is type-checked in its REAL configured project rather than
    /// a fresh inferred/default project (which would yield empty/wrong results).
    ///
    /// Default is a no-op (an engine that does not need project-targeted carrier
    /// queries or local content for position conversion); `TsserverTypeProvider`
    /// overrides it to hydrate its `contents` cache and carrier→project map, sharing
    /// one async project-bootstrap operation across concurrent registrations.
    fn register_carrier_member(
        &self,
        _source_path: &str,
        _companion_path: &str,
        _content: &str,
        _project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Record a published carrier companion without activating it as a TypeScript
    /// project root. Workspace discovery uses this path so generated content and
    /// routing are ready for imported/navigation targets without making tsserver
    /// eagerly type-check every carrier in the workspace. Providers that do not
    /// distinguish publication from activation may use the default forwarding
    /// implementation.
    fn register_carrier_metadata<'a>(
        &'a self,
        source_path: &'a str,
        companion_path: &'a str,
        content: &'a str,
        project_file_name: &'a str,
    ) -> ProviderFuture<'a, ()> {
        self.register_carrier_member(source_path, companion_path, content, project_file_name)
    }

    /// Promote one already-published IDE companion into the provider's active
    /// working set without regenerating or retransmitting its content. Workspace
    /// discovery publishes carrier metadata in bulk; an editor open uses this
    /// control-plane operation to make that existing snapshot interactive while
    /// Verter's semantic/typeinfo refresh proceeds independently.
    ///
    /// Providers whose metadata registration is already active (for example
    /// direct-file tsgo sessions) need no additional work and inherit this no-op.
    fn activate_carrier_member(
        &self,
        _source_path: &str,
        _companion_path: &str,
        _project_file_name: &str,
        _script_kind: CarrierScriptKind,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Promote a demand-discovered carrier frontier atomically from the
    /// provider's point of view. Store-backed providers override this to admit
    /// every source before issuing one project refresh; the default preserves
    /// compatibility for providers whose single-member activation is already a
    /// no-op or constant-cost operation.
    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            for member in members {
                self.activate_carrier_member(
                    &member.source_path,
                    &member.companion_path,
                    &member.project_file_name,
                    member.script_kind,
                )
                .await?;
            }
            Ok(())
        })
    }

    /// Close and re-open all files to refresh project associations.
    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Notify about workspace folder changes (for multi-root support).
    fn update_workspace_folders(
        &self,
        _added: Vec<serde_json::Value>,
        _removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Return the PID of the child process, if any.
    fn child_pid(&self) -> Option<u32> {
        None
    }

    // ── Background-priority file operations ────────────────────────────

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.load_file(path, content)
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.update_file(path, content)
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.close_file(path)
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        self.get_diagnostics(path)
    }

    fn configure_paths_background(
        &self,
        base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        self.configure_paths(base_url, paths)
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.update_workspace_folders(added, removed)
    }

    // ── Normal-priority file operations ─────────────────────────────

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.load_file(path, content)
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.update_file(path, content)
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.close_file(path)
    }
}
