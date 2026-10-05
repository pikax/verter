//! Request-bound adapter implementations for the sealed `ResolverContext`.
//!
//! The method-free marker composes six dyn-compatible services: indexed
//! inputs, owned lowering, routing, fact validation, cancellation and execution
//! submission. Their answers are owned records, typed source demands or an
//! opaque engine attachment; no ambient host/store/config getter is exposed.
//!
//! Private lifecycle adapters select the captured request view and completion
//! overlay. The query facade owns execution and output capabilities, and nested
//! semantic demands reuse that facade. The concrete host remains confined to
//! these private backend owners. External implementations cannot name the
//! private sealing marker. Production requests use `HostResolverContext` or
//! `SessionResolverContext`; direct-host support remains test-only.

use std::sync::Arc;

use verter_session_query::declarations::DeclarationId;
use verter_session_query::resolution::{AmbientSymbolHit, ProjectStableKey};
use verter_session_query::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use super::fact_validation_port::FactValidation;
use super::request_inputs::{IndexedInputRecord, IndexedInputServe, PreparedInputRecord};
use super::request_ports::{
    Cancellation, ExecutionSubmission, IndexedInputs, OwnedLowering, RouteLookup,
};
use verter_session_query::inputs::shallow::ShallowInputRecord;

use crate::project_type_store::IndexedReady;
use crate::resolver_core::fact_tracer_tls;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::ShallowFileState;
use crate::resolver_core::ValueDeclIdentity;

use crate::FileAnalysisSnapshot;
use verter_session_query::analysis::types::Hash16;

/// Private markers used to seal `ResolverContext` (and its request-bound
/// refinement) against external implementations.
pub(super) mod sealed {
    /// Marker trait `ResolverContext` is sealed against. Only types
    /// inside `verter_session` that implement this marker can implement
    /// `ResolverContext`.
    pub trait Sealed {}

    /// Narrower marker sealing [`super::RequestBoundResolverContext`].
    ///
    /// Production implementations are the two genuinely request-bound
    /// contexts. The test-support direct-host seam receives the marker only
    /// in configurations where its entire `ResolverContext` implementation
    /// is compile-visible. No external crate can add an implementation.
    pub trait RequestBoundSealed {}
}

/// A single, tear-free observation of a materialize-memo scope's
/// content identity.
///
/// The materialize-memo publish site
/// (`meta_resolve/materialize/field_types.rs`) needs the scope's
/// content version for two distinct consumers that MUST agree:
///
/// 1. the `NodeScopeId::File { whole_hash }` the projector lowers the
///    `TypeExpr` against — the lowered value's semantic identity;
/// 2. the `ShapeCacheDb` entry's fact-signature self-root — the
///    view-correct shared-cache admission gate.
///
/// Sourcing those from two separate oracles (`shallow_file_state` for
/// the scope id, `authoritative_current_content_hash` for the
/// signature) can tear: an edit landing between the two reads roots a
/// value lowered under `H1` on a signature self-rooted at `H2`. This
/// type closes the tear: the publish site takes ONE
/// `MaterializeScopeObservation` and feeds [`Self::whole_hash`] to
/// BOTH consumers, plus the pinned [`Self::syntactic_export_set`] to
/// the signature builder. Both come from the same
/// `Arc<IndexedReady>` — internally consistent by construction
/// (`FileArtifactStore` is content-addressed; `indexed.whole_hash ==
/// indexed.shallow_state.whole_hash`).
#[derive(Clone)]
pub struct MaterializeScopeObservation {
    /// The scope canonical this observation describes.
    pub canonical_id: Arc<str>,
    /// The content observation whose `whole_hash` roots both
    /// the lowering `NodeScopeId` and the signature self-root.
    pub observed_whole_hash: Hash16,
    pub observed_shallow_hash: Hash16,
    /// The scope's `SyntacticExportSet` parse fact, pinned to
    /// the observed content hash via
    /// [`crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content`].
    /// `None` when the observed version's parse-fact registry is not
    /// recoverable — the publish site then refuses shared-cache
    /// admission while still returning the freshly-computed value.
    pub syntactic_export_set: Option<verter_session_query::facts::fact_cache::ParseFactRef>,
}

impl MaterializeScopeObservation {
    /// The observed scope content version. Feeds both the lowering
    /// `NodeScopeId::File { whole_hash }` and the signature self-root —
    /// a single source, so the two cannot disagree.
    #[inline]
    pub(crate) fn whole_hash(&self) -> crate::resolver_core::ResolverHash16 {
        self.observed_whole_hash
    }
}

