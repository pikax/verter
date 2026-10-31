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
use verter_type_engine::request_context::{ObserveExec, Uncaptured};

fn observe_availability(
    canonical: &str,
    serve: &crate::host_manage::prepared_decl::IndexedReadyServe,
) {
    if !serve.store_published {
        note_non_cacheable_read_fan_out(NonCacheableReadReason::FencedServe);
    }
    // This port exposes only availability, not the source payload or its
    // parse key (which includes a content ID). Use the exact served route
    // interface; the content-consuming ports own their narrower/broader proof.
    observe_fan_out(FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical.into(),
        key: verter_session_query::facts::FactKey::SyntacticRouteInterface,
        lane: verter_session_query::facts::FactLane::Semantic,
        expected_hash: crate::resolver_store::syntactic_route_interface_hash(
            &serve.indexed.shallow_state,
        ),
    }));
}

/// The capability types every host request context hands out: the source's
/// owned expression-demand handle and the host's live workspace slot. Base
/// and overlay contexts share them, so request structures carrying them are
/// instantiated once per capture mode.
///
/// `M` is the capture execution ([`verter_audit::observe::Captured`] or
/// [`verter_audit::observe::Uncaptured`]) a request context of this family
/// runs: the dispatcher and the request ports it resolves through are
/// compiled for it. A host selects it from the mode the installed request
/// carries ([`crate::capture::installed_mode`]); `Uncaptured` names the
/// family of every context that never captures.
pub struct HostCapabilities<M: ObserveExec = Uncaptured>(
    std::convert::Infallible,
    std::marker::PhantomData<M>,
);

impl<M: ObserveExec> ResolverCapabilities for HostCapabilities<M> {
    type ExpressionDemand = crate::host_source_demand::HostExpressionDemand;
    type Clocks = crate::resolver_store::WorkspaceSlotClocks;
    type HostAttachment = crate::session_attachment::SessionAttachment;
    type MacroMirrors = super::request_inputs::MacroMirrorSelector;
    type Observe = M;
}

#[cfg(any(test, feature = "test-support"))]
impl sealed::Sealed for crate::VerterHost {}

impl<'a, M: ObserveExec> sealed::Sealed
    for crate::resolver_core::host_resolver_context::HostResolverContext<'a, M>
{
}

impl<'a, M: ObserveExec> sealed::Sealed
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a, M>
{
}

impl<'a, M: ObserveExec> sealed::RequestBoundSealed
    for crate::resolver_core::host_resolver_context::HostResolverContext<'a, M>
{
}

#[cfg(any(test, feature = "test-support"))]
impl sealed::RequestBoundSealed for crate::VerterHost {}

impl<'a, M: ObserveExec> sealed::RequestBoundSealed
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a, M>
{
}

impl<'a, M: ObserveExec> RequestBoundResolverContext<HostCapabilities<M>>
    for crate::resolver_core::host_resolver_context::HostResolverContext<'a, M>
{
}

