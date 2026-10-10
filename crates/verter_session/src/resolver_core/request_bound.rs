//! The session's request-bound resolver contexts: the direct-host test seam and the
//! request-bound adapter that serves the engine's resolver ports from a lifecycle.
use verter_session_query::facts::reuse::NonCacheableReadReason;
use verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out;

use std::sync::Arc;

use verter_session_query::declarations::DeclarationId;
use verter_session_query::resolution::{AmbientSymbolHit, ProjectStableKey};
use verter_session_query::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use verter_session_query::inputs::indexed::IndexedInputRecord;
use verter_session_query::inputs::indexed::IndexedInputServe;
use verter_session_query::inputs::prepared::PreparedInputRecord;
use verter_session_query::inputs::shallow::ShallowInputRecord;
use verter_type_engine::resolver_core::request_ports::{
    ExecutionSubmission, IndexedInputs, RouteLookup,
};
use verter_type_engine::resolver_core::resolver_context::*;

use crate::project_type_store::IndexedReady;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::ShallowFileState;
use verter_session_query::declarations::metadata::ValueDeclIdentity;
use verter_type_engine::fact_tracing::tracing as tracer_stack;

use verter_session_query::analysis::file_analysis::FileAnalysisSnapshot;
use verter_session_query::analysis::types::Hash16;

/// The capability types every host request context hands out: the source's
/// owned expression-demand handle and the host's live workspace slot. Base
/// and overlay contexts share them, so request structures carrying them are
/// instantiated once.
pub enum HostCapabilities {}

impl ResolverCapabilities for HostCapabilities {
    type ExpressionDemand = crate::host_source_demand::HostExpressionDemand;
    type Clocks = crate::resolver_store::WorkspaceSlotClocks;
    type HostAttachment = crate::session_attachment::SessionAttachment;
    type MacroMirrors = super::request_inputs::MacroMirrorSelector;
}

#[cfg(any(test, feature = "test-support"))]
impl sealed::Sealed for crate::VerterHost {}

impl<'a> sealed::Sealed for crate::resolver_core::host_resolver_context::HostResolverContext<'a> {}

impl<'a> sealed::Sealed
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
{
}

impl<'a> sealed::RequestBoundSealed
    for crate::resolver_core::host_resolver_context::HostResolverContext<'a>
{
}

#[cfg(any(test, feature = "test-support"))]
impl sealed::RequestBoundSealed for crate::VerterHost {}

impl<'a> sealed::RequestBoundSealed
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
{
}

impl<'a> RequestBoundResolverContext<HostCapabilities>
    for crate::resolver_core::host_resolver_context::HostResolverContext<'a>
{
}

impl<'a> RequestBoundResolverContext<HostCapabilities>
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
{
}

#[cfg(not(any(test, feature = "test-support")))]
static_assertions::assert_not_impl_any!(crate::VerterHost: ResolverContext<HostCapabilities>);

#[cfg(any(test, feature = "test-support"))]
static_assertions::assert_impl_all!(crate::VerterHost: ResolverContext<HostCapabilities>);

/// Test-only direct-host seam. Production builds compile this implementation
/// out, so every production `ResolverContext` is structurally request-bound.
#[cfg(any(test, feature = "test-support"))]
impl ResolverContext<HostCapabilities> for crate::VerterHost {}