/// Restricted host facade for resolver-tier code (`resolver_core/*`,
/// `meta_resolve/*` post-moves, `component_meta_caches.rs`,
/// `project_semantic_dispatch/*`).
///
/// `ResolverContext` composes six request ports and private structural seals.
/// All service traits are dyn-compatible and expose no ambient host access.
///
/// Visibility is `pub(crate)` because this is purely an internal seal — no
/// external integrators construct
/// `&dyn ResolverContext`.
pub(crate) trait ResolverContext:
    sealed::Sealed
    + sealed::RequestBoundSealed
    + IndexedInputs
    + OwnedLowering
    + RouteLookup
    + FactValidation
    + Cancellation
    + ExecutionSubmission
{
}

// Sealed marker — `VerterHost` is the base implementer,
// `HostResolverContext` is the request-bound wrapper that carries a
// borrowed `HostStoreView`, and `SessionResolverContext` is the
// overlay-aware wrapper that delegates every method to a borrowed host
// alongside an overlay-rooted view.
#[cfg(any(test, feature = "test-support"))]
impl sealed::Sealed for crate::VerterHost {}
impl<'a> sealed::Sealed for crate::resolver_core::host_resolver_context::HostResolverContext<'a> {}
impl<'a> sealed::Sealed
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
{
}

/// Sealed marker subtrait: a [`ResolverContext`] that is genuinely
/// REQUEST-BOUND — it carries a per-request [`HostStoreView`] (and, for a
/// session query, an overlay) constructed at the request entry boundary,
/// so [`ResolverContext::is_request_bound`] is `true` and every artifact
/// serve is view-correct for the requesting caller.
///
/// This is the STRUCTURAL rail behind every [`ResolverContext`] use: the
/// base trait itself requires the private request-bound seal, and the query
/// host port retains this narrower marker to state its request-bound API
/// contract directly.
///
/// Sealed via [`sealed::RequestBoundSealed`]. The direct
/// [`crate::VerterHost`] implementation exists only behind the
/// compile-absent production test-support fence. The private seal makes an
/// external or in-crate-laundered production implementation impossible
/// without a visible coherence change here.
///
/// The marker deliberately does NOT distinguish a base
/// [`crate::resolver_core::HostResolverContext`] from an overlay
/// [`crate::resolver_core::SessionResolverContext`] — both are
/// request-bound. Overlay-vs-base correctness stays the caller's
/// obligation (chosen at the request entry) and its regression coverage is
/// tracked separately.
pub(crate) trait RequestBoundResolverContext:
    ResolverContext + sealed::RequestBoundSealed
{
}

// Production request contexts carry the seal. The direct host receives it
// only in the explicitly test-only configuration below.
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
impl<'a> RequestBoundResolverContext
    for crate::resolver_core::host_resolver_context::HostResolverContext<'a>
{
}
impl<'a> RequestBoundResolverContext
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
{
}

// Compile-time dyn-compatibility check. If a future trait edit
// accidentally introduces an associated type, generic method, or
// `where Self: Sized` bound that breaks dyn-compatibility, this assertion
// fires inside this file at compile time long before a callsite-cascade
// error.
static_assertions::assert_obj_safe!(ResolverContext);
// The request-bound refinement is used as `&dyn RequestBoundResolverContext`
// by the query host port, so it must stay dyn-compatible too. A marker
// subtrait of a dyn-compatible trait adding no new methods is dyn-safe;
// this pins it against a future edit.
static_assertions::assert_obj_safe!(RequestBoundResolverContext);
#[cfg(not(any(test, feature = "test-support")))]
static_assertions::assert_not_impl_any!(crate::VerterHost: ResolverContext);
#[cfg(any(test, feature = "test-support"))]
static_assertions::assert_impl_all!(crate::VerterHost: ResolverContext);

