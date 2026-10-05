//! Narrow services selected at the request boundary. Evaluated-node storage
//! is owned by the query facade; these ports only serve inputs and typed work.

use super::request_inputs::{IndexedInputRecord, IndexedInputServe, PreparedInputRecord};
use verter_session_query::inputs::shallow::ShallowInputRecord;

use std::sync::Arc;
use verter_session_query::declarations::DeclarationId;
use verter_session_query::resolution::{AmbientSymbolHit, ProjectStableKey};
use verter_session_query::type_solver::{PreparedTypeDecl, PreparedValueDecl};

use super::resolver_context::MaterializeScopeObservation;
use crate::fact_tracing::note_non_cacheable_read_fan_out;
use crate::resolver_core::ValueDeclIdentity;
use crate::FileAnalysisSnapshot;
use verter_session_query::analysis::types::Hash16;
use verter_session_query::facts::reuse::NonCacheableReadReason;

pub struct OperandEnvEpoch {
    root: Option<Arc<verter_workspace::published_state::PublishedRoot>>,
    generation: u64,
}
impl OperandEnvEpoch {
    pub(super) fn new(
        root: Option<Arc<verter_workspace::published_state::PublishedRoot>>,
        generation: u64,
    ) -> Self {
        Self { root, generation }
    }
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.generation == other.generation
            && match (&self.root, &other.root) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}
pub trait IndexedInputs {
    fn operand_env_epoch(&self) -> OperandEnvEpoch;
    fn captured_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity;
    fn project_stable_key_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::resolution::ProjectStableKey>;
    fn host_view_env_hashes(&self) -> crate::session_view::EnvHashes;
    fn host_view_env_hashes_for(&self, canonical: &str) -> crate::session_view::EnvHashes;
    fn host_view_project_identity(&self) -> crate::file_artifact_store::ProjectIdentity;
    fn host_view_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity;
    fn semantic_compiler_options_for(
        &self,
        canonical: &str,
    ) -> verter_session_query::resolution::SemanticCompilerOptions;
    fn resolve_project_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_workspace::workspace_snapshot::ProjectId>;
    fn declaration_sequence_rank(&self, canonical: &str) -> u32;
    fn engine_policy(&self) -> crate::project_semantic_dispatch::EnginePolicy;

    // -------- Identity --------------------------------------------

    /// `true` when this context is request-bound — i.e. a
    /// [`crate::resolver_core::HostResolverContext`] or
    /// [`crate::resolver_core::SessionResolverContext`] backed by a
    /// per-request [`crate::resolver_store::HostStoreView`] (and overlay) constructed at the
    /// request entry boundary. Production contexts always return `true`;
    /// the default exists for test doubles and the explicit direct-host
    /// test-support seam.
    ///
    /// Used by `ComponentMetaQueryEngine::new` to bump the
    /// `bare_engine_constructions` diagnostic counter whenever the
    /// engine is bound to the explicit direct-host support seam instead of
    /// a production request adapter.
    fn is_request_bound(&self) -> bool {
        false
    }

    // -------- Cache accessors --------------------------------------

    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedInputRecord>>;

    /// Materialise (or warm-read) the canonical post-parse artifact,
    /// with the publication status flowed BY VALUE — see
    /// [`crate::host_manage::prepared_decl::IndexedReadyServe`]. This is
    /// the ONLY resolver-tier accessor for a cold/warm `IndexedReady`:
    /// a consumer that derives shared-cache entries from the artifact
    /// gates admission on `serve.store_published`; structurally
    /// read-only consumers take `serve.indexed` (the fenced consumption
    /// still reaches every enclosing traced admission point through the
    /// `note_non_cacheable_read_fan_out` chokepoint flag).
    fn ensure_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe>;

