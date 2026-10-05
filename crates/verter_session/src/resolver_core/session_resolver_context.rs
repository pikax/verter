//! Session request lifecycle for the shared request-bound resolver adapter.

use std::sync::Arc;

use verter_session_query::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use crate::project_type_store::IndexedReady;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_core::resolver_context::{
    MaterializeScopeObservation, RequestBoundAdapter, RequestBoundLifecycle, ResolverContext,
};
#[cfg(any(test, feature = "test-support"))]
use crate::resolver_store::HostStoreView;
use crate::session_view::SessionView;
use crate::types::Hash16;

pub(crate) struct SessionRequestLifecycle<'a> {
    inner: &'a crate::VerterHost,
    view: &'a dyn SessionView,
    request_view: RequestStoreView<'a>,
    /// The view's overlay, built here only when the store view does not
    /// already carry it (a store view not derived from this view).
    fallback_overlay: std::sync::OnceLock<Option<verter_workspace::ResolutionOverlaySnapshot>>,
}

/// Request-bound session context using the shared `ResolverContext` adapter.
pub(crate) type SessionResolverContext<'a> = RequestBoundAdapter<SessionRequestLifecycle<'a>>;

impl<'a> RequestBoundAdapter<SessionRequestLifecycle<'a>> {
    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new(
        inner: &'a crate::VerterHost,
        view: &'a dyn SessionView,
        base: &'a HostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self(SessionRequestLifecycle {
            inner,
            view,
            request_view: RequestStoreView::new(base, overlay),
            fallback_overlay: std::sync::OnceLock::new(),
        })
    }

    #[must_use]
    pub(crate) fn from_cold_seed(
        inner: &'a crate::VerterHost,
        view: &'a dyn SessionView,
        base: &'a crate::resolver_store::ColdSeedHostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self(SessionRequestLifecycle {
            inner,
            view,
            request_view: RequestStoreView::new_cold_seed(base.view(), overlay, base.is_current()),
            fallback_overlay: std::sync::OnceLock::new(),
        })
    }
}

impl RequestBoundLifecycle for SessionRequestLifecycle<'_> {
    fn host(&self) -> &crate::VerterHost {
        self.inner
    }

    fn request_view(&self) -> &RequestStoreView<'_> {
        &self.request_view
    }

    fn session_view(&self) -> Option<&dyn SessionView> {
        Some(self.view)
    }

    fn resolution_overlay(&self) -> Option<&verter_workspace::ResolutionOverlaySnapshot> {
        if let Some(overlay) = self.request_view.base().resolution_overlay() {
            return Some(overlay);
        }
        self.fallback_overlay
            .get_or_init(|| {
                let overlay = self.inner.resolution_overlay_snapshot(self.view);
                (!overlay.is_empty()).then_some(overlay)
            })
            .as_ref()
    }

    fn complete_canonical(&self, canonical: &str) {
        self.request_view
            .overlay()
            .complete_canonical_with_session_view(
                self.inner,
                self.request_view.base(),
                self.view,
                canonical,
            );
    }

    fn prepared_decl_bundle(
        &self,
        ctx: &dyn ResolverContext,
        canonical_id: &str,
    ) -> Option<Arc<PreparedDeclBundle>> {
        self.inner.prepared_decl_bundle_with_context(
            ctx,
            crate::host_manage::prepared_decl::SourceRequestServices {
                session_view: Some(self.view),
                completion_overlay: Some(self.request_view.overlay()),
                base_view: Some(self.request_view.base()),
            },
            canonical_id,
        )
    }

    fn prepared_type_decl(
        &self,
        ctx: &dyn ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.inner.prepared_type_decl_in_with_context(
            ctx,
            crate::host_manage::prepared_decl::SourceRequestServices {
                session_view: Some(self.view),
                completion_overlay: Some(self.request_view.overlay()),
                base_view: Some(self.request_view.base()),
            },
            canonical_id,
            owner,
            symbol_name,
        )
    }

    fn prepared_value_decl(
        &self,
        ctx: &dyn ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.inner.prepared_value_decl_in_with_context(
            ctx,
            crate::host_manage::prepared_decl::SourceRequestServices {
                session_view: Some(self.view),
                completion_overlay: Some(self.request_view.overlay()),
                base_view: Some(self.request_view.base()),
            },
            canonical_id,
            owner,
            symbol_name,
        )
    }

    fn materialize_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        crate::host_manage::overlay_priority::ensure_indexed_ready_serve_with_view(
            self.inner,
            self.view,
            canonical_id,
        )
    }

    fn load(&self, canonical_id: &str) -> bool {
        crate::host_manage::overlay_priority::ensure_loaded_with_view(
            self.inner,
            self.view,
            canonical_id,
        )
    }

    fn shallow_file_state(
        &self,
        ctx: &dyn ResolverContext,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        self.inner.shallow_file_state_with_context(
            ctx,
            crate::host_manage::prepared_decl::SourceRequestServices {
                session_view: Some(self.view),
                completion_overlay: Some(self.request_view.overlay()),
                base_view: Some(self.request_view.base()),
            },
            canonical_id,
        )
    }

    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        if let Some(overlay_hash) = self.view.overlay_content_hash_for(canonical) {
            return Some(overlay_hash);
        }
        if self.view.is_tombstoned(canonical) {
            return None;
        }
        self.inner.authoritative_current_content_hash(canonical)
    }

    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedReady>> {
        if self.view.overlay_content_hash_for(canonical).is_some() {
            return self
                .inner
                .materialize_overlay_indexed_ready_serve_with_view(canonical, self.view)
                .map(|serve| serve.indexed);
        }
        if self.view.is_tombstoned(canonical) {
            return None;
        }
        self.inner.current_content_pinned_indexed(canonical)
    }

    fn artifact_key_for_current_content(
        &self,
        canonical: &str,
    ) -> Option<crate::file_artifact_store::FileArtifactKey> {
        if self.view.overlay_content_hash_for(canonical).is_some() {
            return self
                .inner
                .overlay_artifact_identity(canonical)
                .current_read_key(self.inner, self.view);
        }
        self.inner.authoritative_current_artifact_key(canonical)
    }

    fn observe_materialize_scope(
        &self,
        ctx: &dyn ResolverContext,
        canonical: &str,
    ) -> Option<MaterializeScopeObservation> {
        if let Some(overlay_hash) = self.view.overlay_content_hash_for(canonical) {
            let _ = crate::resolver_core::request_ports::IndexedInputs::ensure_indexed_ready_serve(
                ctx, canonical,
            );
            let identity = self.inner.overlay_artifact_identity(canonical);
            let indexed = Arc::clone(
                &identity
                    .lookup_overlay_artifacts(self.inner, self.view)?
                    .indexed,
            );
            let syntactic_export_set =
                crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
                    ctx,
                    canonical,
                    overlay_hash,
                    verter_session_query::facts::FactKey::SyntacticExportSet,
                    verter_session_query::facts::FactLane::Semantic,
                );
            return Some(MaterializeScopeObservation {
                canonical_id: Arc::from(canonical),
                observed_whole_hash: indexed.whole_hash,
                observed_shallow_hash: indexed.shallow_state.whole_hash,
                syntactic_export_set,
            });
        }
        if self.view.is_tombstoned(canonical) {
            return None;
        }
        self.inner
            .observe_materialize_scope_with_context(ctx, canonical)
    }

    fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        crate::resolver_core::resolver_context::type_route_answer(
            self.inner.resolve_type_dependency_canonical_in(
                self.resolution_overlay(),
                owner_canonical,
                import_source,
            ),
        )
    }
}