/// Test-only direct-host seam. Production builds compile this implementation
/// out, so every production `ResolverContext` is structurally request-bound.
#[cfg(any(test, feature = "test-support"))]
impl ResolverContext for crate::VerterHost {}
#[cfg(any(test, feature = "test-support"))]
impl IndexedInputs for crate::VerterHost {
    fn operand_env_epoch(&self) -> super::request_ports::OperandEnvEpoch {
        let w = self.workspace();
        super::request_ports::OperandEnvEpoch::new(w.published_root(), w.content_generation())
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
    ) -> Option<verter_workspace::workspace_snapshot::ProjectId> {
        crate::VerterHost::resolve_project_for_canonical(self, canonical)
    }
    fn declaration_sequence_rank(&self, canonical: &str) -> u32 {
        crate::VerterHost::declaration_sequence_rank(self, canonical)
    }

    fn engine_policy(&self) -> crate::project_semantic_dispatch::EnginePolicy {
        crate::project_semantic_dispatch::EnginePolicy::from_config(&self.config)
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
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
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
    #[cfg(test)]
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.workspace().is_workspace_owned(canonical_id)
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        self.workspace().is_package_backed(canonical_id)
    }
}
#[cfg(any(test, feature = "test-support"))]
impl Cancellation for crate::VerterHost {}
#[cfg(any(test, feature = "test-support"))]
impl ExecutionSubmission for crate::VerterHost {
    fn attach_engine(&self) -> crate::project_semantic_dispatch::EngineBinding {
        self.project_type_store().bind_engine(
            self.engine_observers(),
            self.source_input_leases.macro_selector(
                #[cfg(test)]
                Arc::clone(&self.test_force),
                #[cfg(test)]
                Arc::clone(&self.macro_hot_lowering_count),
            ),
            self.vue_surface_store_handle(),
            self.svelte_surface_store_handle(),
        )
    }
}

/// Fan `fact` into every active tracer on the current thread's stack.
///
/// Used by the rewritten `compile_fact_emission` and any other producer
/// that must deliver a single observation to all nested tracer scopes.
/// No-op when the stack is empty.
#[inline]
pub(crate) fn observe_fan_out(fact: verter_session_query::facts::fact_cache::FactVersionRef) {
    fact_tracer_tls::observe_fan_out(fact);
}

