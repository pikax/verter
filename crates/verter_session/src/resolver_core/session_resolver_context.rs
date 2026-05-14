//! Session-bound [`ResolverContext`] wrapper that surfaces a
//! [`SessionView`](crate::session_view::SessionView) to resolver-tier
//! consumers without forking the trait.
//!
//! The base `impl ResolverContext for VerterHost` returns
//! `active_session_view() = None` and a `view()` that hands back a
//! `HostViewRef`. Session-bearing query paths construct
//! [`SessionResolverContext`] over `(inner, view)` so the same trait
//! methods become overlay-aware: the wrapper overrides
//! `active_session_view()` to return `Some(view)` and overrides
//! `view()` to hand back the bound session view, while every other
//! method delegates to the inner [`ResolverContext`].
//!
//! ## Lifetime story
//!
//! [`SessionResolverContext`] borrows both the inner context and the
//! view by reference (`&'a dyn ResolverContext` + `&'a dyn SessionView`).
//! The borrow shape matches the existing
//! [`ProjectSemanticDispatch`](crate::project_semantic_dispatch::ProjectSemanticDispatch)
//! call sites, which already accept `&'a dyn ResolverContext`. No
//! `Arc` wrapping is needed — every session dispatch entry point
//! constructs the wrapper on the stack, runs the query, and lets it
//! drop at the end of the call.
//!
//! ## Sealing
//!
//! [`ResolverContext`] is sealed via `mod sealed { pub trait Sealed {} }`
//! in `resolver_context.rs`. The `impl Sealed for SessionResolverContext<'_>`
//! lives next to the trait's other sealed marker so all sealing decisions
//! stay in one file.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource;
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_semantic::analysis::ScriptAnalysisSnapshot;
use verter_workspace::{AmbientSymbolHit, ProjectStableKey};

use crate::host_manage::ValueDeclIdentity;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::project_type_store::{IndexedReady, ProjectTypeStore};
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::resolver_context::ResolverContext;
use crate::resolver_core::{FactReadSetCell, FactVersionRef, ShallowFileState};
use crate::resolver_store::HostStoreView;
use crate::semantic_query::{DepSignature, SemanticNodeData, SemanticNodeId};
use crate::session_view::SessionView;
use crate::types::Hash16;
use crate::FileAnalysisSnapshot;
use crate::HostConfig;

/// Session-bound [`ResolverContext`] wrapper.
///
/// Delegates every `ResolverContext` method to the inner
/// `&dyn ResolverContext` except [`ResolverContext::active_session_view`]
/// (returns `Some(view)`) and [`ResolverContext::view`] (returns the
/// bound view). Resolver-tier helpers that consult
/// `active_session_view()` for overlay-aware reads observe the session
/// view via this wrapper without changing the trait surface or the
/// call-site signature in [`ProjectSemanticDispatch::new`].
pub(crate) struct SessionResolverContext<'a> {
    inner: &'a dyn ResolverContext,
    view: &'a dyn SessionView,
}

impl<'a> SessionResolverContext<'a> {
    /// Construct a session-bound wrapper over `(inner, view)`.
    ///
    /// The borrow shape matches [`ProjectSemanticDispatch::new`]:
    /// callers create the wrapper on the stack, pass `&wrapper` to
    /// the dispatcher, and drop it at the end of the query. The
    /// wrapper does not retain references after the call returns.
    #[must_use]
    pub(crate) fn new(inner: &'a dyn ResolverContext, view: &'a dyn SessionView) -> Self {
        Self { inner, view }
    }

    /// Borrow the bound session view.
    #[allow(dead_code)]
    pub(crate) fn view_ref(&self) -> &'a dyn SessionView {
        self.view
    }
}