impl<'a, M: ObserveExec> RequestBoundResolverContext<HostCapabilities<M>>
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a, M>
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
    fn program_file_order(
        &self,
        anchor: &str,
        ordered: &mut dyn Iterator<Item = &str>,
    ) -> Arc<verter_session_query::inputs::program_order::ProgramFileOrder> {
        crate::VerterHost::program_file_order(self, anchor, ordered)
    }

    fn engine_policy(&self) -> verter_type_engine::project_semantic_dispatch::EnginePolicy {
        self.engine_policy.clone()
    }

    // Cache accessors -------------------------------------------------

    #[inline]
    fn prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<verter_session_query::inputs::prepared::PreparedInputRef> {
        let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
        crate::VerterHost::prepared_decl_bundle_with_store_view(self, &view, None, canonical_id)
            .map(|bundle| self.source_input_leases.retain_prepared(bundle))
    }

    #[inline]
    fn ensure_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical_id).map(|serve| {
            super::owned_lowering_port::observe_whole_content(
                canonical_id,
                serve.indexed.whole_hash,
            );
            self.source_input_leases.retain(serve)
        })
    }

    fn source_is_available(&self, canonical: &str) -> bool {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical).is_some_and(|serve| {
            observe_availability(canonical, &serve);
            true
        })
    }

    #[inline]
    fn base_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical_id).map(|serve| {
            super::owned_lowering_port::observe_whole_content(
                canonical_id,
                serve.indexed.whole_hash,
            );
            self.source_input_leases.retain(serve)
        })
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        crate::VerterHost::ensure_loaded(self, canonical_id)
    }

    fn begin_indexed_load(
        &self,
        canonical_id: &str,
    ) -> verter_type_engine::resolver_core::request_ports::IndexedReadiness<'_> {
        crate::VerterHost::begin_indexed_ready_load(self, canonical_id)
    }

    fn begin_prepared_bundle_load(
        &self,
        canonical_id: &str,
        awaited: &[Arc<str>],
    ) -> Option<verter_type_engine::resolver_core::request_ports::PendingLoad<'_>> {
        crate::VerterHost::begin_prepared_bundle_route_load::<verter_audit::observe::Uncaptured>(
            self,
            self,
            canonical_id,
            awaited,
            |target| crate::VerterHost::begin_indexed_ready_load(self, target),
        )
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
        crate::VerterHost::shallow_file_state(self, canonical_id).map(|state| {
            super::owned_lowering_port::observe_whole_content(canonical_id, state.whole_hash);
            self.source_input_leases.retain_shallow(state)
        })
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
        let snapshot = crate::VerterHost::get_raw_analysis_snapshot(self, canonical)?;
        super::owned_lowering_port::observe_analysis_revision(canonical, &snapshot.anchor_revision);
        Some(snapshot)
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
    fn module_export_table(
        &self,
        canonical: &str,
    ) -> Option<
        verter_retention::heap_size::OwnedValue<
            verter_session_query::declarations::module_exports::ModuleExportTable,
        >,
    > {
        let view = self.resolver_store_view().into_owned_view();
        self.module_export_table_with_context(self, &view, canonical)
    }
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
        verter_session_query::facts::fact_cache::OwnedFactSignature,
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
        crate::VerterHost::routed_shallow_state(self, canonical_id).map(|state| {
            super::owned_lowering_port::observe_whole_content(canonical_id, state.whole_hash);
            self.source_input_leases.retain_shallow(state)
        })
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
        space: verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace,
    ) -> Option<AmbientSymbolHit> {
        crate::VerterHost::program_ambient_symbol(self, consumer_project, symbol, space)
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
pub(crate) trait RequestBoundLifecycle: Sync {
    /// The capture execution this request context runs.
    type Observe: ObserveExec;

    fn capture_targets(&self) -> &verter_type_engine::request_observers::CaptureTargets;

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
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities<Self::Observe>>,
        canonical_id: &str,
    ) -> Option<crate::resolver_core::CachedValue<PreparedDeclBundle>>;

    fn prepared_type_decl(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities<Self::Observe>>,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<verter_retention::heap_size::OwnedValue<PreparedTypeDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
    >;

    fn prepared_value_decl(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities<Self::Observe>>,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<verter_retention::heap_size::OwnedValue<PreparedValueDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
    >;

    /// Materialise (or warm-read) the canonical artifact. The adapter
    /// performs the canonical completion on success.
    fn materialize_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        crate::VerterHost::ensure_indexed_ready_serve_in::<Self::Observe>(
            self.host(),
            self.capture_targets(),
            canonical_id,
        )
    }

    /// Load the canonical. The adapter performs the canonical completion
    /// on success.
    fn load(&self, canonical_id: &str) -> bool {
        crate::VerterHost::ensure_loaded(self.host(), canonical_id)
    }

    /// Begin the load [`Self::load`] would perform, as an owned wait
    /// instead of a drive; `Settled` where the serve loads nothing.
    fn begin_load(
        &self,
        canonical_id: &str,
    ) -> verter_type_engine::resolver_core::request_ports::IndexedReadiness<'_> {
        crate::VerterHost::begin_indexed_ready_load(self.host(), canonical_id)
    }

    fn shallow_file_state(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities<Self::Observe>>,
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
        self.host()
            .current_content_pinned_indexed_in::<Self::Observe>(self.capture_targets(), canonical)
    }

    fn artifact_key_for_current_content(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::source::artifact_key::FileArtifactKey> {
        self.host().authoritative_current_artifact_key(canonical)
    }

    fn observe_materialize_scope(
        &self,
        ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities<Self::Observe>>,
        canonical: &str,
    ) -> Option<MaterializeScopeObservation> {
        self.host()
            .observe_materialize_scope_with_context(ctx, canonical)
    }

    /// The shared type-route policy's publication for this request's
    /// resolution view: admitted, unretained, or refused.
    fn type_dependency_publication(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> verter_workspace::ResolutionPublication<String> {
        crate::VerterHost::resolve_type_dependency_canonical(
            self.host(),
            owner_canonical,
            import_source,
        )
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

impl<L: RequestBoundLifecycle> ResolverContext<HostCapabilities<L::Observe>>
    for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
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
        self.0.request_view().project_env_hashes_for(canonical, || {
            self.0.host().host_view_env_hashes_for(canonical)
        })
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
        self.0.request_view().project_identity_for(canonical, || {
            self.0.host().host_view_project_identity_for(canonical)
        })
    }
    fn semantic_compiler_options_for(
        &self,
        canonical: &str,
    ) -> verter_session_query::resolution::SemanticCompilerOptions {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::semantic_compiler_options_for"
        );
        self.0
            .request_view()
            .project_compiler_options_for(canonical, || {
                self.0.host().semantic_compiler_options_for(canonical)
            })
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
    fn program_file_order(
        &self,
        anchor: &str,
        ordered: &mut dyn Iterator<Item = &str>,
    ) -> Arc<verter_session_query::inputs::program_order::ProgramFileOrder> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::program_file_order");
        self.0.host().program_file_order(anchor, ordered)
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
    fn prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<verter_session_query::inputs::prepared::PreparedInputRef> {
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
            super::owned_lowering_port::observe_whole_content(
                canonical_id,
                serve.indexed.whole_hash,
            );
            self.0
                .request_view()
                .overlay()
                .input_artifacts
                .retain(serve)
        })
    }

    fn source_is_available(&self, canonical: &str) -> bool {
        self.0
            .materialize_indexed_ready_serve(canonical)
            .is_some_and(|serve| {
                self.0.complete_canonical(canonical);
                observe_availability(canonical, &serve);
                true
            })
    }

    /// Base-store read, deliberately NOT the request view's overlay-priority
    /// candidate: the consumer-boundary span slicer publishes from the
    /// base-store artifact, and the request view must not re-point that text
    /// at an overlay candidate.
    #[inline]
    fn base_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::base_indexed_ready_serve");
        crate::VerterHost::ensure_indexed_ready_serve_in::<L::Observe>(
            self.0.host(),
            self.0.capture_targets(),
            canonical_id,
        )
        .map(|serve| {
            super::owned_lowering_port::observe_whole_content(
                canonical_id,
                serve.indexed.whole_hash,
            );
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

    fn begin_indexed_load(
        &self,
        canonical_id: &str,
    ) -> verter_type_engine::resolver_core::request_ports::IndexedReadiness<'_> {
        verter_type_engine::count_resolver_context_call!("IndexedInputs::begin_indexed_load");
        self.0.begin_load(canonical_id)
    }

    fn begin_prepared_bundle_load(
        &self,
        canonical_id: &str,
        awaited: &[Arc<str>],
    ) -> Option<verter_type_engine::resolver_core::request_ports::PendingLoad<'_>> {
        verter_type_engine::count_resolver_context_call!(
            "IndexedInputs::begin_prepared_bundle_load"
        );
        crate::VerterHost::begin_prepared_bundle_route_load::<L::Observe>(
            self.0.host(),
            self,
            canonical_id,
            awaited,
            |target| self.0.begin_load(target),
        )
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
        crate::VerterHost::get_whole_hash_in::<L::Observe>(
            self.0.host(),
            self.0.capture_targets(),
            canonical,
        )
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
                super::owned_lowering_port::observe_whole_content(canonical, indexed.whole_hash);
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
        let snapshot = crate::VerterHost::get_raw_analysis_snapshot_in::<L::Observe>(
            self.0.host(),
            self.0.capture_targets(),
            canonical,
        )?;
        super::owned_lowering_port::observe_analysis_revision(canonical, &snapshot.anchor_revision);
        Some(snapshot)
    }
}