// ---------------------------------------------------------------------------
// Request-bound lifecycle adapters — the ONE shared `ResolverContext`
// implementation behind `HostResolverContext` and `SessionResolverContext`.
// ---------------------------------------------------------------------------

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
        ctx: &dyn ResolverContext,
        canonical_id: &str,
    ) -> Option<Arc<PreparedDeclBundle>>;

    fn prepared_type_decl(
        &self,
        ctx: &dyn ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    >;

    fn prepared_value_decl(
        &self,
        ctx: &dyn ResolverContext,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
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
        ctx: &dyn ResolverContext,
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
        ctx: &dyn ResolverContext,
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
/// host stays on the session side of the adapter, behind the six ports.
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

impl<L: RequestBoundLifecycle> ResolverContext for RequestBoundAdapter<L> where
    Self: sealed::Sealed + sealed::RequestBoundSealed
{
}

impl<L: RequestBoundLifecycle> IndexedInputs for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn operand_env_epoch(&self) -> super::request_ports::OperandEnvEpoch {
        let w = self.0.host().workspace();
        super::request_ports::OperandEnvEpoch::new(w.published_root(), w.content_generation())
    }
    fn project_stable_key_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::resolution::ProjectStableKey> {
        self.0
            .host()
            .workspace()
            .project_stable_key(self.0.host().resolve_project_for_canonical(canonical)?)
    }
    fn captured_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        self.0.request_view().base().project_identity_for(canonical)
    }
    fn host_view_env_hashes(&self) -> crate::session_view::EnvHashes {
        self.0.host().host_view_env_hashes()
    }
    fn host_view_env_hashes_for(&self, canonical: &str) -> crate::session_view::EnvHashes {
        self.0.host().host_view_env_hashes_for(canonical)
    }
    fn host_view_project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
        self.0.host().host_view_project_identity()
    }
    fn host_view_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        self.0.host().host_view_project_identity_for(canonical)
    }
    fn semantic_compiler_options_for(
        &self,
        canonical: &str,
    ) -> verter_session_query::resolution::SemanticCompilerOptions {
        self.0.host().semantic_compiler_options_for(canonical)
    }
    fn resolve_project_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_workspace::workspace_snapshot::ProjectId> {
        self.0.host().resolve_project_for_canonical(canonical)
    }
    fn declaration_sequence_rank(&self, canonical: &str) -> u32 {
        self.0.host().declaration_sequence_rank(canonical)
    }

    fn engine_policy(&self) -> crate::project_semantic_dispatch::EnginePolicy {
        crate::project_semantic_dispatch::EnginePolicy::from_config(&self.0.host().config)
    }

    fn normalized_analysis_canonical(&self, raw_canonical: &str) -> String {
        self.0
            .host()
            .normalized_analysis_canonical(raw_canonical)
            .into_owned()
    }

    #[inline]
    fn is_request_bound(&self) -> bool {
        true
    }

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedInputRecord>> {
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
        let loaded = self.0.load(canonical_id);
        if loaded {
            self.0.complete_canonical(canonical_id);
        }
        loaded
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
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
        crate::VerterHost::local_type_declaration_id(self.0.host(), canonical_source, resolved_name)
    }

    #[inline]
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        crate::VerterHost::get_whole_hash(self.0.host(), canonical)
    }

    #[inline]
    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        self.0.authoritative_current_content_hash(canonical)
    }

    #[inline]
    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedInputRecord>> {
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
        self.0.artifact_key_for_current_content(canonical)
    }

    #[inline]
    fn observe_materialize_scope(&self, canonical: &str) -> Option<MaterializeScopeObservation> {
        self.0.observe_materialize_scope(self, canonical)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        crate::VerterHost::get_raw_analysis_snapshot(self.0.host(), canonical)
    }
}
impl<L: RequestBoundLifecycle> RouteLookup for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn reverse_dependency_canonicals(&self, canonical: &str) -> Vec<String> {
        self.0.host().workspace().reverse_deps_for(canonical)
    }
    fn observe_owner_import_route_witness(&self, canonical: &str) {
        self.0.host().observe_owner_import_route_witness(canonical);
    }

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<verter_session_query::type_solver::ResolvedRootIdentity> {
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
        self.0
            .resolve_type_dependency_canonical(owner_canonical, import_source)
    }

    #[inline]
    fn routed_shallow_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>> {
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
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
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
        self.0
            .host()
            .workspace()
            .lookup_ambient_symbol(consumer_project, symbol)
    }

    #[inline]
    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str) {
        self.0
            .host()
            .workspace()
            .record_ambient_dependency(consumer_canonical, virtual_id);
    }

    #[inline]
    #[cfg(test)]
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.0.host().workspace().is_workspace_owned(canonical_id)
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        self.0.host().workspace().is_package_backed(canonical_id)
    }
}
impl<L: RequestBoundLifecycle> Cancellation for RequestBoundAdapter<L> where
    Self: sealed::Sealed + sealed::RequestBoundSealed
{
}
impl<L: RequestBoundLifecycle> ExecutionSubmission for RequestBoundAdapter<L>
where
    Self: sealed::Sealed + sealed::RequestBoundSealed,
{
    fn attach_engine(&self) -> crate::project_semantic_dispatch::EngineBinding {
        self.0.host().project_type_store().bind_engine(
            self.0.host().engine_observers(),
            self.0
                .request_view()
                .overlay()
                .input_artifacts
                .macro_selector(
                    #[cfg(test)]
                    Arc::clone(&self.0.host().test_force),
                    #[cfg(test)]
                    Arc::clone(&self.0.host().macro_hot_lowering_count),
                ),
            self.0.host().vue_surface_store_handle(),
            self.0.host().svelte_surface_store_handle(),
        )
    }
}

/// Borrowed-slice variant of [`observe_fan_out`]. Used by
/// `bubble_fact_signature_via_tls` and other warm-hit bubble-up paths.
/// No-op when the stack is empty or `sig` is empty.
#[inline]
pub(crate) fn observe_fan_out_borrowed(
    sig: &[verter_session_query::facts::fact_cache::FactVersionRef],
) {
    fact_tracer_tls::observe_fan_out_borrowed(sig);
}

/// Whether any fact tracer is installed on the current thread's stack.
///
/// A cheap early-out for observation producers whose FACT DERIVATION has a
/// non-trivial cost (a predicate check, a hash derivation, an owned
/// canonical clone): with no active tracer the observation would be a
/// no-op, so the producer skips the derivation entirely.
#[inline]
pub(crate) fn fact_tracer_installed() -> bool {
    fact_tracer_tls::current_tracer().is_some()
}

