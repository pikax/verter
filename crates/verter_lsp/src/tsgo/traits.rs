use std::future::Future;
use std::pin::Pin;

use crate::tsgo::protocol::*;

/// Priority tiers for type provider operations.
///
/// Interactive > Normal > Background — the transport drains higher-priority
/// lanes first and preempts lower-priority flushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProviderPriority {
    /// Hover, completion, definition, type_definition queries;
    /// active-file IDE sync in `ensure_current_file_synced`.
    Interactive,
    /// Imported Vue API warmup, tsconfig path config, deferred same-file API sync.
    Normal,
    /// Workspace scanner sync, non-Vue shadow graph loading,
    /// post-init workspace-folder updates, debounced diagnostics.
    Background,
}

/// A boxed, Send future — the return type for all TypeProvider methods.
pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TypeProviderError>> + Send + 'a>>;

/// Abstraction over a TypeScript type provider (e.g., TSGO).
///
/// `verter_lsp` defines this trait; consumers (VS Code extension, playground, tests)
/// provide their own implementations. `verter_lsp` never instantiates a `TypeProvider`
/// itself — it receives one from the outside via `LspConfig`.
///
/// All methods operate on generated TSX file paths and byte offsets (not Vue source positions).
/// Position mapping between Vue and TSX is handled by the LSP layer before/after calling
/// the type provider.
///
/// Uses boxed futures instead of `async fn` to allow `dyn TypeProvider` usage.
pub trait TypeProvider: Send + Sync {
    /// Open a file in the type provider (marks it as "editor-open" — triggers diagnostics).
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;

    /// Load a file into the type provider for import resolution only.
    /// Unlike `open_file`, this does NOT mark the file as editor-open and
    /// does NOT trigger diagnostics. Used for background-synced .vue files.
    /// Default: falls back to `open_file` (providers that don't distinguish).
    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()>;

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult>;

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

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
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
    /// `data` is the opaque data from the original completion item.
    /// Returns `None` if no additional edits are needed.
    fn resolve_completion(
        &self,
        _path: &str,
        _data: serde_json::Value,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        Box::pin(async { Ok(None) })
    }

    /// Gracefully shut down the type provider.
    ///
    /// For TSGO, this sends the LSP `shutdown` request followed by `exit` notification.
    /// Default implementation is a no-op for providers that don't need cleanup.
    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Update the inferred project compiler options with path mappings.
    ///
    /// Called after workspace initialization when tsconfig paths are discovered.
    /// For tsserver, sends updated `compilerOptionsForInferredProjects`.
    /// For TSGO, sends `workspace/didChangeConfiguration` (may be ignored by TSGO).
    fn configure_paths(
        &self,
        _base_url: &str,
        _paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Close and re-open all files in the provider to refresh project associations.
    /// Used after workspace folder or path configuration changes so that tsserver
    /// re-discovers each file's project using the updated `projectRootPath`.
    /// Default: no-op (TSGO handles this via workspace/didChangeWorkspaceFolders).
    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Notify the provider about workspace folder changes (for multi-root support).
    ///
    /// For TSGO, forwards the `workspace/didChangeWorkspaceFolders` notification.
    /// For tsserver, updates the stored project roots for per-file `projectRootPath`.
    /// Default: no-op for providers that don't support multi-root.
    fn update_workspace_folders(
        &self,
        _added: Vec<serde_json::Value>,
        _removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Return the PID of the child process, if any.
    ///
    /// Used by the server to report the TSGO PID to the extension for orphan cleanup.
    fn child_pid(&self) -> Option<u32> {
        None
    }

    // ── Background-priority file operations ────────────────────────────
    // Default: delegate to the interactive (standard) versions.
    // Concrete providers override to route through the Background lane.

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
    // Default: delegate to the interactive (standard) versions.

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
