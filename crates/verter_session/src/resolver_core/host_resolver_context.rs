//! Request-bound [`ResolverContext`] wrapper that carries a borrowed
//! [`HostStoreView`] alongside the host reference.
//!
//! ## Why a separate wrapper (rather than `impl ResolverContext for VerterHost`)
//!
//! The bare `impl ResolverContext for VerterHost` cannot satisfy the
//! request-bound `store_view()` borrow contract — `&VerterHost` owns
//! no `HostStoreView`. The previous trait shape returned a freshly-built
//! owned `HostStoreView` from every call, which made every
//! `ValidatedFactCache::get_if_valid*` warm-hit pay for a full workspace
//! sweep (5-7 sweeps, no memoisation). That is the regression the per-request hoist
//! fixes.
//!
//! [`HostResolverContext`] holds `(&'a VerterHost, &'a HostStoreView)`
//! and implements `ResolverContext` by delegating every method to the
//! inner host EXCEPT [`ResolverContext::store_view`], which returns the
//! borrow. The view is built ONCE at the request boundary and threaded
//! through the resolver pipeline.
//!
//! ## Construction
//!
//! Top-level request entry points (component-meta cold compute,
//! fallthrough resolution, any resolver-tier consumer that opens a
//! transient `ProjectSemanticDispatch::new(...)`) build the view via
//! [`crate::VerterHost::resolver_store_view`] once, then pass
//! `&HostResolverContext::new(host, &view)` into the dispatcher /
//! pipeline. The wrapper does not retain references after the call
//! returns.
//!
//! ## Lifetime story
//!
//! Both fields are `&'a` borrows. The borrow shape matches
//! [`crate::resolver_core::SessionResolverContext`]; callers create the
//! wrapper on the stack, pass `&wrapper` to the dispatcher, run the
//! query, and let it drop at the end of the call.
//!
//! ## Sealing
//!
//! [`ResolverContext`] is sealed via `mod sealed` in `resolver_context.rs`.
//! The `impl Sealed for HostResolverContext<'_>` lives in the same
//! sealing block.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource;
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_workspace::{AmbientSymbolHit, ProjectStableKey};

use crate::host_manage::ValueDeclIdentity;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::project_type_store::ProjectTypeStore;
use crate::request_context::bump_resolver_store_view_call;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_core::resolver_context::ResolverContext;
use crate::resolver_core::{FactReadSetCell, FactVersionRef, ShallowFileState, StoreView};
use crate::resolver_store::HostStoreView;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::types::Hash16;
use crate::FileAnalysisSnapshot;
use crate::HostConfig;

/// Request-bound [`ResolverContext`] wrapper.
///
/// Holds `(&'a VerterHost, RequestStoreView<'a>)`. Every
/// [`ResolverContext`] method delegates to the inner host except
/// [`Self::store_view`], which returns a borrow into the owned
/// [`RequestStoreView`] field. The base [`HostStoreView`] is built
/// ONCE at the request boundary; resolver-tier callers that consult
/// cached-validity-bound state read through the borrow instead of
/// triggering a per-call full workspace snapshot.
///
/// The owned [`RequestStoreView`] chains a
/// [`CanonicalCompletionOverlay`] in front of the borrowed base view
/// so canonicals loaded mid-request (`ensure_loaded` /
/// `ensure_indexed_ready_serve` successes) are promoted into the overlay and
/// observed by the self-root fact validator on subsequent reads. The
/// overlay lives behind an `Arc` so cooperative-admission lanes that
/// inherit the request's context can share it.
pub(crate) struct HostResolverContext<'a> {
    inner: &'a crate::VerterHost,
    view: RequestStoreView<'a>,
}