/// The fact reads a computation made, for replay into scopes that were
/// not live while it ran.
#[derive(Debug, Clone)]
pub(crate) struct RecordedFactReads {
    /// Every distinct fact fanned out while the computation ran: its own
    /// reads, and the receipt of each completed result it consumed.
    pub(crate) facts: std::sync::Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>,
    /// Whether any non-cacheable read was marked, local-only included.
    pub(crate) non_cacheable: bool,
}

/// Where the active scopes stood when a computation began, so its
/// observations can be replaced by its receipt if it completes
/// ([`complete_with_receipt`]).
pub(crate) use fact_tracer_tls::EvidenceMarks;

/// Mark every active tracer and recorder before a computation whose
/// completed result may be answered by a receipt.
#[inline]
pub(crate) fn mark_evidence() -> EvidenceMarks {
    fact_tracer_tls::mark_evidence()
}

/// A computation that ran between `start` and `end` completed with the
/// evidence `reads`: mint its receipt and replace, in every scope still
/// active, what it observed in that range with the receipt. Returns the
/// receipt, the one fact a later consumer of the result observes instead of
/// its reads.
pub(crate) fn complete_with_receipt(
    start: &EvidenceMarks,
    end: &EvidenceMarks,
    reads: &RecordedFactReads,
) -> verter_session_query::facts::fact_cache::FactVersionRef {
    let receipt = verter_session_query::facts::fact_cache::FactVersionRef::Receipt(
        verter_session_query::facts::fact_cache::ResultReceipt::new(reads.facts.to_vec()),
    );
    fact_tracer_tls::collapse_evidence(start, end, &receipt);
    receipt
}

/// Run `work` under a passive fact-read recorder and return what it read.
///
/// Recording changes nothing the installed tracers observe (see
/// [`fact_tracer_tls::FactReadRecorder`]). A producer skips deriving an
/// observation when no tracer is installed, so a recording is complete only
/// if a tracer was installed throughout — the caller checks
/// [`fact_tracer_installed`] first.
pub(crate) fn record_fact_reads<R>(work: impl FnOnce() -> R) -> (R, RecordedFactReads) {
    let recorder = fact_tracer_tls::FactReadRecorder::default();
    let result = {
        let _scope = fact_tracer_tls::install_recorder(&recorder);
        work()
    };
    let (facts, non_cacheable) = recorder.into_parts();
    (
        result,
        RecordedFactReads {
            facts: facts.into(),
            non_cacheable,
        },
    )
}

/// A typed reason a read was NON-CACHEABLE — the discriminant a marking
/// site passes to [`note_non_cacheable_read_fan_out`].
///
/// The tracer records only a boolean (any non-cacheable read refuses
/// shared-cache admission for the enclosing compute); the reason is a
/// structural, self-documenting signal at the marking site so the
/// non-cacheability class is closed by TYPED dispatch rather than an
/// untyped "mark suppress" call. Orthogonal to completeness: marking a
/// read non-cacheable never makes the result `Partial`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NonCacheableReadReason {
    /// A FENCED (ReturnOnly, `store_published == false`) `IndexedReady`
    /// serve consumed inside the tracer scope: the payload was computed
    /// from a served-without-publication (superseded) artifact.
    FencedServe,
    /// A broken decl-body lease pin (`DemandOutcome::LeaseMiss` /
    /// `PreparedDeclOutcome::LeaseMiss` / `LocatorBodyDerefError::LeaseMiss`):
    /// the demanded body did not lower and produced nothing — a TRANSIENT
    /// no-warm signal, recoverable on a later demand under a live lease.
    LeaseMiss,
    /// An unrootable / unadmitted import route: the served value's basis
    /// cannot be soundly fact-rooted for warm admission.
    UnrootableRoute,
    /// An unobservable contributor source-env identity: the exact artifact
    /// key the read served from is unavailable, so the result cannot be
    /// fact-rooted.
    UnobservableSource,
    /// A local semantic-inference safety budget stopped before producing an
    /// authoritative declaration result.
    InferenceBudgetExceeded,
    /// Declaration preparation failed with a typed structural failure. The
    /// failed slot remains vacant and any Option-shaped caller may serve the
    /// failure only as ReturnOnly; it must never publish it as real absence.
    PreparationFailure,
    /// A terminal output-materialization LOST typed degradation: a
    /// present-but-unraisable fold (`None`) or a non-empty degradation
    /// sidecar discarded at the capability-gated terminal unwrap. The
    /// compat tree may still be published for display, but the compute must
    /// never be warm-admitted as a complete result. Orthogonal to
    /// completeness: the result is NOT partial, it is non-cacheable.
    OutputMaterializationLoss,
}

