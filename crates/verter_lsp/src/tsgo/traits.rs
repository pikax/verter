use std::future::Future;
use std::pin::Pin;

use crate::tsgo::protocol::*;

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
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;

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

    /// Return the PID of the child process, if any.
    ///
    /// Used by the server to report the TSGO PID to the extension for orphan cleanup.
    fn child_pid(&self) -> Option<u32> {
        None
    }
}