impl<'a> HostResolverContext<'a> {
    /// Construct a request-bound wrapper over `(inner, base, overlay)`.
    ///
    /// Callers build `base` once via
    /// [`crate::VerterHost::resolver_store_view`] at the request entry,
    /// then pass `&wrapper` to the resolver-tier pipeline. The wrapper
    /// owns its own [`RequestStoreView`]; the base view is borrowed for
    /// the duration of the wrapper. Pass an `Arc<CanonicalCompletionOverlay>`
    /// so cooperative-admission lanes that inherit the context share
    /// the same overlay.
    #[must_use]
    pub(crate) fn new(
        inner: &'a crate::VerterHost,
        base: &'a HostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self {
            inner,
            view: RequestStoreView::new(base, overlay),
        }
    }

    /// Construct a request-bound wrapper rooted on a PROVEN-CURRENT base
    /// view ([`crate::resolver_store::CurrentHostStoreView`]).
    ///
    /// Use this when the caller obtained the view through
    /// [`crate::resolver_store::StoreViewRead::current`] (e.g. a typeinfo
    /// query-returner after a bounded retry settled on a current read):
    /// the nested warm-cache probes inside the dispatch may validate
    /// against the view because the manager proved it coherent.
    #[must_use]
    pub(crate) fn from_current(
        inner: &'a crate::VerterHost,
        base: &'a crate::resolver_store::CurrentHostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self {
            inner,
            view: RequestStoreView::new(base.view(), overlay),
        }
    }

    /// Construct a request-bound wrapper rooted on a COLD-SEED base view
    /// ([`crate::resolver_store::ColdSeedHostStoreView`]).
    ///
    /// Use this for a fenced cold builder or a post-fence extraction
    /// binder. The cold-seed carries its own currentness: if the seed
    /// originated from a non-current (`ReturnOnly`) read, EVERY nested
    /// warm-cache probe through this context MISSES (the `RequestStoreView`
    /// fails its `validates*` family closed), so a result computed against
    /// the stale seed can never be warm-served — the builder's own
    /// `is_stable` / publish fence guards top-level promotion.
    #[must_use]
    pub(crate) fn from_cold_seed(
        inner: &'a crate::VerterHost,
        base: &'a crate::resolver_store::ColdSeedHostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self {
            inner,
            view: RequestStoreView::new_cold_seed(base.view(), overlay, base.is_current()),
        }
    }

    /// Borrow the inner host.
    ///
    /// Reserved accessor for cooperative-admission lanes that inherit a
    /// HostResolverContext and need to construct a sibling wrapper
    /// (see also [`Self::overlay`]). Currently unused — every active
    /// production caller goes through the impl blocks; the lane-
    /// inheritance path lives in `SessionResolverContext` for now. Kept
    /// here for symmetry with the session variant.
    #[allow(dead_code)]
    pub(crate) fn host(&self) -> &'a crate::VerterHost {
        self.inner
    }

    /// Borrow the request-scoped overlay.
    ///
    /// Cooperative-admission lanes that inherit the context call this
    /// to clone the `Arc` and seed a sibling wrapper that shares the
    /// same per-request completion state. Currently unused — the
    /// active lane-inheritance path lives in `SessionResolverContext`;
    /// kept here for symmetry with the session variant.
    #[allow(dead_code)]
    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
        self.view.overlay()
    }

    /// Idempotently promote a newly-loaded canonical into the overlay
    /// (epoch-guarded).
    ///
    /// Called from `ensure_loaded` / `ensure_indexed_ready_serve` success
    /// paths so subsequent self-root fact validation observes the
    /// freshly-loaded canonical's current content rather than
    /// false-missing because the request-entry base view did not track
    /// it.
    pub(crate) fn complete_canonical(&self, canonical: &str) {
        self.view
            .overlay()
            .complete_canonical(self.inner, self.view.base(), canonical);
    }
}

impl<'a> ResolverContext for HostResolverContext<'a> {
    // -------- Identity --------------------------------------------

    #[inline]
    fn is_request_bound(&self) -> bool {
        true
    }