impl NonCacheableReadReason {
    /// A non-cacheable READ taints the derivation basis, rather than merely
    /// declining retention in one cache family, so every current read class
    /// propagates through all enclosing cold-compute scopes.
    #[inline]
    pub(crate) fn propagation(
        self,
    ) -> verter_session_query::facts::fact_read_set::NonCacheablePropagation {
        match self {
            Self::FencedServe
            | Self::LeaseMiss
            | Self::UnrootableRoute
            | Self::UnobservableSource
            | Self::InferenceBudgetExceeded
            | Self::PreparationFailure
            | Self::OutputMaterializationLoss => {
                verter_session_query::facts::fact_read_set::NonCacheablePropagation::Transitive
            }
        }
    }

    /// Whether a COMPLETE result whose compute consumed this read stays
    /// deterministic under the request's immutable view — the axis the
    /// [`ReuseClass`](super::reuse::ReuseClass) splits on.
    ///
    /// Exhaustive by design: a new reason cannot be added without
    /// deciding whether a value built on it may be reused inside the
    /// request. Defaulting a new arm either way is exactly the silent
    /// mistake this match exists to prevent — the permissive default
    /// freezes a recoverable miss, the conservative one re-runs a
    /// deterministic compute on every touch.
    #[inline]
    pub(crate) fn request_reuse(self) -> super::reuse::RequestReuse {
        match self {
            // The payload is a definite answer for this view: a
            // superseded artifact's content, a route whose basis cannot
            // be fact-rooted, a contributor whose source-env identity is
            // unobservable. Re-running inside the same request world
            // reproduces it, so only PUBLICATION is refused.
            Self::FencedServe | Self::UnrootableRoute | Self::UnobservableSource => {
                super::reuse::RequestReuse::Deterministic
            }
            // The payload is DEGRADED and a later demand may improve it:
            // a broken decl-body lease recovers under a live lease, a
            // safety-budget stop and a structural preparation failure
            // both leave a slot vacant rather than answering it.
            Self::LeaseMiss | Self::InferenceBudgetExceeded | Self::PreparationFailure => {
                super::reuse::RequestReuse::Transient
            }
            // A materialization loss is a definite answer under the
            // request's immutable view: the same fold loses the same
            // degradation sidecar on every re-run, so only warm
            // PUBLICATION is refused, never intra-request reuse.
            Self::OutputMaterializationLoss => super::reuse::RequestReuse::Deterministic,
        }
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

/// Mark every active tracer on the current thread's stack as having
/// consumed a NON-CACHEABLE read — the by-value rail enclosing traced cold
/// computes consult to refuse shared-cache admission. `reason` is the typed
/// marking-site discriminant; the tracer records only the boolean, so the
/// reason documents intent and keeps the marking surface typed (not an
/// untyped suppress). No-op when the stack is empty (no traced compute is
/// in scope).
#[inline]
pub(crate) fn note_non_cacheable_read_fan_out(reason: NonCacheableReadReason) {
    // The typed half: every active `RefusalObservationScope` records the
    // REASON, so a producer can classify its result's reuse rail instead
    // of inferring it from a boolean that a fenced serve and a broken
    // lease set identically.
    super::reuse::record_refusal(reason);
    note_non_cacheable_propagation(reason.propagation());
}

/// Apply an already-classified refusal to the active tracer stack. Cache
/// owners use this after a typed `ReturnOnly` result escapes its own tracing
/// scope; ordinary read sites should use [`note_non_cacheable_read_fan_out`]
/// so their closed reason enum selects the propagation policy.
#[inline]
pub(crate) fn note_non_cacheable_propagation(
    propagation: verter_session_query::facts::fact_read_set::NonCacheablePropagation,
) {
    fact_tracer_tls::note_non_cacheable_read(propagation);
}

// ── `with_fact_tracer` installer ──────────────────────────────────────
//
// One cold compute on one thread holds a `FactReadSetCell` for its
// lifetime. The installer plants the cell into a TLS slot and the
// trait method [`ResolverContext::current_fact_tracer`] reads it.
//
// **Why this is NOT an R18 violation.** R18 forbids hidden global
// view state — views must be passed explicitly so concurrent
// sessions don't see each other's overlays. The fact tracer is a
// different substrate: it is per-compute, per-thread instrumentation
// that NEVER stores host state and NEVER influences resolver
// semantics. The TLS slot is a back-end for the
// [`crate::VerterHost::with_fact_tracer`] RAII scope and is reachable
// only through the documented trait method. The contract is:
//   1. Each installer brackets one traced scope on one thread.
//   2. Nesting IS supported: the active tracers form a per-thread STACK
//      (`ACTIVE_TRACERS`), and every observation / non-cacheability mark
//      fans out to ALL active levels, so an inner scope's observations are
//      also seen by every enclosing scope. An inner `with_fact_tracer`
//      pushes a second cell and pops it on drop (RAII, including on
//      unwind). This is what lets the evaluator-scoped carrier observer
//      nest inside a cold build's tracer.
//   3. Readers must go through `ResolverContext::current_fact_tracer`,
//      never through the TLS slot directly. The slot is private to
//      this module.
//
// The trait-method discipline is the architectural contract. The TLS
// implementation is hidden inside this module and is not part of any
// public surface.

/// RAII guard that clears the TLS tracer slot on drop.
///
/// Internal to the `with_fact_tracer` machinery. Returned by
/// [`install_tracer`] so the caller's `with_fact_tracer` closure
/// can hold the guard for the closure's duration.
struct TracerScope;

impl Drop for TracerScope {
    fn drop(&mut self) {
        fact_tracer_tls::clear();
    }
}

/// A fact tracer whose cell a suspendable compute owns: it is installed on
/// the thread only while one of the compute's steps runs
/// ([`Self::install`]), and yields its read set once, when the compute
/// completes.
pub(crate) struct OwnedFactTracer {
    cell: Box<verter_session_query::facts::fact_read_set::FactReadSetCell>,
}

impl OwnedFactTracer {
    pub(crate) fn new(
        basis: verter_session_query::facts::fact_cache::AggregateGenerations,
    ) -> Self {
        let cell = Box::new(verter_session_query::facts::fact_read_set::FactReadSetCell::new());
        cell.set_aggregate_basis(basis);
        Self { cell }
    }

    /// Install the cell until the returned scope drops, unwinding included.
    pub(crate) fn install(&self) -> OwnedTracerScope<'_> {
        fact_tracer_tls::install(&self.cell);
        OwnedTracerScope {
            _tracer: std::marker::PhantomData,
        }
    }

    pub(crate) fn into_read_set(self) -> verter_session_query::facts::fact_read_set::FactReadSet {
        (*self.cell).into_inner()
    }
}

/// One installation of an [`OwnedFactTracer`]; dropping it uninstalls
/// the cell, which it borrows so the cell outlives the installation.
pub(crate) struct OwnedTracerScope<'t> {
    _tracer: std::marker::PhantomData<&'t OwnedFactTracer>,
}

