//! The `$/verter/tsQuery` transport seam for [`ExtensionTypeProvider`].
//!
//! One raw `command + arguments -> JSON body` choke point between the LSP and
//! the VS Code extension host, factored out of the provider so the provider file
//! holds only provider behaviour. Wired back as a child module (`mod transport`)
//! of `extension_provider.rs`.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::server::{TsQuery, TsQueryParams};
use crate::type_provider::protocol::TypeProviderError;

/// Transport seam for the `$/verter/tsQuery` server→client request.
///
/// `ExtensionTypeProvider` talks to the VS Code extension host over a single
/// raw `command + arguments → JSON body` choke point. In production that body
/// is delivered over a concrete `tower_lsp_server::Client`
/// ([`LspTsQueryTransport`]); tests inject a scripted in-memory transport so the
/// provider's completion / resolve / diagnostics request envelopes can be
/// driven headlessly, without a live extension-host `Client`.
///
/// This is a TRANSPORT abstraction only — it carries the typed `command`/
/// `arguments` envelope and returns the raw response body. It does NOT resolve
/// types, parse responses, or duplicate any of the provider's
/// tsserver-family mapping (`parse_tsserver_completion`,
/// `completion_entry_details_to_resolve_result`, `merge_diagnostic_sets`, …)
/// which remain the single shared owner in `verter_type_runtime::tsserver::ipc`.
///
/// The trait is statically dispatched (generic injection, no `dyn`,
/// no `async_trait`, no boxed future) so the production path stays
/// zero-overhead.
pub trait TsQueryTransport: Send + Sync {
    /// Send one `$/verter/tsQuery` request and return its raw JSON response body.
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<serde_json::Value, TypeProviderError>> + Send + '_;
}

/// Production [`TsQueryTransport`] — forwards each `$/verter/tsQuery` over the
/// deferred extension-host LSP `Client`.
pub struct LspTsQueryTransport {
    /// Deferred LSP client — populated during `LspService::build()`.
    pub(super) client: Arc<OnceCell<tower_lsp_server::Client>>,
}

impl TsQueryTransport for LspTsQueryTransport {
    // The trait declares an explicit `+ Send` return bound (load-bearing: the
    // future is awaited from `Send` provider methods and downstream
    // `ProviderFuture`s). `async fn` in a trait impl cannot express that bound,
    // so the explicit `impl Future + Send` form stays — clippy's `async fn`
    // suggestion would drop the requirement.
    #[allow(clippy::manual_async_fn)]
    fn ts_query(
        &self,
        params: TsQueryParams,
    ) -> impl Future<Output = Result<serde_json::Value, TypeProviderError>> + Send + '_ {
        async move {
            let client = self
                .client
                .get()
                .ok_or_else(|| TypeProviderError::new("LSP client not yet initialized"))?;
            client
                .send_request::<TsQuery>(params)
                .await
                .map_err(|e| TypeProviderError::new(format!("tsQuery failed: {e}")))
        }
    }
}
