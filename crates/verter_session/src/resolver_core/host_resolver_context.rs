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
//! sweep (5-7 sweeps, no memoisation). That is the regression Block 6.c
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

use verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource;
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_workspace::{AmbientSymbolHit, ProjectStableKey};

use crate::host_manage::ValueDeclIdentity;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::project_type_store::{IndexedReady, ProjectTypeStore};
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::resolver_context::ResolverContext;
use crate::resolver_core::{FactReadSetCell, FactVersionRef, ShallowFileState};
use crate::resolver_store::HostStoreView;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::types::Hash16;
use crate::FileAnalysisSnapshot;
use crate::HostConfig;

/// Request-bound [`ResolverContext`] wrapper.
///
/// Holds `(&'a VerterHost, &'a HostStoreView)`. Every [`ResolverContext`]
/// method delegates to the inner host except [`Self::store_view`], which
/// returns the borrowed view. The view is built ONCE at the request
/// boundary; resolver-tier callers that consult cached-validity-bound
/// state read through the borrow instead of triggering a per-call full
/// workspace snapshot.
pub(crate) struct HostResolverContext<'a> {
    inner: &'a crate::VerterHost,
    store_view: &'a HostStoreView,
}

impl<'a> HostResolverContext<'a> {
    /// Construct a request-bound wrapper over `(inner, store_view)`.
    ///
    /// Callers build `store_view` once via
    /// [`crate::VerterHost::resolver_store_view`] at the request entry,
    /// then pass `&wrapper` to the resolver-tier pipeline. The wrapper
    /// does not retain references after the call returns.
    #[must_use]
    pub(crate) fn new(inner: &'a crate::VerterHost, store_view: &'a HostStoreView) -> Self {
        Self { inner, store_view }
    }

    /// Borrow the inner host.
    #[allow(dead_code)]
    pub(crate) fn host(&self) -> &'a crate::VerterHost {
        self.inner
    }
}

impl<'a> ResolverContext for HostResolverContext<'a> {
    // -------- Cache accessors --------------------------------------

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
        self.inner
            .prepared_decl_bundle_with_store_view(self.store_view, canonical_id)
    }

    #[inline]
    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        self.inner
            .prepared_type_decl_with_store_view(self.store_view, canonical_id, symbol_name)
    }

    #[inline]
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        self.inner
            .prepared_value_decl_with_store_view(self.store_view, canonical_id, symbol_name)
    }

    #[inline]
    fn ensure_indexed_ready(&self, canonical_id: &str) -> Option<Arc<IndexedReady>> {
        crate::VerterHost::ensure_indexed_ready(self.inner, canonical_id)
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        crate::VerterHost::ensure_loaded(self.inner, canonical_id)
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
    fn resolver_store_view(&self) -> HostStoreView {
        // Owned-view variant — preserves the pre-6.c semantics of
        // building a fresh snapshot per call.
        //
        // Intentionally does NOT delegate to `self.store_view.clone()`:
        // the borrow is the view captured at request construction, which
        // does NOT track dependencies loaded mid-request (the
        // canonical-completion hidden risk codex flagged). Callers that
        // depend on freshness across additive loads read through the
        // fresh build until the overlay is in place.
        //
        // Production hot-path callers should prefer [`Self::store_view`]
        // (the borrow) for zero-allocation cache-validity reads.
        crate::VerterHost::resolver_store_view(self.inner)
    }

    #[inline]
    fn store_view(&self) -> &HostStoreView {
        self.store_view
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
        crate::VerterHost::resolve_imported_type_root(self.inner, dep_canonical, imported_name)
    }

    #[inline]
    fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        crate::VerterHost::resolve_named_type_export_target(
            self.inner,
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
        crate::VerterHost::resolve_named_type_export_target_shallow(
            self.inner,
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
        self.inner.resolve_owner_direct_import_with_store_view(
            self.store_view,
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
    fn resolve_route_type_edge(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        crate::VerterHost::resolve_route_type_edge(self.inner, owner_canonical, source_specifier)
    }

    #[inline]
    fn route_owned_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        crate::VerterHost::route_owned_shallow_state(self.inner, canonical_id)
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        crate::host_manage::jsdoc_resolve::resolve_type_declaration(
            self.inner,
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