impl Drop for OwnedTracerScope<'_> {
    fn drop(&mut self) {
        fact_tracer_tls::clear();
    }
}

/// Install a request-owned basis without granting host access to the compute.
pub(crate) fn with_fact_tracer_cell<F, R>(
    basis: verter_session_query::facts::fact_cache::AggregateGenerations,
    f: F,
) -> (R, verter_session_query::facts::fact_read_set::FactReadSet)
where
    F: FnOnce(&verter_session_query::facts::fact_read_set::FactReadSetCell) -> R,
{
    let cell = verter_session_query::facts::fact_read_set::FactReadSetCell::new();
    cell.set_aggregate_basis(basis);
    // Push onto the tracer stack. The RAII guard pops on drop
    // (including on panic unwind) so no dangling pointer remains.
    fact_tracer_tls::install(&cell);
    let scope = TracerScope;
    let result = f(&cell);
    // Explicit drop so the stack is popped before we consume
    // `cell.into_inner()`. After this point no `&FactReadSetCell`
    // can leak out of TLS.
    drop(scope);
    (result, cell.into_inner())
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
        fact_tracer_tls::current_tracer()
    }
}

#[cfg(test)]
mod request_bound_adapter_structure_tests {
    use super::ResolverContext;

    fn assert_resolver_context<T: ResolverContext>() {}

    #[test]
    fn request_bound_lifecycles_share_one_resolver_context_implementation() {
        assert_resolver_context::<crate::resolver_core::HostResolverContext<'static>>();
        assert_resolver_context::<crate::resolver_core::SessionResolverContext<'static>>();
    }
}
