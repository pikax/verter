//! Session-bound [`ResolverContext`] wrapper that surfaces a
//! [`SessionView`](crate::session_view::SessionView) to resolver-tier
//! consumers without forking the trait.
//!
//! The base `impl ResolverContext for VerterHost` returns
//! `active_session_view() = None` and the base per-canonical /
//! per-domain `StoreView` snapshots. Session-bearing query paths
//! construct [`SessionResolverContext`] over `(inner, view)` so the
//! same trait methods become overlay-aware: the wrapper overrides
//! `active_session_view()` to return `Some(view)` and overrides the
//! `resolver_store_view` / `shallow_file_state` / `ensure_indexed_ready`
//! family so an overlay-bearing canonical pins against the overlay
//! content hash, while every other method delegates to the inner
//! [`ResolverContext`].
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
use verter_workspace::{AmbientSymbolHit, ProjectStableKey};

use crate::host_manage::ValueDeclIdentity;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::project_type_store::{IndexedReady, ProjectTypeStore};
use crate::request_context::bump_resolver_store_view_call;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_core::resolver_context::ResolverContext;
use crate::resolver_core::StoreView;
use crate::resolver_core::{FactReadSetCell, FactVersionRef, ShallowFileState};
use crate::resolver_store::HostStoreView;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::session_view::SessionView;
use crate::types::Hash16;
use crate::FileAnalysisSnapshot;
use crate::HostConfig;

/// Session-bound [`ResolverContext`] wrapper.
///
/// Delegates every `ResolverContext` method to the inner
/// [`crate::VerterHost`] except [`ResolverContext::active_session_view`]
/// (returns `Some(view)`) and the overlay-aware overrides on
/// [`ResolverContext::store_view`],
/// [`ResolverContext::shallow_file_state`],
/// [`ResolverContext::ensure_indexed_ready`], etc. Resolver-tier
/// helpers that consult `active_session_view()` (or the per-method
/// overrides above) for overlay-aware reads observe the session view
/// via this wrapper without changing the trait surface or the call-
/// site signature in [`ProjectSemanticDispatch::new`].
///
/// `inner` is the concrete host so the wrapper can reach
/// view-aware internals (e.g.,
/// [`crate::VerterHost::prepared_decl_bundle_with_context`]) that the
/// `ResolverContext` trait surface itself does not expose.
///
/// The owned [`RequestStoreView`] field chains a
/// [`CanonicalCompletionOverlay`] in front of an overlay-rooted base
/// [`HostStoreView`] built ONCE at the request boundary via
/// [`crate::VerterHost::resolver_store_view`] followed by
/// [`HostStoreView::with_session_overlay`]. Per the 6.c per-request
/// view-hoisting rail the overlay re-rooting runs exactly once per
/// session-bearing request, not per resolver method call. Canonicals
/// loaded mid-request through `ensure_loaded` / `ensure_indexed_ready`
/// are promoted into the completion overlay so the self-root validator
/// observes them on subsequent reads.
pub(crate) struct SessionResolverContext<'a> {
    inner: &'a crate::VerterHost,
    view: &'a dyn SessionView,
    request_view: RequestStoreView<'a>,
}

impl<'a> SessionResolverContext<'a> {
    /// Construct a session-bound wrapper over `(inner, view, base, overlay)`.
    ///
    /// `base` MUST be an overlay-rooted view — typically
    /// `host.resolver_store_view().with_session_overlay(host, view)`.
    /// The caller owns `base`; this wrapper borrows it. The completion
    /// overlay is owned via `Arc` so cooperative-admission lanes can
    /// share it. The borrow shape matches
    /// [`ProjectSemanticDispatch::new`]: callers create the wrapper on
    /// the stack, pass `&wrapper` to the dispatcher, and drop it at
    /// the end of the query. The wrapper does not retain references
    /// after the call returns.
    #[must_use]
    pub(crate) fn new(
        inner: &'a crate::VerterHost,
        view: &'a dyn SessionView,
        base: &'a HostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
    ) -> Self {
        Self {
            inner,
            view,
            request_view: RequestStoreView::new(base, overlay),
        }
    }

    /// Borrow the request-scoped overlay.
    ///
    /// Cooperative-admission lanes that inherit the context call this
    /// to clone the `Arc` and seed a sibling wrapper that shares the
    /// same per-request completion state.
    #[allow(dead_code)]
    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
        self.request_view.overlay()
    }

    /// Idempotently promote a newly-loaded canonical into the overlay
    /// (epoch-guarded; codex refinement #5).
    ///
    /// Called from `ensure_loaded` / `ensure_indexed_ready` success
    /// paths so subsequent self-root fact validation observes the
    /// freshly-loaded canonical's current content rather than
    /// false-missing because the request-entry base view did not track
    /// it.
    #[allow(dead_code)]
    pub(crate) fn complete_canonical(&self, canonical: &str) {
        self.request_view
            .overlay()
            .complete_canonical_with_session_view(
                self.inner,
                self.request_view.base(),
                self.view,
                canonical,
            );
    }
}