#[cfg(any(test, feature = "test-support"))]
impl IndexedInputs for crate::VerterHost {
    fn operand_env_epoch(
        &self,
    ) -> verter_type_engine::resolver_core::request_ports::OperandEnvEpoch {
        let w = self.workspace();
        verter_type_engine::resolver_core::request_ports::OperandEnvEpoch::new(
            w.published_root(),
            w.content_generation(),
        )
    }
    fn project_stable_key_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::resolution::ProjectStableKey> {
        self.workspace()
            .project_stable_key(crate::VerterHost::resolve_project_for_canonical(
                self, canonical,
            )?)
    }
    fn captured_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        crate::VerterHost::resolver_store_view(self)
            .into_owned_view()
            .project_identity_for(canonical)
    }
    fn host_view_env_hashes(&self) -> crate::session_view::EnvHashes {
        crate::VerterHost::host_view_env_hashes(self)
    }
    fn host_view_env_hashes_for(&self, canonical: &str) -> crate::session_view::EnvHashes {
        crate::VerterHost::host_view_env_hashes_for(self, canonical)
    }
    fn host_view_project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
        crate::VerterHost::host_view_project_identity(self)
    }
    fn host_view_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        crate::VerterHost::host_view_project_identity_for(self, canonical)
    }
    fn semantic_compiler_options_for(
        &self,
        canonical: &str,
    ) -> verter_session_query::resolution::SemanticCompilerOptions {
        crate::VerterHost::semantic_compiler_options_for(self, canonical)
    }
    fn resolve_project_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::resolution::ProjectId> {
        crate::VerterHost::resolve_project_for_canonical(self, canonical)
    }
    fn declaration_sequence_rank(&self, canonical: &str) -> u32 {
        crate::VerterHost::declaration_sequence_rank(self, canonical)
    }

    fn engine_policy(&self) -> verter_type_engine::project_semantic_dispatch::EnginePolicy {
        self.engine_policy.clone()
    }

    // Cache accessors -------------------------------------------------

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedInputRecord>> {
        let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
        crate::VerterHost::prepared_decl_bundle_with_store_view(self, &view, None, canonical_id)
            .map(|bundle| self.source_input_leases.retain_prepared(bundle))
    }

    #[inline]
    fn ensure_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical_id)
            .map(|serve| self.source_input_leases.retain(serve))
    }

    #[inline]
    fn base_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical_id)
            .map(|serve| self.source_input_leases.retain(serve))
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        crate::VerterHost::ensure_loaded(self, canonical_id)
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
        crate::VerterHost::shallow_file_state(self, canonical_id)
            .map(|state| self.source_input_leases.retain_shallow(state))
    }

    #[inline]
    fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        crate::VerterHost::local_type_declaration_id(self, canonical_source, resolved_name)
    }

    #[inline]
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        crate::VerterHost::get_whole_hash(self, canonical)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        crate::VerterHost::get_raw_analysis_snapshot(self, canonical)
    }
    /// Authoritative current content hash for `canonical` — the hash
    /// source [`Self::indexed_for_current_content`] pins against.
    ///
    /// Unlike [`Self::get_whole_hash`] this accessor has **no
    /// permissive fallback**: it never derives a hash from a
    /// content-agnostic `FileArtifactStore` scan
    /// (`FileArtifactStore::get_any`).
    /// When only a stale artifact could answer (the canonical was
    /// evicted/deleted while its `IndexedReady` lingers) it returns
    /// `None` so the pinned read becomes a miss rather than resolving
    /// the stale artifact via its own hash.
    ///
    /// The default impl delegates to
    /// [`crate::VerterHost::authoritative_current_content_hash`] on the
    /// concrete host — the scheduler `parse.whole_hash` gated on the
    /// `DerivedRawState` entry being non-evicted. The overlay-aware
    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`]
    /// overrides it to consult the active [`SessionView`](crate::session_view::SessionView):
    /// an overlay-covered canonical resolves to the overlay's content
    /// hash (the hash the overlay `IndexedReady` was prewarmed under),
    /// not the base host's hash.
    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        self.authoritative_current_content_hash(canonical)
    }
    /// Content-pinned [`IndexedReady`] lookup.
    ///
    /// Resolves the canonical's authoritative current content hash via
    /// [`Self::authoritative_current_content_hash`] (no `get_any`
    /// fallback; overlay-aware under `SessionResolverContext`) and
    /// reads the artifact store pinned to that hash via
    /// [`crate::file_artifact_store::FileArtifactStore::get_for_current_content`].
    /// Returns `None` when the canonical has no authoritative current
    /// content hash OR when the only cached artifact is a stale
    /// candidate for an older content hash.
    ///
    /// Correctness-sensitive readers in the seal scope —
    /// materialisation fence seeding
    /// and the component-meta proof producers
    /// (`component_meta_caches.rs`) — MUST use this instead of the
    /// permissive `project_type_store().indexed().get_any(..)`. Seeding
    /// a fence (or observing a `FileWholeHash` fact) from a stale
    /// artifact bakes the stale content hash into the cached entry's
    /// `read_set_signature`, so fact validation would later confirm a
    /// stale cache entry as valid. Resolving the pin from a `get_any`
    /// hash, or from the base host's hash while an overlay is active,
    /// reintroduces exactly that staleness — so the pin is derived
    /// strictly from the authoritative accessor above.
    ///
    /// Defaulted so the base implementer ([`crate::VerterHost`])
    /// inherits the host's pinned-read body
    /// ([`crate::VerterHost::current_content_pinned_indexed`]) — which
    /// resolves the authoritative current content hash and reads the
    /// artifact store pinned to it, keyed by the **normalised analysis
    /// canonical** so a RAW requested canonical (the architectural id
    /// before an overlay-detection point) does not mis-key for a
    /// non-identity `.js`. The overlay-aware
    /// [`crate::resolver_core::SessionResolverContext`] overrides this
    /// method: it gates the overlay branch on the raw id via
    /// [`crate::host_manage::overlay_materialize::OverlayArtifactIdentity`]
    /// and only falls through to the base host (this body) for an
    /// unmasked canonical.
    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedInputRecord>> {
        self.current_content_pinned_indexed(canonical)
            .map(|indexed| {
                self.source_input_leases
                    .retain(crate::host_manage::prepared_decl::IndexedReadyServe {
                        indexed,
                        store_published: true,
                    })
                    .indexed
            })
    }
    /// Exact artifact identity for the authority-visible current source.
    fn artifact_key_for_current_content(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::source::artifact_key::FileArtifactKey> {
        self.authoritative_current_artifact_key(canonical)
    }
    /// Establish ONE tear-free [`MaterializeScopeObservation`] for a
    /// materialize-memo scope canonical.
    ///
    /// The materialize-memo publish site needs the scope's content
    /// version for two consumers that must agree (the lowering
    /// `NodeScopeId` and the signature self-root). This accessor
    /// produces a single `Arc<IndexedInputRecord>` whose `whole_hash` roots
    /// BOTH — eliminating the two-oracle tear.
    ///
    /// Returns `None` when the scope has no recoverable *current*
    /// indexed artifact: an evicted / deleted canonical whose stale
    /// `IndexedReady` lingers, or a tombstoned overlay canonical. A
    /// `None` observation makes the publish site skip shared-cache
    /// admission while still returning the freshly-computed value.
    ///
    /// The default impl delegates to
    /// [`crate::VerterHost::observe_materialize_scope`]. The
    /// overlay-aware `SessionResolverContext` overrides it: an
    /// overlay-covered canonical is pinned to the overlay
    /// `IndexedReady` (the overlay content hash), with no base
    /// fallback; a session tombstone yields `None`; otherwise it
    /// delegates to the base host.
    fn observe_materialize_scope(&self, canonical: &str) -> Option<MaterializeScopeObservation> {
        self.observe_materialize_scope(canonical)
    }
    /// Rewrite a raw canonical to its analysis canonical — the identity
    /// every `FileArtifactStore` artifact (base and overlay) is keyed by.
    ///
    /// A raw canonical has two forms: the form the session edited /
    /// requested, and the `normalized_analysis_canonical` rewrite (a
    /// runtime `.js` whose `.d.ts` companion is the analysis target). The
    /// two coincide for an ordinary `.ts` / `.tsx` / `.d.ts` file. The
    /// overlay materialiser publishes under the normalised id, and the
    /// base [`Self::ensure_indexed_ready_serve`] normalises before publishing,
    /// so `FileArtifactKey::canonical` is always the normalised id.
    ///
    /// Content-addressed `FileArtifactStore` lookups (parse-fact
    /// recovery in particular) MUST normalise the canonical before
    /// keying the store — a raw-keyed lookup misses the artifact
    /// whenever `normalize(raw) != raw`. The default impl delegates to
    /// [`crate::VerterHost::normalized_analysis_canonical`]; every context
    /// resolves through the same host method.
    fn normalized_analysis_canonical(&self, raw_canonical: &str) -> String {
        crate::VerterHost::normalized_analysis_canonical(self, raw_canonical).into_owned()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl RouteLookup for crate::VerterHost {
    fn reverse_dependency_canonicals(&self, canonical: &str) -> Vec<String> {
        self.workspace().reverse_deps_for(canonical)
    }
    fn observe_owner_import_route_witness(&self, canonical: &str) {
        crate::VerterHost::observe_owner_import_route_witness(self, canonical);
    }

    // Symbol / route resolution --------------------------------------

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<verter_session_query::type_solver::ResolvedRootIdentity> {
        let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
        crate::VerterHost::resolve_imported_type_root_with_store_view(
            self,
            &view,
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
        Option<verter_session_query::type_solver::ResolvedRootIdentity>,
        Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>,
    ) {
        let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
        crate::VerterHost::resolve_imported_type_root_with_facts_with_store_view(
            self,
            self,
            None,
            &view,
            dep_canonical,
            imported_name,
        )
    }

    #[inline]
    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
        crate::VerterHost::resolve_named_type_export_target_shallow_with_store_view(
            self,
            self,
            &view,
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
        let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
        crate::VerterHost::resolve_owner_direct_import_with_store_view(
            self,
            self,
            None,
            &view,
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
        type_route_answer(crate::VerterHost::resolve_type_dependency_canonical(
            self,
            owner_canonical,
            import_source,
        ))
    }

    #[inline]
    fn routed_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_session_query::inputs::shallow::ShallowInputRecord>> {
        crate::VerterHost::routed_shallow_state(self, canonical_id)
            .map(|state| self.source_input_leases.retain_shallow(state))
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        requested_name: &str,
    ) -> verter_session_query::declarations::metadata::ResolvedTypeDeclaration {
        crate::host_manage::jsdoc_resolve::resolve_type_declaration_with_context(
            self,
            self,
            dep_canonical,
            owner,
            requested_name,
        )
    }

    #[inline]
    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        crate::VerterHost::resolve_value_export_target(self, dep_canonical_id, imported_name)
    }

    // Ambient resolution (narrow capabilities) -----------------------

    #[inline]
    fn lookup_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        self.workspace()
            .lookup_ambient_symbol(consumer_project, symbol)
    }

    #[inline]
    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str) {
        self.workspace()
            .record_ambient_dependency(consumer_canonical, virtual_id);
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        self.workspace().is_package_backed(canonical_id)
    }
}

#[cfg(any(test, feature = "test-support"))]
/// Session-side ownership classification read beside the engine's route port.
#[cfg(test)]
impl crate::VerterHost {
    /// Whether `canonical_id` is workspace-owned per the workspace's
    /// resolver-classification (NOT a path-substring check on
    /// `node_modules`). True for workspace package sources, including
    /// pnpm-symlink hops whose realpath resolves into a workspace project,
    /// and workspace-linked packages that happen to live under
    /// `node_modules/`.
    pub(crate) fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.workspace().is_workspace_owned(canonical_id)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ExecutionSubmission for crate::VerterHost {
    type MacroMirrors = super::request_inputs::MacroMirrorSelector;
    fn attach_engine(
        &self,
    ) -> verter_type_engine::project_semantic_dispatch::EngineBinding<Self::MacroMirrors> {
        self.project_type_store().bind_engine(
            self.engine_observers(),
            self.source_input_leases.macro_selector(
                #[cfg(any(test, feature = "test-support"))]
                Arc::clone(&self.test_force.engine),
            ),
        )
    }
}

#[cfg(any(test, feature = "test-support"))]
impl verter_type_engine::resolver_core::request_ports::HostAttachmentPort for crate::VerterHost {
    type HostAttachment = crate::session_attachment::SessionAttachment;
    fn host_attachment(&self) -> &Self::HostAttachment {
        self.session_attachment()
    }
}

/// What distinguishes one request-bound lifecycle from another.
///
/// Both request-bound contexts hold a `&VerterHost` plus a per-request
/// [`RequestStoreView`](crate::resolver_core::RequestStoreView); every
/// `ResolverContext` method that only threads those two is implemented
/// ONCE on [`RequestBoundAdapter`]. The methods below are the residue
/// that genuinely differs between a base request and a session-overlay
/// request. Every hook has a base-host default; the session lifecycle
/// overrides the overlay-aware ones.
///
/// Hooks that must re-enter the resolver tier receive the enclosing
/// adapter as `ctx: &dyn ResolverContext` so the re-entry binds to the
/// same request-bound context.
pub(crate) trait RequestBoundLifecycle {
    /// The host this request runs against.
    fn host(&self) -> &crate::VerterHost;

    /// The request-bound view (base view chained behind the request's
    /// [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)).
    fn request_view(&self) -> &crate::resolver_core::RequestStoreView<'_>;

    /// The snapshot captured when this request was admitted.
    fn request_snapshot(
        &self,
    ) -> &verter_type_engine::resolver_core::RequestSnapshot<
        crate::resolver_store::WorkspaceSlotClocks,
    >;

    /// The active session view, if this lifecycle carries one.
    fn session_view(&self) -> Option<&dyn crate::session_view::SessionView>;

    /// The request overlay this lifecycle resolves through; `None`
    /// resolves through the workspace view.
    fn resolution_overlay(&self) -> Option<&verter_workspace::ResolutionOverlaySnapshot> {
        None
    }

    /// Idempotently promote a newly-loaded canonical into the request
    /// overlay (epoch-guarded); the session lifecycle threads its view.
    fn complete_canonical(&self, canonical: &str);

    fn prepared_decl_bundle(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
        canonical_id: &str,
    ) -> Option<Arc<PreparedDeclBundle>>;

    fn prepared_type_decl(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
    >;

    fn prepared_value_decl(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
    >;

    /// Materialise (or warm-read) the canonical artifact. The adapter
    /// performs the canonical completion on success.
    fn materialize_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self.host(), canonical_id)
    }

    /// Load the canonical. The adapter performs the canonical completion
    /// on success.
    fn load(&self, canonical_id: &str) -> bool {
        crate::VerterHost::ensure_loaded(self.host(), canonical_id)
    }

    fn shallow_file_state(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
        canonical_id: &str,
    ) -> Option<Arc<ShallowFileState>> {
        self.host().shallow_file_state_with_context(
            ctx,
            crate::host_manage::prepared_decl::SourceRequestServices {
                session_view: self.session_view(),
                completion_overlay: Some(self.request_view().overlay()),
                base_view: Some(self.request_view().base()),
            },
            canonical_id,
        )
    }

    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        self.host().authoritative_current_content_hash(canonical)
    }

    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedReady>> {
        self.host().current_content_pinned_indexed(canonical)
    }

    fn artifact_key_for_current_content(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::source::artifact_key::FileArtifactKey> {
        self.host().authoritative_current_artifact_key(canonical)
    }

    fn observe_materialize_scope(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
        canonical: &str,
    ) -> Option<MaterializeScopeObservation> {
        self.host()
            .observe_materialize_scope_with_context(ctx, canonical)
    }

    fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        type_route_answer(crate::VerterHost::resolve_type_dependency_canonical(
            self.host(),
            owner_canonical,
            import_source,
        ))
    }
}

/// The request-bound [`ResolverContext`] carrier.
///
/// `HostResolverContext<'a>` and `SessionResolverContext<'a>` are this
/// struct over their respective lifecycles; the `ResolverContext`
/// implementation below is written once and threads
/// [`RequestBoundLifecycle::request_view`] (the request-bound view) into
/// every view-aware host entry, so both lifecycles validate warm caches
/// against the view built at their request boundary.
pub struct RequestBoundAdapter<L>(pub(super) L);

/// Compile-time ARITY pin for the request-port carrier: the destructuring
/// pattern below is exhaustive over [`RequestBoundAdapter`]'s field set, so a
/// SECOND field on the carrier is a build error here.
///
/// What actually holds the "the engine holds no host, store or config field"
/// boundary is NOT this witness. It is (1) the carrier's single field being
/// `pub(super)`, so nothing outside this module can name or read it, and (2)
/// the engine holding only `&dyn ResolverContext` — the sealed super-trait
/// surface — which gives engine-tier code no path to the carrier at all. The
/// lifecycle this carrier wraps DOES expose
/// [`RequestBoundLifecycle::host`], and the adapter's port impls call it: the
/// host stays on the session side of the adapter, behind the five ports.
#[cfg(test)]
mod adapter_field_set_witness {
    use super::RequestBoundAdapter;

    /// Exhaustive over the carrier's field set — adding a field breaks the
    /// build.
    fn lifecycle_only<L>(adapter: RequestBoundAdapter<L>) -> L {
        let RequestBoundAdapter(lifecycle) = adapter;
        lifecycle
    }

    #[test]
    fn adapter_carries_exactly_one_field() {
        // The value of this test is that it COMPILES: the body asserts
        // nothing at runtime. See the module doc for the rails that keep
        // the field out of the engine's reach.
        fn accepts_projection<L>(_: fn(RequestBoundAdapter<L>) -> L) {}
        accepts_projection::<u8>(lifecycle_only::<u8>);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<L> RequestBoundAdapter<L> {
    pub(crate) fn has_session_view_for_tests(&self) -> bool
    where
        L: RequestBoundLifecycle,
    {
        self.0.session_view().is_some()
    }
}

#[cfg(test)]
impl<L> RequestBoundAdapter<L> {
    pub(crate) fn complete_canonical(&self, canonical: &str)
    where
        L: RequestBoundLifecycle,
    {
        self.0.complete_canonical(canonical);
    }
}

impl<L: RequestBoundLifecycle> ResolverContext<HostCapabilities> for RequestBoundAdapter<L> where
    Self: sealed::Sealed + sealed::RequestBoundSealed
{
}

impl<L: RequestBoundLifecycle> IndexedInputs for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn operand_env_epoch(
        &self,
    ) -> verter_type_engine::resolver_core::request_ports::OperandEnvEpoch {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::operand_env_epoch");
        let w = self.0.host().workspace();
        verter_type_engine::resolver_core::request_ports::OperandEnvEpoch::new(
            w.published_root(),
            w.content_generation(),
        )
    }
    fn project_stable_key_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::resolution::ProjectStableKey> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::project_stable_key_for_canonical"
        );
        self.0
            .host()
            .workspace()
            .project_stable_key(self.0.host().resolve_project_for_canonical(canonical)?)
    }
    fn captured_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::captured_project_identity_for"
        );
        self.0.request_view().base().project_identity_for(canonical)
    }
    fn host_view_env_hashes(&self) -> crate::session_view::EnvHashes {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::host_view_env_hashes");
        self.0.host().host_view_env_hashes()
    }
    fn host_view_env_hashes_for(&self, canonical: &str) -> crate::session_view::EnvHashes {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::host_view_env_hashes_for");
        self.0.host().host_view_env_hashes_for(canonical)
    }
    fn host_view_project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::host_view_project_identity"
        );
        self.0.host().host_view_project_identity()
    }
    fn host_view_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::host_view_project_identity_for"
        );
        self.0.host().host_view_project_identity_for(canonical)
    }
    fn semantic_compiler_options_for(
        &self,
        canonical: &str,
    ) -> verter_session_query::resolution::SemanticCompilerOptions {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::semantic_compiler_options_for"
        );
        self.0.host().semantic_compiler_options_for(canonical)
    }
    fn resolve_project_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::resolution::ProjectId> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::resolve_project_for_canonical"
        );
        self.0.host().resolve_project_for_canonical(canonical)
    }
    fn declaration_sequence_rank(&self, canonical: &str) -> u32 {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::declaration_sequence_rank"
        );
        self.0.host().declaration_sequence_rank(canonical)
    }

    fn engine_policy(&self) -> verter_type_engine::project_semantic_dispatch::EnginePolicy {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::engine_policy");
        self.0.host().engine_policy.clone()
    }

    fn normalized_analysis_canonical(&self, raw_canonical: &str) -> String {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::normalized_analysis_canonical"
        );
        self.0
            .host()
            .normalized_analysis_canonical(raw_canonical)
            .into_owned()
    }

    #[inline]
    fn is_request_bound(&self) -> bool {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::is_request_bound");
        true
    }

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedInputRecord>> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::prepared_decl_bundle");
        self.0
            .prepared_decl_bundle(self, canonical_id)
            .map(|bundle| {
                self.0
                    .request_view()
                    .overlay()
                    .input_artifacts
                    .retain_prepared(bundle)
            })
    }

    #[inline]
    fn ensure_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::ensure_indexed_ready_serve"
        );
        let result = self.0.materialize_indexed_ready_serve(canonical_id);
        if result.is_some() {
            // Eager canonical completion, idempotent + epoch-guarded.
            self.0.complete_canonical(canonical_id);
        }
        result.map(|serve| {
            self.0
                .request_view()
                .overlay()
                .input_artifacts
                .retain(serve)
        })
    }

    /// Base-store read, deliberately NOT the request view's overlay-priority
    /// candidate: the consumer-boundary span slicer publishes from the
    /// base-store artifact, and the request view must not re-point that text
    /// at an overlay candidate.
    #[inline]
    fn base_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::base_indexed_ready_serve");
        crate::VerterHost::ensure_indexed_ready_serve(self.0.host(), canonical_id).map(|serve| {
            self.0
                .request_view()
                .overlay()
                .input_artifacts
                .retain(serve)
        })
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::ensure_loaded");
        let loaded = self.0.load(canonical_id);
        if loaded {
            self.0.complete_canonical(canonical_id);
        }
        loaded
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::shallow_file_state");
        self.0.shallow_file_state(self, canonical_id).map(|state| {
            self.0
                .request_view()
                .overlay()
                .input_artifacts
                .retain_shallow(state)
        })
    }

    #[inline]
    fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::local_type_declaration_id"
        );
        crate::VerterHost::local_type_declaration_id(self.0.host(), canonical_source, resolved_name)
    }

    #[inline]
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::get_whole_hash");
        crate::VerterHost::get_whole_hash(self.0.host(), canonical)
    }

    #[inline]
    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::authoritative_current_content_hash"
        );
        self.0.authoritative_current_content_hash(canonical)
    }

    #[inline]
    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedInputRecord>> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::indexed_for_current_content"
        );
        self.0
            .indexed_for_current_content(canonical)
            .map(|indexed| {
                self.0
                    .request_view()
                    .overlay()
                    .input_artifacts
                    .retain(crate::host_manage::prepared_decl::IndexedReadyServe {
                        indexed,
                        store_published: true,
                    })
                    .indexed
            })
    }

    #[inline]
    fn artifact_key_for_current_content(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::source::artifact_key::FileArtifactKey> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::artifact_key_for_current_content"
        );
        self.0.artifact_key_for_current_content(canonical)
    }

    #[inline]
    fn observe_materialize_scope(&self, canonical: &str) -> Option<MaterializeScopeObservation> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::observe_materialize_scope"
        );
        self.0.observe_materialize_scope(self, canonical)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::get_raw_analysis_snapshot"
        );
        crate::VerterHost::get_raw_analysis_snapshot(self.0.host(), canonical)
    }
}

impl<L: RequestBoundLifecycle> RouteLookup for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn reverse_dependency_canonicals(&self, canonical: &str) -> Vec<String> {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::reverse_dependency_canonicals"
        );
        self.0.host().workspace().reverse_deps_for(canonical)
    }
    fn observe_owner_import_route_witness(&self, canonical: &str) {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::observe_owner_import_route_witness"
        );
        self.0.host().observe_owner_import_route_witness(canonical);
    }

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<verter_session_query::type_solver::ResolvedRootIdentity> {
        verter_type_engine::count_resolver_context_call!("RouteLookup::resolve_imported_type_root");
        // The context-bound shim validates the cached imported-root entry
        // against this request's view instead of rebuilding a snapshot.
        self.0.host().resolve_imported_type_root_with_context(
            self,
            self.0.session_view(),
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
        Option<verter_session_query::type_solver::ResolvedRootIdentity>,
        Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>,
    ) {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::resolve_imported_type_root_with_facts"
        );
        self.0
            .host()
            .resolve_imported_type_root_with_facts_with_context(
                self,
                self.0.session_view(),
                dep_canonical,
                imported_name,
            )
    }

    #[inline]
    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::resolve_named_type_export_target_shallow"
        );
        self.0
            .host()
            .resolve_named_type_export_target_shallow_with_store_view(
                self,
                self.0.request_view(),
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
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::resolve_owner_direct_import"
        );
        self.0.host().resolve_owner_direct_import_with_store_view(
            self,
            self.0.session_view(),
            self.0.request_view(),
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
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::resolve_type_dependency_canonical"
        );
        self.0
            .resolve_type_dependency_canonical(owner_canonical, import_source)
    }

    #[inline]
    fn routed_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
        verter_type_engine::count_resolver_context_call!("RouteLookup::routed_shallow_state");
        self.0
            .host()
            .routed_shallow_state_with_view(canonical_id, self.0.session_view())
            .map(|state| {
                self.0
                    .request_view()
                    .overlay()
                    .input_artifacts
                    .retain_shallow(state)
            })
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        requested_name: &str,
    ) -> verter_session_query::declarations::metadata::ResolvedTypeDeclaration {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::resolve_type_declaration_for_dep"
        );
        // The walker constructed inside binds to this request-bound context.
        crate::host_manage::jsdoc_resolve::resolve_type_declaration_with_context(
            self.0.host(),
            self,
            dep_canonical,
            owner,
            requested_name,
        )
    }

    #[inline]
    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::resolve_value_export_target"
        );
        crate::VerterHost::resolve_value_export_target(
            self.0.host(),
            dep_canonical_id,
            imported_name,
        )
    }

    #[inline]
    fn lookup_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        verter_type_engine::count_resolver_context_call!("RouteLookup::lookup_ambient_symbol");
        self.0
            .host()
            .workspace()
            .lookup_ambient_symbol(consumer_project, symbol)
    }

    #[inline]
    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str) {
        verter_type_engine::count_resolver_context_call!("RouteLookup::record_ambient_dependency");
        self.0
            .host()
            .workspace()
            .record_ambient_dependency(consumer_canonical, virtual_id);
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "RouteLookup::workspace_is_package_backed"
        );
        self.0.host().workspace().is_package_backed(canonical_id)
    }
}

impl<L: RequestBoundLifecycle> ExecutionSubmission for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    type MacroMirrors = super::request_inputs::MacroMirrorSelector;
    fn attach_engine(
        &self,
    ) -> verter_type_engine::project_semantic_dispatch::EngineBinding<Self::MacroMirrors> {
        verter_type_engine::count_resolver_context_call!("ExecutionSubmission::attach_engine");
        self.0.host().project_type_store().bind_engine(
            self.0.host().engine_observers(),
            self.0
                .request_view()
                .overlay()
                .input_artifacts
                .macro_selector(
                    #[cfg(any(test, feature = "test-support"))]
                    Arc::clone(&self.0.host().test_force.engine),
                ),
        )
    }
}

impl<L: RequestBoundLifecycle> verter_type_engine::resolver_core::request_ports::HostAttachmentPort
    for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    type HostAttachment = crate::session_attachment::SessionAttachment;
    fn host_attachment(&self) -> &Self::HostAttachment {
        verter_type_engine::count_resolver_context_call!("HostAttachmentPort::host_attachment");
        self.0.host().session_attachment()
    }
}

/// A type-route publication as a resolver context answers it: the admitted
/// target, or — for a refusal — `None` marked non-cacheable, so a refused
/// route never becomes a cacheable "not found". Every context answers
/// through this, whichever resolution snapshot produced the publication.
pub(crate) fn type_route_answer(
    publication: verter_workspace::ResolutionPublication<String>,
) -> Option<String> {
    match publication {
        verter_workspace::ResolutionPublication::Admitted(admitted) => admitted.into_result(),
        verter_workspace::ResolutionPublication::Refused(_) => {
            note_non_cacheable_read_fan_out(NonCacheableReadReason::UnrootableRoute);
            None
        }
    }
}

impl crate::VerterHost {
    /// A fact tracer for a compute that runs in steps, its basis installed
    /// now exactly as [`Self::with_fact_tracer_cell`] installs one.
    /// Run `f` with a fact tracer installed; return
    /// `(R, FactReadSet)`.
    ///
    /// The tracer accumulates every `observe` /
    /// `observe_borrowed_signature` call made through any
    /// [`ResolverContext`] reference derived from this host
    /// inside the closure.
    ///
    /// Nesting is supported: an inner `with_fact_tracer` scope pushes a
    /// second cell onto the tracer stack. `observe_fan_out*` delivers
    /// observations into **all** active cells simultaneously, so outer
    /// scopes see the inner scope's observations.
    ///
    /// The tracer is `!Send + !Sync` and is installed on the caller's
    /// thread only. Worker threads spawned from inside `f` do NOT
    /// inherit the tracer; consumers that fan out work across threads
    /// must collect signatures explicitly and call
    /// [`ResolverContext::observe_borrowed_signature`] on the parent
    /// thread to merge the worker's facts.
    #[must_use]
    pub fn with_fact_tracer<F, R>(
        &self,
        seed: verter_session_query::facts::fact_cache::AggregateBasisSeed,
        f: F,
    ) -> (R, verter_session_query::facts::fact_read_set::FactReadSet)
    where
        F: FnOnce() -> R,
    {
        self.with_fact_tracer_cell(seed, |_cell| f())
    }

    /// [`Self::with_fact_tracer`], handing the traced closure a borrow of
    /// the scope's own [`verter_session_query::facts::fact_read_set::FactReadSetCell`].
    ///
    /// The cell accumulates monotonically, so a closure holding it can read
    /// the scope's verdict-so-far MID-SCOPE — the seam
    /// `fact_signature_helpers::with_cacheability_scope` builds its
    /// `CacheabilityProbe` on, so a shared-cache admission that happens
    /// INSIDE the traced compute can consult the verdict without popping the
    /// tracer. The borrow cannot escape: it lives only for the closure call.
    ///
    /// **The one place a tracer cell is created, and therefore the one
    /// place a compaction basis is installed.** `with_fact_tracer`
    /// delegates here, so every scope — the two cacheability/signature
    /// helpers and every raw consumer that bypasses them — gets its basis
    /// from this single seam rather than from whichever helper the
    /// producer happened to open.
    ///
    /// `seed` is the already-bound view's captured contribution; the live
    /// half comes from [`Self::live_aggregate_counters`]. The basis is
    /// installed BEFORE `f` runs, so the scope's first observation is
    /// already covered by movement detection.
    #[must_use]
    pub fn with_fact_tracer_cell<F, R>(
        &self,
        seed: verter_session_query::facts::fact_cache::AggregateBasisSeed,
        f: F,
    ) -> (R, verter_session_query::facts::fact_read_set::FactReadSet)
    where
        F: FnOnce(&verter_session_query::facts::fact_read_set::FactReadSetCell) -> R,
    {
        self::with_fact_tracer_cell(
            verter_session_query::facts::fact_cache::AggregateGenerations::from_seed(
                &seed,
                &self.live_aggregate_counters(),
            ),
            f,
        )
    }

    /// Public accessor for the active fact tracer.
    ///
    /// Returns the currently-installed [`FactReadSetCell`] handle, or
    /// `None` when no [`Self::with_fact_tracer`] scope is on the
    /// stack. This is the public-API mirror of the resolver-tier
    /// trait method `ResolverContext::current_fact_tracer` and exists
    /// so consumers outside the resolver-tier seal (notably integration
    /// tests + benches) can verify warm-hit vs cold-compute behaviour
    /// without depending on the sealed trait.
    #[inline]
    #[must_use]
    pub fn current_fact_tracer(
        &self,
    ) -> Option<&verter_session_query::facts::fact_read_set::FactReadSetCell> {
        tracer_stack::current_tracer()
    }
}

#[cfg(test)]
mod request_bound_adapter_structure_tests {
    use verter_type_engine::resolver_core::ResolverContext;

    fn assert_resolver_context<T: ResolverContext<super::HostCapabilities>>() {}

    #[test]
    fn request_bound_lifecycles_share_one_resolver_context_implementation() {
        assert_resolver_context::<crate::resolver_core::HostResolverContext<'static>>();
        assert_resolver_context::<crate::resolver_core::SessionResolverContext<'static>>();
    }
}

use std::collections::BTreeSet;
use verter_session_query::facts::fact_cache::{
    DerivedFactKind, FactVersionRef, ParseFactRef, ProgramAnalysisFactRef, ResolveImportsFactRef,
    RouteSurfaceFactRef,
};
use verter_session_query::facts::store_view::{ResolverHash16, StoreView, StoreViewCompatToken};
use verter_type_engine::resolver_core::fact_validation_port::FactValidation;

impl<L: RequestBoundLifecycle>
    verter_type_engine::resolver_core::fact_validation_port::LiveFactValidation
    for RequestBoundAdapter<L>
{
    type Clocks = crate::resolver_store::WorkspaceSlotClocks;
    fn request_snapshot(
        &self,
    ) -> &verter_type_engine::resolver_core::RequestSnapshot<Self::Clocks> {
        verter_type_engine::count_resolver_context_call!("LiveFactValidation::request_snapshot");
        self.0.request_snapshot()
    }
}

impl<L: RequestBoundLifecycle> FactValidation for RequestBoundAdapter<L> {
    fn current_external_supersession_fingerprint(&self) -> u64 {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::current_external_supersession_fingerprint"
        );
        self.0.host().current_external_supersession_fingerprint()
    }
    fn source_environment(
        &self,
        key: &verter_session_query::source::artifact_key::FileArtifactKey,
    ) -> verter_session_query::source::env_identity::SourceEnvIdentity {
        verter_type_engine::count_resolver_context_call!("FactValidation::source_environment");
        crate::resolver_store::live_source_env_identity(self.0.host(), key)
    }
    fn request_flags(&self) -> &verter_type_engine::resolver_core::RequestFlags {
        verter_type_engine::count_resolver_context_call!("FactValidation::request_flags");
        self.0.request_snapshot().flags()
    }
    fn complete_graph_signature(
        &self,
        roots: &[(
            std::sync::Arc<str>,
            verter_session_query::analysis::types::Hash16,
        )],
        facts: &[FactVersionRef],
    ) -> Result<
        verter_type_engine::fact_signature_helpers::StructuralCarrierReadSet,
        verter_audit::NonAdmissionReason,
    > {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::complete_graph_signature"
        );
        verter_type_engine::semantic_query_memo::semantic_graph_read_set_signature(
            self.0.request_view(),
            roots,
            facts,
        )
    }
    #[cfg(any(test, feature = "test-support"))]
    fn tracer_forcing(&self) -> bool {
        verter_type_engine::count_resolver_context_call!("FactValidation::tracer_forcing");
        self.0
            .host()
            .test_force
            .engine
            .force_fact_tracer_non_cacheable_read
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[inline]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::current_dependency_fact_versions"
        );
        crate::VerterHost::current_dependency_fact_versions(self.0.host(), canonical, tracked_deps)
    }
    fn compat_token(&self) -> StoreViewCompatToken {
        verter_type_engine::count_resolver_context_call!("FactValidation::compat_token");
        self.0.request_view().compat_token()
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        verter_type_engine::count_resolver_context_call!("FactValidation::validates");
        self.0.request_view().validates(fact)
    }
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        verter_type_engine::count_resolver_context_call!("FactValidation::validates_parse_domain");
        self.0.request_view().validates_parse_domain(fact)
    }
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_resolve_imports_domain"
        );
        self.0.request_view().validates_resolve_imports_domain(fact)
    }
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_route_surface_domain"
        );
        self.0.request_view().validates_route_surface_domain(fact)
    }
    fn validates_program_analysis_domain(&self, fact: &ProgramAnalysisFactRef) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_program_analysis_domain"
        );
        self.0
            .request_view()
            .validates_program_analysis_domain(fact)
    }
    fn validates_file_source_env(
        &self,
        canonical_id: &str,
        parse_env_hash: verter_session_query::facts::fact_cache::ParseEnvHash,
        parse_key: &verter_language::ParseKey,
        file_language_id: &verter_language::FileLanguage,
    ) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_file_source_env"
        );
        self.0.request_view().validates_file_source_env(
            canonical_id,
            parse_env_hash,
            parse_key,
            file_language_id,
        )
    }
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_self_root_whole_hash"
        );
        self.0
            .request_view()
            .validates_self_root_whole_hash(canonical_id, hash)
    }
    fn strict_self_root_world_identity(
        &self,
    ) -> Option<verter_session_query::facts::fact_cache::StrictSelfRootWorld> {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::strict_self_root_world_identity"
        );
        self.0.request_view().strict_self_root_world_identity()
    }
    fn strict_self_root_is_witnessable(&self, canonical_id: &str) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::strict_self_root_is_witnessable"
        );
        self.0
            .request_view()
            .strict_self_root_is_witnessable(canonical_id)
    }
    fn mint_strict_self_root_world(
        &self,
        roots: &[(&str, ResolverHash16)],
    ) -> Option<verter_session_query::facts::fact_cache::StrictSelfRootWorld> {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::mint_strict_self_root_world"
        );
        self.0.request_view().mint_strict_self_root_world(roots)
    }
    fn tracks_file(&self, canonical_id: &str) -> bool {
        verter_type_engine::count_resolver_context_call!("FactValidation::tracks_file");
        self.0.request_view().tracks_file(canonical_id)
    }
    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        verter_type_engine::count_resolver_context_call!("FactValidation::derived_hash_for");
        self.0.request_view().derived_hash_for(canonical_id, kind)
    }
    fn aggregate_basis_seed(&self) -> verter_session_query::facts::fact_cache::AggregateBasisSeed {
        verter_type_engine::count_resolver_context_call!("FactValidation::aggregate_basis_seed");
        self.0.request_view().aggregate_basis_seed()
    }
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_fact_signature"
        );
        self.0.request_view().validates_fact_signature(sig)
    }
    fn validate_fact_signature(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> Result<(), usize> {
        verter_type_engine::count_resolver_context_call!("FactValidation::validate_fact_signature");
        self.0
            .request_view()
            .validate_fact_signature(sig, self_root_canonicals)
    }
    fn validates_fact_signature_with_self_roots(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_fact_signature_with_self_roots"
        );
        self.0
            .request_view()
            .validates_fact_signature_with_self_roots(sig, self_root_canonicals)
    }
    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: verter_session_query::analysis::types::Hash16,
        route_hash: Option<verter_session_query::analysis::types::Hash16>,
    ) {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::promote_route_completion"
        );
        self.0
            .request_view()
            .promote_route_completion(canonical, whole_hash, route_hash)
    }
}
