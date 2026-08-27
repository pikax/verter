//! Base-host request lifecycle for the shared request-bound resolver adapter.

use std::sync::Arc;

use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_core::resolver_context::{
    RequestBoundAdapter, RequestBoundLifecycle, ResolverContext,
};
use crate::resolver_store::HostStoreView;

pub(crate) struct HostRequestLifecycle<'a> {
    inner: &'a crate::VerterHost,
    view: RequestStoreView<'a>,
}

/// Request-bound base-host context.
///
/// Both base and session requests use the single `ResolverContext`
/// implementation on `RequestBoundAdapter`; this type supplies only the
/// lifecycle-specific construction and observation hooks.
pub(crate) type HostResolverContext<'a> = RequestBoundAdapter<HostRequestLifecycle<'a>>;

impl<'a> RequestBoundAdapter<HostRequestLifecycle<'a>> {
    #[must_use]
    #[cfg(any(test, feature = "test-support", feature = "oracle-gen"))]
    pub(crate) fn new(
        inner: &'a crate::VerterHost,
        base: &'a HostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self(HostRequestLifecycle {
            inner,
            view: RequestStoreView::new(base, overlay),
        })
    }

    #[must_use]
    pub(crate) fn from_current(
        inner: &'a crate::VerterHost,
        base: &'a crate::resolver_store::CurrentHostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self(HostRequestLifecycle {
            inner,
            view: RequestStoreView::new(base.view(), overlay),
        })
    }

    #[must_use]
    pub(crate) fn from_cold_seed(
        inner: &'a crate::VerterHost,
        base: &'a crate::resolver_store::ColdSeedHostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self(HostRequestLifecycle {
            inner,
            view: RequestStoreView::new_cold_seed(base.view(), overlay, base.is_current()),
        })
    }

    /// Bind a request context to a batch-captured fixed view.
    #[must_use]
    pub(crate) fn from_fixed_view(
        inner: &'a crate::VerterHost,
        base: &'a HostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
        is_current: bool,
    ) -> Self {
        Self(HostRequestLifecycle {
            inner,
            view: RequestStoreView::new_cold_seed(base, overlay, is_current),
        })
    }
}

impl RequestBoundLifecycle for HostRequestLifecycle<'_> {
    fn host(&self) -> &crate::VerterHost {
        self.inner
    }

    fn request_view(&self) -> &RequestStoreView<'_> {
        &self.view
    }

    fn session_view(&self) -> Option<&dyn crate::session_view::SessionView> {
        None
    }

    fn complete_canonical(&self, canonical: &str) {
        self.view
            .overlay()
            .complete_canonical(self.inner, self.view.base(), canonical);
    }

    #[track_caller]
    fn owned_store_view(&self) -> HostStoreView {
        self.view.base().clone()
    }

    fn prepared_decl_bundle(
        &self,
        _ctx: &dyn ResolverContext,
        canonical_id: &str,
    ) -> Option<Arc<PreparedDeclBundle>> {
        self.inner.prepared_decl_bundle_with_store_view(
            &self.view,
            Some(self.view.overlay().bundle_memo()),
            canonical_id,
        )
    }

    fn prepared_type_decl(
        &self,
        _ctx: &dyn ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.inner.prepared_type_decl_in_with_store_view(
            &self.view,
            Some(self.view.overlay().bundle_memo()),
            canonical_id,
            owner,
            symbol_name,
        )
    }

    fn prepared_value_decl(
        &self,
        _ctx: &dyn ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.inner.prepared_value_decl_in_with_store_view(
            &self.view,
            Some(self.view.overlay().bundle_memo()),
            canonical_id,
            owner,
            symbol_name,
        )
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn with_bare_host_ctx_for_test<R>(
    host: &crate::VerterHost,
    f: impl FnOnce(&(dyn ResolverContext + Sync)) -> R,
) -> R {
    let view = crate::VerterHost::resolver_store_view(host).into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let host_ctx = HostResolverContext::new(host, &view, overlay);
    f(&host_ctx)
}