impl<L: RequestBoundLifecycle> RouteLookup for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn module_export_table(
        &self,
        canonical: &str,
    ) -> Option<
        verter_retention::heap_size::OwnedValue<
            verter_session_query::declarations::module_exports::ModuleExportTable,
        >,
    > {
        verter_type_engine::count_resolver_context_call!("RouteLookup::module_export_table");
        self.0
            .host()
            .module_export_table_with_context(self, self.0.request_view(), canonical)
    }
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
        verter_session_query::facts::fact_cache::OwnedFactSignature,
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
        type_route_answer(
            self.0
                .type_dependency_publication(owner_canonical, import_source),
        )
    }

    #[inline]
    fn routed_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
        verter_type_engine::count_resolver_context_call!("RouteLookup::routed_shallow_state");
        self.0
            .host()
            .routed_shallow_state_with_view_in::<L::Observe>(
                self.0.capture_targets(),
                canonical_id,
                self.0.session_view(),
            )
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
        space: verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace,
    ) -> Option<AmbientSymbolHit> {
        verter_type_engine::count_resolver_context_call!("RouteLookup::lookup_ambient_symbol");
        crate::VerterHost::program_ambient_symbol(self.0.host(), consumer_project, symbol, space)
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
/// target; an unretained target, the same complete answer, marking the
/// compute that reads it non-cacheable so nothing derived from it is
/// retained; or — for a refusal — `None` marked non-cacheable, so a
/// refused route never becomes a cacheable "not found". Every context
/// answers through this, whichever resolution snapshot produced the
/// publication.
pub(crate) fn type_route_answer(
    publication: verter_workspace::ResolutionPublication<String>,
) -> Option<String> {
    match publication {
        verter_workspace::ResolutionPublication::Admitted(admitted) => admitted.into_result(),
        verter_workspace::ResolutionPublication::Unretained(unretained) => {
            note_non_cacheable_read_fan_out(NonCacheableReadReason::UnretainedResolution);
            unretained.into_result()
        }
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
            self.project_type_store().fact_policy(),
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
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    type Clocks = crate::resolver_store::WorkspaceSlotClocks;
    fn request_snapshot(
        &self,
    ) -> &verter_type_engine::resolver_core::RequestSnapshot<Self::Clocks> {
        verter_type_engine::count_resolver_context_call!("LiveFactValidation::request_snapshot");
        self.0.request_snapshot()
    }
}

impl<L> RequestBoundAdapter<L> {
    /// Complete edited parse facts before the view captures its validation
    /// epoch. Indexing during a leaf check would tear that epoch instead.
    fn prepare_validation_inputs(
        &self,
        facts: verter_session_query::facts::fact_set::FactsRef<'_>,
    ) -> Result<(), usize>
    where
        L: RequestBoundLifecycle,
        Self: verter_type_engine::resolver_core::request_ports::OwnedLowering,
    {
        #[cfg(test)]
        self.0.request_view().note_validation_preparation();
        // Completing parse facts serves the validation: what it reads is no
        // dependency of the computation that is validating a candidate.
        verter_type_engine::resolver_core::resolver_context::without_fact_observers(|| {
            let mut walk = verter_session_query::facts::fact_cache::ReceiptWalk::default();
            let population_ready = std::cell::Cell::new(false);
            facts.iter().enumerate().try_for_each(|(index, fact)| {
            let valid = verter_session_query::facts::fact_cache::validates_through_receipts(
                fact,
                &mut walk,
                |leaf| {
                    if let FactVersionRef::RouteSurface(fact) = leaf {
                        if let verter_session_query::facts::FactKey::ModuleAugmentationIndexShape {
                            target_kind_tag, external_specifier, resolved_relative_canonical, wildcard_pattern,
                        } = &fact.key {
                            if !population_ready.replace(true) {
                                self.0.host().ingest_program_ambient_roots();
                            }
                            let population_decl = crate::global_contributors::population_fact_decl_name(
                                *target_kind_tag, external_specifier.as_ref().map(|s| s.as_ref()), wildcard_pattern.as_ref().map(|s| s.as_ref()));
                            if population_decl.is_none() {
                                let Some(target) = crate::resolver_store::augmentation_target_kind_from_shape(
                                    *target_kind_tag, external_specifier.as_ref().map(|s| s.as_ref()),
                                    resolved_relative_canonical.as_deref(), wildcard_pattern.as_ref().map(|s| s.as_ref())) else {
                                    return false;
                                };
                                // Materialize through the same target population owner as
                                // cold evaluation. The captured root may still name a
                                // formerly empty set; both current and captured proofs
                                // must agree before that receipt can be reused.
                                let (key, current, _) = verter_type_engine::resolver_core::request_ports::OwnedLowering::augmentation_index_with_resolutions(self, target);
                                if current.fingerprint != fact.expected_hash
                                    || !self.0.host().project_type_store().indexed()
                                        .get_augmenter_set(&key).is_some_and(|published| std::sync::Arc::ptr_eq(&published, &current)) {
                                    return false;
                                }
                            }
                        }
                    }
                    let FactVersionRef::Parse(parse) = leaf else {
                        return true;
                    };
                    // Completing a canonical records the content this context
                    // serves, which the view's overlay lookups then trust over
                    // the view's own tracked hash. A canonical a session layer
                    // replaces with different content is not this context's to
                    // complete: the fact cannot be certified from the host's content.
                    if self.0.request_view().base().session_masks(&parse.canonical_id)
                        && self.0.request_view().base().whole_hash(&parse.canonical_id)
                            != self.0.authoritative_current_content_hash(&parse.canonical_id)
                    {
                        return false;
                    }
                    if matches!(
                        parse.key,
                        verter_session_query::facts::FactKey::DirectDeclarationExport { .. }
                    ) {
                        self.0.complete_canonical(&parse.canonical_id);
                        return true;
                    }
                    let Some(serve) = self.0.materialize_indexed_ready_serve(&parse.canonical_id)
                    else {
                        return false;
                    };
                    if !serve.store_published
                        || self
                            .0
                            .authoritative_current_content_hash(&parse.canonical_id)
                            != Some(serve.indexed.whole_hash)
                    {
                        return false;
                    }
                    self.0.complete_canonical(&parse.canonical_id);
                    true
                },
            );
            valid.then_some(()).ok_or(index)
        })
        })
    }
}

impl<L: RequestBoundLifecycle> FactValidation for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn publication_input_fingerprint(&self) -> Option<u64> {
        self.0.request_view().publication_input_fingerprint()
    }

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
            verter_type_expr::OwnedStr,
            verter_session_query::analysis::types::Hash16,
        )],
        traced: &verter_session_query::facts::fact_set::FactSet,
        extra: Vec<FactVersionRef>,
    ) -> Result<
        verter_type_engine::fact_signature_helpers::StructuralCarrierReadSet,
        verter_audit::NonAdmissionReason,
    > {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::complete_graph_signature"
        );
        verter_type_engine::semantic_query_memo::semantic_graph_carrier(
            self.0.request_view(),
            roots,
            traced,
            extra,
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
        self.prepare_validation_inputs(verter_session_query::facts::fact_set::FactsRef::Slice(
            std::slice::from_ref(fact),
        ))
        .is_ok()
            && self.0.request_view().validates(fact)
    }
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        verter_type_engine::count_resolver_context_call!("FactValidation::validates_parse_domain");
        self.prepare_validation_inputs(verter_session_query::facts::fact_set::FactsRef::Slice(&[
            FactVersionRef::Parse(fact.clone()),
        ]))
        .is_ok()
            && self.0.request_view().validates_parse_domain(fact)
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
        self.prepare_validation_inputs(verter_session_query::facts::fact_set::FactsRef::Slice(&[
            FactVersionRef::RouteSurface(fact.clone()),
        ]))
        .is_ok()
            && self.0.request_view().validates_route_surface_domain(fact)
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
    fn validates_fact_signature(
        &self,
        sig: verter_session_query::facts::fact_set::FactsRef<'_>,
    ) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_fact_signature"
        );
        self.prepare_validation_inputs(sig).is_ok()
            && self.0.request_view().validates_fact_signature(sig)
    }
    fn validate_fact_signature(
        &self,
        sig: verter_session_query::facts::fact_set::FactsRef<'_>,
        self_root_canonicals: &[&str],
    ) -> Result<(), usize> {
        verter_type_engine::count_resolver_context_call!("FactValidation::validate_fact_signature");
        self.prepare_validation_inputs(sig)?;
        self.0
            .request_view()
            .validate_fact_signature(sig, self_root_canonicals)
    }
    fn validates_fact_signature_with_self_roots(
        &self,
        sig: verter_session_query::facts::fact_set::FactsRef<'_>,
        self_root_canonicals: &[&str],
    ) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_fact_signature_with_self_roots"
        );
        self.prepare_validation_inputs(sig).is_ok()
            && self
                .0
                .request_view()
                .validates_fact_signature_with_self_roots(sig, self_root_canonicals)
    }
    fn validates_retained_signature_with_self_roots(
        &self,
        sig: &verter_session_query::facts::fact_set::FactSet,
        self_root_canonicals: &[&str],
    ) -> bool {
        verter_type_engine::count_resolver_context_call!(
            "FactValidation::validates_retained_signature_with_self_roots"
        );
        let view = self.0.request_view();
        let settled = view.settled_retained_signature(sig, self_root_canonicals);
        if settled == Some(false) {
            return false;
        }
        // Sampled before the walk: a mutation landing during it leaves this
        // token behind the live one, so the next read walks again.
        let inputs = self.0.host().current_validation_token();
        if settled == Some(true) && view.signature_prepared_under(sig, &inputs) {
            return true;
        }
        if self
            .prepare_validation_inputs(verter_session_query::facts::fact_set::FactsRef::Set(sig))
            .is_err()
        {
            return false;
        }
        view.note_signature_prepared(sig, inputs);
        view.validates_retained_signature(sig, self_root_canonicals)
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

#[cfg(test)]
mod retained_validation_tests {
    use std::sync::Arc;

    use verter_session_query::facts::fact_cache::{FactVersionRef, ParseFactRef};
    use verter_session_query::facts::fact_set::FactSet;
    use verter_session_query::facts::policy::FactPolicy;
    use verter_session_query::facts::{FactKey, FactLane};
    use verter_type_engine::resolver_core::fact_validation_port::FactValidation;

    use super::RequestBoundLifecycle;
    use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};

    const CANONICAL: &str = "/proj/button.ts";

    fn route_interface(expected_hash: [u8; 16]) -> FactSet {
        FactPolicy::production().set_of(vec![FactVersionRef::Parse(ParseFactRef {
            canonical_id: CANONICAL.into(),
            key: FactKey::SyntacticRouteInterface,
            lane: FactLane::Semantic,
            expected_hash,
        })])
    }

    fn served_host() -> (crate::VerterHost, [u8; 16]) {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: None,
                input_id: CANONICAL.to_string(),
                source: Arc::from(
                    "export interface ButtonProps { label: string }
",
                ),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("the file is admitted");
        let serve = host
            .ensure_indexed_ready_serve(CANONICAL)
            .expect("the file is served");
        let current =
            crate::resolver_store::syntactic_route_interface_hash(&serve.indexed.shallow_state);
        (host, current)
    }

    /// The answer to one read, and how many signatures it prepared.
    fn probe(ctx: &HostResolverContext<'_>, signature: &FactSet) -> (bool, usize) {
        let view = ctx.0.request_view();
        let before = view.validation_preparations_for_tests();
        let answer = ctx.validates_retained_signature_with_self_roots(signature, &[]);
        (answer, view.validation_preparations_for_tests() - before)
    }

    /// Preparing inputs walks every fact of a signature, so it belongs to the
    /// read that settles the signature. A warm read meets the same signature
    /// many times in a request; preparing it each time costs as much as
    /// validating it would, and turned the settled verdict's one lookup into
    /// the larger part of a whole-store inference. Stale verdicts count too:
    /// a candidate that will not serve is met as often as one that will.
    #[test]
    fn a_settled_retained_signature_prepares_its_inputs_once() {
        let (host, current) = served_host();
        let base = host.resolver_store_view_read().into_owned_view();
        let ctx: HostResolverContext<'_> =
            HostResolverContext::new(&host, &base, Arc::new(CanonicalCompletionOverlay::new()));

        for (signature, valid) in [
            (route_interface(current), true),
            (route_interface([0xEE; 16]), false),
        ] {
            assert_eq!(
                probe(&ctx, &signature),
                (valid, 1),
                "the read that settles it"
            );
            assert_eq!(
                probe(&ctx, &signature),
                (valid, 0),
                "a read of the settled verdict"
            );
        }
    }

    /// A settled verdict speaks for the request's base; whether the file is
    /// still served as published is a fact of the live host. The host fences
    /// a serve only after a mutation moved a generation the flight captured,
    /// so the fence below is armed together with that move: it needs nothing
    /// a production fence lacks. A refused read proves nothing to reuse, and
    /// the proof returns with the published serve.
    #[test]
    fn a_settled_valid_signature_observes_a_fence_raised_after_it_settled() {
        let (host, current) = served_host();
        let base = host.resolver_store_view_read().into_owned_view();
        let ctx: HostResolverContext<'_> =
            HostResolverContext::new(&host, &base, Arc::new(CanonicalCompletionOverlay::new()));
        let signature = route_interface(current);
        assert_eq!(
            probe(&ctx, &signature),
            (true, 1),
            "the read that settles it"
        );
        assert_eq!(probe(&ctx, &signature), (true, 0), "unchanged host");

        let fence = &host.test_force.force_indexed_ready_serve_fence_for_tests;
        fence.store(true, std::sync::atomic::Ordering::Relaxed);
        host.bump_store_view_epoch();
        assert_eq!(probe(&ctx, &signature), (false, 1), "the fenced serve");
        assert_eq!(probe(&ctx, &signature), (false, 1), "still fenced");

        fence.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(probe(&ctx, &signature), (true, 1), "published again");
        assert_eq!(probe(&ctx, &signature), (true, 0), "unchanged host");
    }

    /// The same boundary with no test seam in the path: the host retires the
    /// file the signature was settled against while the request runs.
    #[test]
    fn a_settled_valid_signature_is_refused_once_its_file_is_removed() {
        let (host, current) = served_host();
        let base = host.resolver_store_view_read().into_owned_view();
        let ctx: HostResolverContext<'_> =
            HostResolverContext::new(&host, &base, Arc::new(CanonicalCompletionOverlay::new()));
        let signature = route_interface(current);
        assert_eq!(
            probe(&ctx, &signature),
            (true, 1),
            "the read that settles it"
        );

        host.remove(CANONICAL).expect("the file is tracked");
        assert_eq!(probe(&ctx, &signature), (false, 1), "the removed file");
    }

    /// A proof holds from before its walk, not from after it: a walk that
    /// publishes moved the host itself, as a mutation landing during the walk
    /// would, so the first warm read proves the host it finds.
    #[test]
    fn a_preparation_that_moved_the_host_does_not_prove_the_host_it_left() {
        let (_, current) = served_host();
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: None,
                input_id: CANONICAL.to_string(),
                source: Arc::from(
                    "export interface ButtonProps { label: string }
",
                ),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("the file is admitted");
        let base = host.resolver_store_view_read().into_owned_view();
        let ctx: HostResolverContext<'_> =
            HostResolverContext::new(&host, &base, Arc::new(CanonicalCompletionOverlay::new()));
        let signature = route_interface(current);
        assert_eq!(
            probe(&ctx, &signature),
            (true, 1),
            "the walk that publishes"
        );
        assert_eq!(probe(&ctx, &signature), (true, 1), "the host it moved");
        assert_eq!(probe(&ctx, &signature), (true, 0), "unchanged host");
    }

    /// A stale verdict stays stale whatever the host does next, so moving
    /// the host costs it no walk.
    #[test]
    fn a_settled_stale_signature_is_not_prepared_again_when_the_host_moves() {
        let (host, _) = served_host();
        let base = host.resolver_store_view_read().into_owned_view();
        let ctx: HostResolverContext<'_> =
            HostResolverContext::new(&host, &base, Arc::new(CanonicalCompletionOverlay::new()));
        let signature = route_interface([0xEE; 16]);
        assert_eq!(
            probe(&ctx, &signature),
            (false, 1),
            "the read that settles it"
        );

        host.bump_store_view_epoch();
        assert_eq!(probe(&ctx, &signature), (false, 0), "the moved host");
    }
}