impl<'a> ResolverContext for SessionResolverContext<'a> {
    // -------- Identity --------------------------------------------

    #[inline]
    fn is_request_bound(&self) -> bool {
        true
    }

    // -------- Cache accessors --------------------------------------

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
        // Route through the host's view-aware variant so an
        // overlay-bearing view observes overlay-content-rooted
        // prepared declarations rather than the host's base bundle
        // cache. Non-overlay canonicals fall through to the warm
        // bundle cache transparently.
        self.inner
            .prepared_decl_bundle_with_context(self, canonical_id)
    }

    #[inline]
    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        self.inner
            .prepared_type_decl_with_context(self, canonical_id, symbol_name)
    }

    #[inline]
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        self.inner
            .prepared_value_decl_with_context(self, canonical_id, symbol_name)
    }

    #[inline]
    fn ensure_indexed_ready(&self, canonical_id: &str) -> Option<Arc<IndexedReady>> {
        // Overlay-priority hook: the session view governs whether the
        // base host's IndexedReady is acceptable for this query. The
        // host's own ensure-loaded already covers the non-overlay
        // case; overlay-aware materialisation routes through the
        // shared helper in `host_manage::overlay_priority`.
        let result = crate::host_manage::overlay_priority::ensure_indexed_ready_with_view(
            self.inner,
            self.view,
            canonical_id,
        );
        if result.is_some() {
            // Eager canonical completion (codex refinement #3 / #5):
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
        let loaded = crate::host_manage::overlay_priority::ensure_loaded_with_view(
            self.inner,
            self.view,
            canonical_id,
        );
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
        ResolverContext::external_type_analysis(self.inner, canonical_id)
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        // Thread `self` (the session context) as the current-content
        // oracle so the pinned `IndexedReady` read and the route-owned
        // fallback both observe the active overlay's content identity —
        // an overlay-covered dependency pins against the overlay hash,
        // not the base host's.
        self.inner
            .shallow_file_state_with_context(self, canonical_id)
    }

    #[inline]
    fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        ResolverContext::local_type_declaration_id(self.inner, canonical_source, resolved_name)
    }

    #[inline]
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        ResolverContext::get_whole_hash(self.inner, canonical)
    }

    /// Overlay-aware authoritative current-content hash.
    ///
    /// The base-host default ([`ResolverContext::authoritative_current_content_hash`])
    /// reads the scheduler's `parse.whole_hash`, which is the *base*
    /// content hash. When this context wraps an overlay over an
    /// existing base file, the overlay `IndexedReady` candidate was
    /// prewarmed under the *overlay* content hash — so a content-pinned
    /// read keyed by the base hash would miss the overlay candidate (or
    /// resolve the stale base artifact) while computing overlay
    /// component-meta / proof data.
    ///
    /// Resolution order:
    /// 1. An overlay-Upsert covering `canonical` →
    ///    [`SessionView::overlay_content_hash_for`] (the overlay
    ///    source's hash). Authoritative for this session.
    /// 2. A session tombstone for `canonical` → `None` (the session
    ///    deleted the file; there is no current content).
    /// 3. Otherwise → the base-host authoritative scheduler hash, with
    ///    the same no-`get_any` guarantee as the base default.
    #[inline]
    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        if let Some(overlay_hash) = self.view.overlay_content_hash_for(canonical) {
            return Some(overlay_hash);
        }
        if self.view.is_tombstoned(canonical) {
            return None;
        }
        self.inner.authoritative_current_content_hash(canonical)
    }

    /// Overlay-aware materialize-scope observation.
    ///
    /// The base-host [`crate::VerterHost::observe_materialize_scope`]
    /// resolves the scope's `IndexedReady` from the scheduler /
    /// artifact-current authorities — the *base* content. When this
    /// context wraps an overlay over `canonical`, the materialiser must
    /// observe the OVERLAY `IndexedReady`: the overlay candidate was
    /// prewarmed under the overlay content hash, so lowering /
    /// signature-rooting under the base hash would mis-identify the
    /// version actually being materialised.
    ///
    /// Resolution order:
    /// 1. An overlay-Upsert covering `canonical` →
    ///    [`SessionView::overlay_content_hash_for`] gives the overlay
    ///    content hash; the overlay `IndexedReady` is materialised /
    ///    fetched via the overlay-priority `ensure_indexed_ready` path
    ///    and pinned to that exact hash — **no base fallback**. If the
    ///    overlay candidate cannot be recovered at the overlay hash,
    ///    `None` (refuse admission rather than observe the base).
    /// 2. A session tombstone for `canonical` → `None` (the session
    ///    deleted the file; there is no current content).
    /// 3. Otherwise → the base-host
    ///    [`crate::VerterHost::observe_materialize_scope`].
    #[inline]
    fn observe_materialize_scope(
        &self,
        canonical: &str,
    ) -> Option<crate::resolver_core::MaterializeScopeObservation> {
        if let Some(overlay_hash) = self.view.overlay_content_hash_for(canonical) {
            // Materialise / fetch the overlay candidate, then read it
            // back — no base fallback. Overlay detection
            // (`overlay_content_hash_for`) and the materialise step
            // (`ensure_indexed_ready`) operate on the RAW `canonical`
            // because the `SessionView` overlay maps are raw-keyed. The
            // store read then goes through `OverlayArtifactIdentity`,
            // which builds the exact key the overlay materialiser
            // published under — the raw-owner overlay hash +
            // discriminator with the NORMALISED `analysis_canonical` as
            // `FileArtifactKey.canonical`. Keying the read on the raw
            // `canonical` directly would miss the publish whenever
            // `normalize(raw) != raw` (a `.js` with a `.d.ts`
            // companion). `lookup_overlay_artifacts` stays off the base
            // artifact — a real divergence when the overlay bytes are
            // identical to the base.
            let _ = ResolverContext::ensure_indexed_ready(self, canonical);
            let identity = self.inner.overlay_artifact_identity(canonical);
            let indexed = Arc::clone(
                &identity
                    .lookup_overlay_artifacts(self.inner, self.view)?
                    .indexed,
            );
            let syntactic_export_set =
                crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
                    self,
                    canonical,
                    overlay_hash,
                    verter_semantic::facts::FactKey::SyntacticExportSet,
                    verter_semantic::facts::FactLane::Semantic,
                );
            return Some(crate::resolver_core::MaterializeScopeObservation {
                canonical_id: Arc::from(canonical),
                indexed,
                syntactic_export_set,
            });
        }
        if self.view.is_tombstoned(canonical) {
            return None;
        }
        self.inner.observe_materialize_scope(canonical)
    }

    /// Overlay-aware content-pinned [`IndexedReady`] lookup.
    ///
    /// The default [`ResolverContext::indexed_for_current_content`]
    /// resolves the pin hash via [`Self::authoritative_current_content_hash`]
    /// (overlay-aware here) but reads the artifact store via
    /// `get_for_current_content` — the **legacy** key. When this
    /// context's view carries an explicit overlay for `canonical`, the
    /// overlay `IndexedReady` was published under an `overlay_scoped`
    /// key (overlay content hash + overlay-set discriminator); a
    /// legacy-key read would miss it, or — when the overlay bytes are
    /// identical to the base — return the BASE artifact, whose import
    /// routes diverge from the overlay's (the overlay can resolve an
    /// overlay-only relative helper the base cannot). This override
    /// reads the overlay candidate through `get_overlay_scoped` so the
    /// content-pinned read stays on the session's own artifact; an
    /// unmasked canonical keeps the base legacy-key read.
    #[inline]
    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedReady>> {
        if self.view.overlay_content_hash_for(canonical).is_some() {
            // Overlaid canonical. Overlay detection
            // (`overlay_content_hash_for`) keys on the RAW `canonical`
            // — the `SessionView` overlay maps are raw-keyed. The
            // artifact-store read routes through `OverlayArtifactIdentity`,
            // which rebuilds the exact key the overlay materialiser
            // published under: the raw-owner overlay hash + discriminator
            // with the NORMALISED `analysis_canonical` as
            // `FileArtifactKey.canonical`. A read keyed directly on the
            // raw `canonical` would miss the publish whenever
            // `normalize(raw) != raw` (a `.js` with a `.d.ts`
            // companion). `lookup_overlay_artifacts` handles both the
            // `overlay_scoped` (discriminator present) and `legacy`
            // (base-passthrough) key shapes.
            let identity = self.inner.overlay_artifact_identity(canonical);
            return identity
                .lookup_overlay_artifacts(self.inner, self.view)
                .map(|facts| Arc::clone(&facts.indexed));
        }
        if self.view.is_tombstoned(canonical) {
            return None;
        }
        // Unmasked canonical: the base host's content-pinned read
        // (scheduler authoritative hash → legacy-key artifact).
        self.inner.current_content_pinned_indexed(canonical)
    }

    /// Owned-view variant — preserves the pre-6.c semantics of building
    /// a fresh overlay-rooted snapshot per call. Retained for cold-path
    /// callers that need an owned snapshot; production hot-path callers
    /// should prefer [`Self::store_view`] (the borrow into the
    /// request-bound view with shadowing completion overlay) for
    /// zero-allocation cache-validity reads.
    #[inline]
    #[track_caller]
    fn resolver_store_view(&self) -> HostStoreView {
        bump_resolver_store_view_call();
        ResolverContext::resolver_store_view(self.inner).with_session_overlay(self.inner, self.view)
    }

    /// Borrowed access to the request-bound overlay-rooted store view
    /// chained behind a per-request
    /// [`CanonicalCompletionOverlay`].
    ///
    /// The base [`HostStoreView`] is built ONCE at the request boundary
    /// — `host.resolver_store_view().with_session_overlay(host, view)`
    /// — and threaded into this wrapper by reference. The overlay
    /// re-rooting therefore runs exactly once per session-bearing
    /// request, not per resolver method call.
    ///
    /// Identity semantics of `with_session_overlay` are preserved: an
    /// overlay-Upsert canonical's per-canonical / per-domain
    /// (`whole_hashes`, `file_facts`, `derived_hashes`) snapshots are
    /// re-rooted at the session's overlay content; a session-tombstoned
    /// canonical's snapshots are dropped and the canonical is recorded
    /// in [`HostStoreView`]'s `tombstoned_canonicals` set so the strict
    /// validators reject any warm entry rooted on the deleted file.
    ///
    /// The chained [`CanonicalCompletionOverlay`] shadows the base view
    /// with any canonicals loaded mid-request — additive loads observed
    /// through `ensure_loaded` / `ensure_indexed_ready` successes.
    /// Shadowing reads are authoritative: a mismatched overlay value
    /// rejects (no fallthrough to the base view).
    #[inline]
    fn store_view(&self) -> &dyn StoreView {
        &self.request_view
    }

    #[inline]
    fn project_type_store(&self) -> &Arc<ProjectTypeStore> {
        ResolverContext::project_type_store(self.inner)
    }

    #[inline]
    fn config(&self) -> &HostConfig {
        ResolverContext::config(self.inner)
    }

    // -------- Symbol / route resolution ----------------------------

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        // Route through the view-bound shim so the cached imported-root
        // entry validates against the session-bound overlay-aware view
        // (`self.request_view` — overlay + session reads layered over
        // base). Avoids the carrier-site bare-host rebuild.
        self.inner.resolve_imported_type_root_with_store_view(
            &self.request_view,
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
        self.inner.resolve_named_type_export_target_with_store_view(
            &self.request_view,
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
        self.inner
            .resolve_named_type_export_target_shallow_with_store_view(
                &self.request_view,
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
        // Carrier site closure: route through the view-bound variant so
        // the cached owner-import surface validates against the
        // session-bound view rather than rebuilding a fresh snapshot.
        self.inner.resolve_owner_direct_import_with_store_view(
            &self.request_view,
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
        ResolverContext::resolve_type_dependency_canonical(
            self.inner,
            owner_canonical,
            import_source,
        )
    }

    #[inline]
    fn route_owned_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        ResolverContext::route_owned_shallow_state(self.inner, canonical_id)
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        // Route through the context-aware variant so the
        // `HostComponentMetaResolver` walker constructed inside binds
        // to the session-bound view (this `SessionResolverContext`)
        // rather than the bare-host context that delegating to
        // `self.inner` would synthesize.
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
        ResolverContext::resolve_value_export_target(self.inner, dep_canonical_id, imported_name)
    }

    // -------- Ambient resolution -----------------------------------

    #[inline]
    fn lookup_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        ResolverContext::lookup_ambient_symbol(self.inner, consumer_project, symbol)
    }

    #[inline]
    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str) {
        ResolverContext::record_ambient_dependency(self.inner, consumer_canonical, virtual_id)
    }

    #[inline]
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        ResolverContext::workspace_is_workspace_owned(self.inner, canonical_id)
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        ResolverContext::workspace_is_package_backed(self.inner, canonical_id)
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
        ResolverContext::dispatch_node_data(self.inner, node)
    }

    // -------- Component-meta-tier bridges --------------------------

    #[inline]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        ResolverContext::current_dependency_fact_versions(self.inner, canonical, tracked_deps)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        ResolverContext::get_raw_analysis_snapshot(self.inner, canonical)
    }

    // -------- Fact tracer ------------------------------------------

    #[inline]
    fn current_fact_tracer(&self) -> Option<&FactReadSetCell> {
        ResolverContext::current_fact_tracer(self.inner)
    }

    // -------- Overlay-aware session view ---------------------------

    #[inline]
    fn active_session_view(&self) -> Option<&dyn SessionView> {
        Some(self.view)
    }

    #[inline]
    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost {
        self.inner
    }
}