    /// The canonical's post-parse artifact read through the **base-store**
    /// pin rather than the request view's overlay-priority read.
    ///
    /// [`Self::ensure_indexed_ready_serve`] is the resolver-tier accessor: it
    /// materialises the candidate the CALLING request must see, so a session
    /// request resolves an overlay-priority candidate. Span materialisation at
    /// the consumer boundary is the one documented exception: published member
    /// JSDoc is sliced from `IndexedReady.raw_source` through this accessor, so
    /// the published text is frozen against the base-store artifact and is NOT
    /// re-pointed at a request view's overlay candidate. The read stays
    /// content-pinned per canonical (no `get_any` fallback) and returns the
    /// same owned [`IndexedInputServe`] record, so it adds no borrow and no
    /// ambient host surface to the trait.
    ///
    /// It goes through the SAME host materialisation bridge as
    /// [`Self::ensure_indexed_ready_serve`], so it publishes and observes
    /// identically: for a store-published serve the bridge records the
    /// canonical's parse fact into the active request fact tracer, and a
    /// fenced serve still flows its status by value.
    ///
    /// Only the span-slicing primitive
    /// (`typeinfo::framework_surface::vue_exec::slice_canonical_span`) reads
    /// through it; every resolver-tier semantic read keeps using
    /// [`Self::ensure_indexed_ready_serve`]. That reader set is ENFORCED, not
    /// merely documented: `the_base_store_serve_has_exactly_one_reader` in
    /// `project_global_cache_tests` fails the build if a second call site
    /// appears anywhere in the crate.
    fn base_indexed_ready_serve(&self, canonical_id: &str) -> Option<IndexedInputServe>;

    fn ensure_loaded(&self, canonical_id: &str) -> bool;

    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowInputRecord>>;

    fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId>;

    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16>;

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
    /// The private base adapter delegates to
    /// [`crate::VerterHost::authoritative_current_content_hash`] on the
    /// source backend — the scheduler `parse.whole_hash` gated on the
    /// `DerivedRawState` entry being non-evicted. The overlay-aware
    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`]
    /// overrides it to consult the active [`SessionView`](crate::session_view::SessionView):
    /// an overlay-covered canonical resolves to the overlay's content
    /// hash (the hash the overlay `IndexedReady` was prewarmed under),
    /// not the base host's hash.
    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16>;

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
    /// The base adapter uses the host's pinned-read operation
    /// ([`crate::VerterHost::current_content_pinned_indexed`]) — which
    /// resolves the authoritative current content hash and reads the
    /// artifact store pinned to it, keyed by the **normalised analysis
    /// canonical** so a RAW requested canonical (the architectural id
    /// before an overlay-detection point) does not mis-key for a
    /// non-identity `.js`. The overlay-aware
    /// [`crate::resolver_core::SessionResolverContext`] overrides this
    /// method: it gates the overlay branch on the raw id via
    /// [`crate::host_manage::overlay_materialize::OverlayArtifactIdentity`]
    /// and only falls through to the private base adapter for an
    /// unmasked canonical.
    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedInputRecord>>;

    /// Exact artifact identity for the authority-visible current source.
    fn artifact_key_for_current_content(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::source::artifact_key::FileArtifactKey>;

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
    fn observe_materialize_scope(&self, canonical: &str) -> Option<MaterializeScopeObservation>;

    #[allow(dead_code)]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot>;

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
    fn normalized_analysis_canonical(&self, raw_canonical: &str) -> String;
}

pub trait RouteLookup {
    fn reverse_dependency_canonicals(&self, canonical: &str) -> Vec<String>;
    fn observe_owner_import_route_witness(&self, canonical: &str);
    // -------- Symbol / route resolution ----------------------------

    /// Fact-DISCARDING import-root resolution (final `(canonical, symbol)`
    /// tuple only).
    ///
    /// MUST NOT be used on a memoized-build path (a `LowerLocator` /
    /// read-set-validated cold build): the discarded route-chain facts are
    /// the only proof a barrel/re-export retarget invalidates the enclosing
    /// cache entry — dropping them false-warms the entry when an
    /// intermediate barrel changes while the owner file does not. Memoized
    /// builds call [`Self::resolve_imported_type_root_with_facts`] and
    /// record the returned facts onto the active tracer.
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<verter_session_query::type_solver::ResolvedRootIdentity>;

    /// Like [`Self::resolve_imported_type_root`] but ALSO returns the full
    /// route-chain fact list the resolution observed (every barrel /
    /// re-export participant's version).
    ///
    /// REQUIRED on any memoized-build path: the caller records the returned
    /// facts onto the active fact tracer
    /// ([`Self::observe_borrowed_signature`]) so the enclosing cache
    /// entry's `ReadSetSignature` carries the route proof and a barrel
    /// retarget misses the warm entry.
    fn resolve_imported_type_root_with_facts(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (
        Option<verter_session_query::type_solver::ResolvedRootIdentity>,
        Arc<[verter_session_query::facts::fact_cache::FactVersionRef]>,
    );

    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)>;

    fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)>;

    fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String>;

    /// fetch the routed shallow state for a canonical id.
    /// Used by macro-shape materialisation when re-resolving paths
    /// through cross-file type-import edges.
    fn routed_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<verter_session_query::inputs::shallow::ShallowInputRecord>>;

    /// resolve a type declaration via the
    /// `meta_resolve::resolve_type_declaration` host-tier helper. Used by
    /// the component-meta query engine and `component_meta_registry` to
    /// resolve named declarations through the host's symbol resolver.
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration;

    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity>;

    // -------- Ambient resolution (narrow capabilities) -------------

    fn lookup_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
    ) -> Option<AmbientSymbolHit>;

    fn record_ambient_dependency(&self, consumer_canonical: &str, virtual_id: &str);

    /// Whether `canonical_id` is workspace-owned per the workspace's
    /// resolver-classification (NOT a path-substring check on
    /// `node_modules`). True for workspace package sources, including
    /// pnpm-symlink hops whose realpath resolves into a workspace
    /// project, and workspace-linked packages that happen to live under
    /// `node_modules/`.
    ///
    /// Used by Issue #5 (indexed-access early-out) and Issue #11
    /// (workspace-local canonical cache reuse) to gate fast paths on
    /// actual workspace ownership. Per CLAUDE.md macro-traversal rule,
    /// callers MUST NOT substitute `path.contains("/node_modules/")`
    /// for this method.
    #[cfg(test)]
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool;

    /// Whether `canonical_id` is package-backed per the workspace's
    /// resolver-classification (NOT a path-substring check on
    /// `node_modules`). True only when the realpath sits under
    /// `node_modules/` AND no registered project root claims the file.
    ///
    /// Used by Issue #11 (workspace-local canonical cache reuse) and
    /// the shared symbolic-preservation helper to decide when an
    /// imported ref must materialize canonically vs. stay symbolic.
    /// Callers MUST NOT substitute `path.contains("/node_modules/")`
    /// for this method.
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool;
}

/// The installed cancellation selector is evaluated at each checkpoint, after
/// cooperative job entry, so a cancelled waiter cannot cancel sibling work.
#[derive(Clone, Copy)]
pub struct CancellationCheckpoint {
    _private: (),
}
impl CancellationCheckpoint {
    pub(crate) fn token(self) -> Option<verter_execution::cancellation::CancellationToken> {
        let job = verter_execution::cancellation::current_job_cancellation_token();
        if job
            .as_ref()
            .is_some_and(|token| token.has_registered_owners())
        {
            return job;
        }
        crate::request_context::current_request_cancellation_token().or(job)
    }
    #[cfg_attr(feature = "test-support", track_caller)]
    pub(crate) fn is_cancelled(self) -> bool {
        let cancelled = self.token().is_some_and(|token| token.is_cancelled());
        #[cfg(feature = "test-support")]
        if cancelled {
            crate::for_tests::signature_kernel_bench_support::cancel_trace::observed(
                std::panic::Location::caller(),
            );
        }
        cancelled
    }
}

pub trait Cancellation {
    fn cancellation_checkpoint(&self) -> CancellationCheckpoint {
        CancellationCheckpoint { _private: () }
    }
    /// A port consumer that needs the live token takes it from the checkpoint
    /// (`cancellation_checkpoint().token()`); the port therefore exposes no
    /// second, unread cancellation accessor.
    #[cfg_attr(feature = "test-support", track_caller)]
    fn is_cancelled(&self) -> bool {
        self.cancellation_checkpoint().is_cancelled()
    }
}

pub trait ExecutionSubmission {
    fn attach_engine(&self) -> crate::project_semantic_dispatch::EngineBinding;
}

/// A single population read and its owned query result. The all-space shape
/// fingerprint travels beside the filtered answer, including an empty answer.
pub struct ContributorAnswer {
    pub population_fingerprint: verter_session_query::analysis::types::Hash16,
    pub contributors: verter_session_query::inputs::contributors::SymbolContributors,
}

/// Header facts and exact artifact identity selected by the established
/// augmentation self-heal. No fact registry, source worker, or store escapes.
pub struct AugmenterArtifactAnswer {
    pub augmentations: Arc<Vec<verter_session_query::source::augmentation::ModuleAugmentationFact>>,
    pub refreshed_key: Option<verter_session_query::source::artifact_key::FileArtifactKey>,
}

pub struct TerminalMacroInventory {
    pub(crate) origin_whole_hash: Option<verter_session_query::analysis::types::Hash16>,
    pub(crate) script_analysis:
        Option<Arc<verter_session_query::analysis::script_snapshot::ScriptAnalysisSnapshot>>,
}
pub trait OwnedLowering {
    fn member_presence_for_observed_content(
        &self,
        canonical: &str,
        observed: verter_session_query::analysis::types::Hash16,
        key: verter_session_query::facts::registry::FactKey,
    ) -> Option<bool>;
    fn terminal_macro_inventory(&self, canonical: &str) -> TerminalMacroInventory;
    /// The `<script setup generic="…">` type parameters of the indexed input
    /// `serve` names, re-borrowed from its retained parse. Empty for inputs
    /// without such a clause.
    fn script_setup_type_params(
        &self,
        serve: &IndexedInputServe,
    ) -> Vec<verter_type_expr::TypeParam>;
    fn svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    >;

    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    >;

    /// Consume a typed preparation failure as a ReturnOnly absence at an
    /// Option-shaped semantic boundary. The failure stays explicit through
    /// [`Self::prepared_type_decl`]; this adapter is the sole lossy boundary
    /// and taints every enclosing cacheability scope before returning `None`.
    fn prepared_type_decl_return_only(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        match self.prepared_type_decl(canonical_id, owner, symbol_name) {
            Ok(decl) => decl,
            Err(failure) => {
                note_non_cacheable_read_fan_out(NonCacheableReadReason::PreparationFailure);
                tracing::error!(
                    canonical_id,
                    ?owner,
                    symbol_name,
                    ?failure,
                    "prepared type declaration failed; serving ReturnOnly absence"
                );
                None
            }
        }
    }

    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    >;

    /// Consume a typed preparation failure as a ReturnOnly absence at an
    /// Option-shaped semantic boundary — the value-space mirror of
    /// [`Self::prepared_type_decl_return_only`]. The failure stays explicit
    /// through [`Self::prepared_value_decl`]; this adapter is the sole lossy
    /// boundary and taints every enclosing cacheability scope before
    /// returning `None`. Callers that must preserve the `Failed` distinction
    /// (the `defineExpose` admission gate) call [`Self::prepared_value_decl`]
    /// directly instead.
    fn prepared_value_decl_return_only(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        match self.prepared_value_decl(canonical_id, owner, symbol_name) {
            Ok(decl) => decl,
            Err(failure) => {
                note_non_cacheable_read_fan_out(NonCacheableReadReason::PreparationFailure);
                tracing::error!(
                    canonical_id,
                    ?owner,
                    symbol_name,
                    ?failure,
                    "prepared value declaration failed; serving ReturnOnly absence"
                );
                None
            }
        }
    }

    fn augmentation_index(
        &self,
        target: crate::file_artifact_store::AugmentationTargetKind,
    ) -> (
        crate::file_artifact_store::AugmentationTargetKey,
        Arc<crate::file_artifact_store::AugmenterSet>,
    );
    fn global_contributor_answer(
        &self,
        name: &str,
        space: verter_session_query::facts::SymbolSpace,
    ) -> ContributorAnswer;
    fn contributor_answer(
        &self,
        target: &crate::file_artifact_store::AugmentationTargetKind,
        name: &str,
        allow_automatic_libs: bool,
        space: Option<verter_session_query::facts::SymbolSpace>,
    ) -> ContributorAnswer;
    fn augmenter_artifact_answer(
        &self,
        captured: &verter_session_query::source::artifact_key::FileArtifactKey,
        observed_hash: verter_session_query::analysis::types::Hash16,
    ) -> Option<AugmenterArtifactAnswer>;
    fn refresh_augmentation_keys(
        &self,
        key: &crate::file_artifact_store::AugmentationTargetKey,
        observed: &crate::file_artifact_store::AugmenterSet,
        refreshed: Vec<(
            usize,
            verter_session_query::source::artifact_key::FileArtifactKey,
        )>,
    );
    #[cfg(any(test, feature = "test-support"))]
    fn augmentation_source_env_forced_unobservable(&self) -> bool;
    fn ordered_sfc_structure(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::analysis::component_meta::OrderedSfcStructureAnalysis>;

    fn parse_fact_for_observed_content(
        &self,
        canonical: &str,
        observed_hash: verter_session_query::analysis::types::Hash16,
        key: verter_session_query::facts::registry::FactKey,
        lane: verter_session_query::facts::registry::FactLane,
    ) -> Option<verter_session_query::facts::fact_cache::ParseFactRef>;
    fn indexed_flow_source(
        &self,
        canonical: &str,
    ) -> Option<(
        IndexedInputServe,
        Option<std::sync::Arc<dyn verter_session_query::source::demand::ExpressionSourceDemand>>,
    )>;

    fn indexed_expression_source(
        &self,
        canonical: &str,
    ) -> Option<(
        IndexedInputServe,
        std::sync::Arc<dyn verter_session_query::source::demand::ExpressionSourceDemand>,
    )>;

    fn recover_member_spans(
        &self,
        source: &ShallowInputRecord,
        origin: &verter_type_expr::span_origins::MemberSpansOrigin,
    ) -> verter_type_expr::MemberSpans;

    fn deref_authored_body(
        &self,
        source: &ShallowInputRecord,
        locator: &verter_type_expr::locators::AuthoredBodyLocator,
    ) -> Result<
        verter_session_query::source::deref::DerefedAuthoredBody,
        verter_session_query::source::deref::LocatorBodyDerefError,
    >;

    fn prepared_type_from_input(
        &self,
        input: &PreparedInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    >;
    fn prepared_type_for_projection(
        &self,
        input: &PreparedInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> crate::resolver_core::prepared_decl::PreparedTypeDeclResolution;
    fn prepare_augmentation_type(
        &self,
        input: &PreparedInputRecord,
        scope: &verter_session_query::declarations::AugmentationScopeKind,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> crate::resolver_core::prepared_decl::PreparedDeclOutcome<PreparedTypeDecl>;
    fn prepare_augmentation_value(
        &self,
        input: &PreparedInputRecord,
        scope: &verter_session_query::declarations::AugmentationScopeKind,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> crate::resolver_core::prepared_decl::PreparedDeclOutcome<PreparedValueDecl>;

    #[cfg(test)]
    fn function_program_index(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<verter_session_query::function_program::FunctionProgramIndex>>;

    fn transient_type_parts(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> verter_session_query::source::demand::DemandOutcome<
        crate::decl_body_memo::TransientTypeParts,
    >;
    fn transient_value_parts(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> verter_session_query::source::demand::DemandOutcome<
        crate::decl_body_memo::TransientValueParts,
    >;

    /// The owner's `TypeDecl` with its body already lowered — the port's
    /// eager type-body demand. Its value-decl twin `lowered_value_decl` is the
    /// read path in use. Gated to the `typeinfo::oracle_core` consumer module
    /// (`#[cfg(any(test, feature = "oracle-gen"))]`, see `typeinfo/mod.rs`),
    /// so a shipped build carries no unread method on the port contract.
    #[cfg(any(test, feature = "oracle-gen"))]
    fn lowered_type_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredTypeDecl>>;
    fn lowered_value_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredValueDecl>>;
    fn effective_type_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredTypeDecl>>;
    fn effective_value_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredValueDecl>>;
    fn type_dependencies(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<verter_session_query::inputs::shallow::ClassifiedTypeDeps>>;
    /// The owner's raw-source surfaces in one `SymbolSpace` — the port's
    /// escape-free read for the syntactic symbol inventory. Gated to the
    /// `typeinfo::oracle_core` consumer module
    /// (`#[cfg(any(test, feature = "oracle-gen"))]`, see `typeinfo/mod.rs`),
    /// so a shipped build carries no unread method on the port contract.
    #[cfg(any(test, feature = "oracle-gen"))]
    fn raw_source_surfaces(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        space: verter_parser::utils::oxc::script::raw_surface::SymbolSpace,
    ) -> Option<Arc<Vec<verter_parser::utils::oxc::script::raw_surface::RawSourceSurface>>>;
    fn deref_type_argument(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        locator: &verter_type_expr::locators::TypeArgLocator,
    ) -> Result<
        verter_type_expr::TypeExpr,
        verter_session_query::source::deref::LocatorBodyDerefError,
    >;

    fn lower_authored_body(
        &self,
        locator: &verter_type_expr::locators::AuthoredBodyLocator,
    ) -> verter_session_query::QueryHostServe;

    fn prepare_function_structure(
        &self,
        key: &verter_session_query::flow::bundle::FlowSliceFunctionKey,
    ) -> Result<
        Option<verter_session_query::flow::skeleton::PreparedFunctionBodySkeleton>,
        verter_session_query::flow::binding::FlowBindingMapError,
    >;
}