    // -------- Cache accessors --------------------------------------

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
        // Pass the request-bound `RequestStoreView` (which chains the
        // `CanonicalCompletionOverlay` in front of the base) so cache
        // validation consults the overlay-shadowed view rather than
        // bypassing it via `self.view.base()`. The overlay-bypass
        // bug fix: the previous
        // `self.view.base()` argument made every prepared-decl bundle
        // warm-read pay the request-level snapshot cost without
        // observing canonicals promoted by mid-request `ensure_loaded`.
        self.inner
            .prepared_decl_bundle_with_store_view(&self.view, canonical_id)
    }

    #[inline]
    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        // Overlay-aware view (same rationale as `prepared_decl_bundle`).
        self.inner
            .prepared_type_decl_with_store_view(&self.view, canonical_id, symbol_name)
    }

    #[inline]
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        // Overlay-aware view (same rationale as `prepared_decl_bundle`).
        self.inner
            .prepared_value_decl_with_store_view(&self.view, canonical_id, symbol_name)
    }

    #[inline]
    fn ensure_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        let result = crate::VerterHost::ensure_indexed_ready_serve(self.inner, canonical_id);
        if result.is_some() {
            // Eager canonical completion:
            // promote the freshly-loaded canonical's per-canonical
            // facts into the request overlay so subsequent self-root
            // validation does not false-miss on a canonical the
            // request-entry base view did not track. Idempotent +
            // epoch-guarded inside `complete_canonical`.
            self.complete_canonical(canonical_id);
        }
        result
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        let loaded = crate::VerterHost::ensure_loaded(self.inner, canonical_id);
        if loaded {
            self.complete_canonical(canonical_id);
        }
        loaded
    }

    #[inline]
    fn external_type_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<AnalyzedExternalTypeSource>> {
        crate::VerterHost::external_type_analysis(self.inner, canonical_id)
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        crate::VerterHost::shallow_file_state(self.inner, canonical_id)
    }

    #[inline]
    fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        crate::VerterHost::local_type_declaration_id(self.inner, canonical_source, resolved_name)
    }

    #[inline]
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        crate::VerterHost::get_whole_hash(self.inner, canonical)
    }

    #[inline]
    #[track_caller]
    fn resolver_store_view(&self) -> HostStoreView {
        // Owned-view variant — builds a fresh snapshot per call. Retained
        // for cold-path callers that need an owned snapshot; production
        // hot-path callers should prefer [`Self::store_view`] (the borrow
        // into the request-bound view with shadowing overlay) for
        // zero-allocation cache-validity reads.
        bump_resolver_store_view_call();
        crate::VerterHost::resolver_store_view(self.inner).into_owned_view()
    }

    #[inline]
    fn store_view(&self) -> &dyn StoreView {
        &self.view
    }

    #[inline]
    fn project_type_store(&self) -> &Arc<ProjectTypeStore> {
        crate::VerterHost::project_type_store(self.inner)
    }

    #[inline]
    fn config(&self) -> &HostConfig {
        crate::VerterHost::config(self.inner)
    }

    // -------- Symbol / route resolution ----------------------------

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        // Route through the view-bound shim so the cached imported-root
        // entry validates against the request-bound overlay-aware view
        // rather than rebuilding a fresh owned workspace snapshot per
        // call (the carrier site identified by 6.e attribution).
        self.inner.resolve_imported_type_root_with_store_view(
            &self.view,
            dep_canonical,
            imported_name,
        )
    }

    #[inline]
    fn resolve_imported_type_root_with_facts(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (
        (String, String),
        Arc<[crate::resolver_core::FactVersionRef]>,
    ) {
        // Facts-returning variant for memoized-build callers: the same
        // request-bound view as the tuple form, plus the route-chain fact
        // list the caller records onto the active tracer so the enclosing
        // cache entry invalidates on a barrel retarget.
        self.inner
            .resolve_imported_type_root_with_facts_with_store_view(
                &self.view,
                dep_canonical,
                imported_name,
            )
    }

    #[inline]
    fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        // Route through the view-bound variant so the cached route
        // validates against the request-bound view (the carrier site
        // at `route_surface.rs`).
        self.inner.resolve_named_type_export_target_with_store_view(
            &self.view,
            dep_canonical,
            requested_name,
        )
    }

    #[inline]
    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        // Route through the view-bound variant (carrier site at
        // `frontier_engine.rs:903`).
        self.inner
            .resolve_named_type_export_target_shallow_with_store_view(
                &self.view,
                dep_canonical,
                requested_name,
            )
    }

    #[inline]
    fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        // Overlay-aware view (overlay-bypass bug fix — see
        // `prepared_decl_bundle` above for the diagnosis).
        self.inner.resolve_owner_direct_import_with_store_view(
            &self.view,
            owner_canonical,
            local_name,
        )
    }

    #[inline]
    fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        crate::VerterHost::resolve_type_dependency_canonical(
            self.inner,
            owner_canonical,
            import_source,
        )
    }

    #[inline]
    fn routed_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        crate::VerterHost::routed_shallow_state(self.inner, canonical_id)
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        // Route through the context-aware variant so the
        // `HostComponentMetaResolver` walker constructed inside binds
        // to the request-bound view (this `HostResolverContext`)
        // rather than the bare-host context that the no-context entry
        // would synthesize.
        crate::host_manage::jsdoc_resolve::resolve_type_declaration_with_context(
            self.inner,
            self,
            dep_canonical,
            requested_name,
        )
    }

    #[inline]
    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        crate::VerterHost::resolve_value_export_target(self.inner, dep_canonical_id, imported_name)
    }

    // -------- Ambient resolution -----------------------------------

    #[inline]
    fn lookup_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        self.inner
            .workspace()
            .lookup_ambient_symbol(consumer_project, symbol)
    }

    #[inline]
    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str) {
        self.inner
            .workspace()
            .record_ambient_dependency(consumer_canonical, virtual_id);
    }

    #[inline]
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.inner.workspace().is_workspace_owned(canonical_id)
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        self.inner.workspace().is_package_backed(canonical_id)
    }

    // -------- Dispatch facade --------------------------------------

    #[inline]
    fn dispatch(&self) -> ProjectSemanticDispatch<'_> {
        // The dispatcher carries `&dyn ResolverContext`; anchoring at
        // `self` threads the borrowed `HostStoreView` through every
        // dispatch-tier call.
        ProjectSemanticDispatch::new(self)
    }

    #[inline]
    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.inner
            .project_type_store()
            .semantic_graph()
            .node_data(node)
    }

    // -------- Component-meta-tier bridges --------------------------

    #[inline]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        crate::VerterHost::current_dependency_fact_versions(self.inner, canonical, tracked_deps)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        crate::VerterHost::get_raw_analysis_snapshot(self.inner, canonical)
    }

    // -------- Fact tracer ------------------------------------------

    #[inline]
    fn current_fact_tracer(&self) -> Option<&FactReadSetCell> {
        ResolverContext::current_fact_tracer(self.inner)
    }

    #[inline]
    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost {
        self.inner
    }
}

/// Test-only convenience: build a request-bound [`HostResolverContext`]
/// over a bare host and invoke `f` with it.
///
/// Production code paths construct the wrapper inline at the request
/// entry boundary (see e.g. `component_meta_entry.rs`); this helper
/// exists so unit + integration test fixtures that exercise APIs
/// tightened to require `&dyn ResolverContext` (extract / policy /
/// fallthrough / intrinsic-projection) can keep their bodies concise
/// without inlining the 3-line construction dance.
///
/// `#[cfg(any(test, debug_assertions))]`-gated so release builds drop
/// the helper entirely.
#[cfg(any(test, debug_assertions))]
pub(crate) fn with_bare_host_ctx_for_test<R>(
    host: &crate::VerterHost,
    f: impl FnOnce(&dyn ResolverContext) -> R,
) -> R {
    let view = crate::VerterHost::resolver_store_view(host).into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let host_ctx = HostResolverContext::new(host, &view, overlay);
    f(&host_ctx)
}