impl<'a> ResolverContext for SessionResolverContext<'a> {
    // -------- Cache accessors --------------------------------------

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
        self.inner.prepared_decl_bundle(canonical_id)
    }

    #[inline]
    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        self.inner.prepared_type_decl(canonical_id, symbol_name)
    }

    #[inline]
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        self.inner.prepared_value_decl(canonical_id, symbol_name)
    }

    #[inline]
    fn ensure_indexed_ready(&self, canonical_id: &str) -> Option<Arc<IndexedReady>> {
        // Overlay-priority hook: the session view governs whether the
        // base host's IndexedReady is acceptable for this query. The
        // host's own ensure-loaded already covers the non-overlay
        // case; overlay-aware materialisation routes through the
        // shared helper in `host_manage::overlay_priority`.
        crate::host_manage::overlay_priority::ensure_indexed_ready_with_view(
            self.inner,
            self.view,
            canonical_id,
        )
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        crate::host_manage::overlay_priority::ensure_loaded_with_view(
            self.inner,
            self.view,
            canonical_id,
        )
    }

    #[inline]
    fn external_type_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<AnalyzedExternalTypeSource>> {
        self.inner.external_type_analysis(canonical_id)
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        self.inner.shallow_file_state(canonical_id)
    }

    #[inline]
    fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        self.inner
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    #[inline]
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        self.inner.get_whole_hash(canonical)
    }

    #[inline]
    fn resolver_store_view(&self) -> HostStoreView {
        self.inner.resolver_store_view()
    }

    #[inline]
    fn project_type_store(&self) -> &Arc<ProjectTypeStore> {
        self.inner.project_type_store()
    }

    #[inline]
    fn config(&self) -> &HostConfig {
        self.inner.config()
    }

    #[inline]
    fn analyzed_macro_snapshot(&self, canonical_id: &str) -> Option<Arc<ScriptAnalysisSnapshot>> {
        self.inner.analyzed_macro_snapshot(canonical_id)
    }

    // -------- Symbol / route resolution ----------------------------

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        self.inner
            .resolve_imported_type_root(dep_canonical, imported_name)
    }

    #[inline]
    fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.inner
            .resolve_named_type_export_target(dep_canonical, requested_name)
    }

    #[inline]
    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.inner
            .resolve_named_type_export_target_shallow(dep_canonical, requested_name)
    }

    #[inline]
    fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        self.inner
            .resolve_owner_direct_import(owner_canonical, local_name)
    }

    #[inline]
    fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.inner
            .resolve_type_dependency_canonical(owner_canonical, import_source)
    }

    #[inline]
    fn resolve_route_type_edge(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        self.inner
            .resolve_route_type_edge(owner_canonical, source_specifier)
    }

    #[inline]
    fn route_owned_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        self.inner.route_owned_shallow_state(canonical_id)
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        ResolverContext::resolve_type_declaration_for_dep(self.inner, dep_canonical, requested_name)
    }

    #[inline]
    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        self.inner
            .resolve_value_export_target(dep_canonical_id, imported_name)
    }

    // -------- Ambient resolution -----------------------------------

    #[inline]
    fn lookup_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        self.inner.lookup_ambient_symbol(consumer_project, symbol)
    }

    #[inline]
    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str) {
        self.inner
            .record_ambient_dependency(consumer_canonical, virtual_id);
    }

    #[inline]
    fn workspace_content_generation(&self) -> u64 {
        self.inner.workspace_content_generation()
    }

    #[inline]
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.inner.workspace_is_workspace_owned(canonical_id)
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        self.inner.workspace_is_package_backed(canonical_id)
    }

    // -------- Dispatch facade --------------------------------------

    #[inline]
    fn dispatch(&self) -> ProjectSemanticDispatch<'_> {
        // The dispatcher carries `&dyn ResolverContext` (sealed),
        // so a fresh dispatcher anchored at `self` correctly threads
        // the session view through every dispatch-tier call.
        ProjectSemanticDispatch::new(self)
    }

    #[inline]
    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.inner.dispatch_node_data(node)
    }

    // -------- Cache validation -------------------------------------

    #[inline]
    fn validate_dep_signature(&self, signature: &DepSignature) -> bool {
        self.inner.validate_dep_signature(signature)
    }

    // -------- Component-meta-tier bridges --------------------------

    #[inline]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        self.inner
            .current_dependency_fact_versions(canonical, tracked_deps)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        ResolverContext::get_raw_analysis_snapshot(self.inner, canonical)
    }

    // -------- Session view (overlay-aware) -------------------------

    #[inline]
    fn view(&self) -> Box<dyn SessionView + '_> {
        Box::new(BoundSessionViewRef { inner: self.view })
    }

    // -------- Fact tracer ------------------------------------------

    #[inline]
    fn current_fact_tracer(&self) -> Option<&FactReadSetCell> {
        self.inner.current_fact_tracer()
    }

    // -------- Overlay-aware session view ---------------------------

    #[inline]
    fn active_session_view(&self) -> Option<&dyn SessionView> {
        Some(self.view)
    }

    // -------- Overlay materialiser ---------------------------------

    #[inline]
    fn materialize_overlay_indexed_ready(
        &self,
        canonical_id: &str,
        overlay_source: &Arc<str>,
        overlay_whole_hash: Hash16,
    ) -> Option<Arc<IndexedReady>> {
        self.inner.materialize_overlay_indexed_ready(
            canonical_id,
            overlay_source,
            overlay_whole_hash,
        )
    }
}

/// Borrow adapter that lets [`SessionResolverContext::view`] return a
/// `Box<dyn SessionView + '_>` over a `&dyn SessionView` without
/// requiring the caller to own a clonable view.
struct BoundSessionViewRef<'a> {
    inner: &'a dyn SessionView,
}

impl<'a> SessionView for BoundSessionViewRef<'a> {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        self.inner.source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        self.inner.content_hash_for(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        self.inner.parse_artifacts(canonical)
    }

    fn project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
        self.inner.project_identity()
    }

    fn env_hashes(&self) -> &crate::session_view::EnvHashes {
        self.inner.env_hashes()
    }

    fn is_tombstoned(&self, canonical: &str) -> bool {
        self.inner.is_tombstoned(canonical)
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        self.inner.resolved_import_facts(canonical)
    }

    fn fingerprint(&self) -> u64 {
        self.inner.fingerprint()
    }

    fn overlay_canonicals(&self) -> Vec<String> {
        self.inner.overlay_canonicals()
    }
}
