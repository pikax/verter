//! sealed `ResolverContext` super-trait.
//!
//! Restricted host facade for resolver-tier code under the
//! `crates/verter_session/src/{resolver_core, meta_resolve,
//! project_semantic_dispatch}/` subtree, plus the two top-level files
//! `component_meta_caches.rs` and `component_meta_materialize.rs`. Every
//! such file routes its host access through this trait; the
//! `no_concrete_verter_host_in_seal_scope` architecture guard
//! (`tests/cases/architecture_guards.rs`) enforces that the seal scope contains
//! zero production references to `crate::VerterHost`.
//!
//! ## Why a separate trait (not super-trait composition)
//!
//! Several existing domain traits — `ComponentMetaResolverHost`,
//! `ComponentMetaRequestHost`, `FallthroughResolverHost`,
//! `FallthroughComputeHost`, `FallthroughRequestHost`,
//! `ExternalMacroTypeCollectorHost`, `FrontierHost`,
//! `DeclarationMetadataResolver` — use **associated types** in method
//! positions (`Snapshot`, `EvalContext`, `View`, `Mode`, `ChildResolution`,
//! `Resolution`, `Error`). A trait with non-dyn-compatible super-traits
//! inherits non-dyn-compatibility, so `&dyn ResolverContext` would not
//! compile if any of these were super-traits. The cascade concern that
//! motivated super-trait composition was a phantom: every `engine.host` /
//! `query_engine.host` callsite in the seal scope passes concrete
//! `&VerterHost` to **concrete-parameter functions** (e.g.,
//! `ProjectSemanticDispatch::new`,
//! `project_expr_class_a_via_dispatch`), not to generic-bound
//! `<H: SomeDomainTrait>` functions. Generic-bound entry points
//! (`run_component_meta_request<H>`, `run_fallthrough_request<H>`) are
//! invoked from inside `impl VerterHost { ... }` blocks where
//! `self: &VerterHost` is concrete and `H` resolves to `VerterHost`.
//!
//! Conclusion: the existing domain traits stay UNCHANGED and remain
//! available as generic bounds; `ResolverContext` is independent and
//! dyn-compatible.
//!
//! ## Sealed against external implementations
//!
//! The trait extends `sealed::Sealed`. The `Sealed` marker is defined in
//! a private inner module and only `VerterHost` registers as `Sealed`
//! (`impl sealed::Sealed for crate::VerterHost {}` at the bottom of this
//! file). External crates therefore cannot implement `ResolverContext`,
//! preserving the host-as-singular-implementer invariant.
//!
//! ## Architectural guarantees (cross-referenced from CLAUDE.md)
//!
//! - **Canonical Dependency Cache Rule:** every method that returns
//!   analysis or prepared-decl state delivers a cache-owned `Arc<T>`. No
//!   raw source is exposed.
//! - **Macro Type Traversal Rule:** symbol-graph walks happen through
//!   `resolve_named_type_export_target` / `resolve_imported_type_root`,
//!   never through ad-hoc parsing.
//! - **Authority Chain:** workspace mutators are NOT exposed; the trait
//!   exposes only the narrow ambient capabilities required by
//!   `ambient_resolve.rs` (`lookup_ambient_symbol`,
//!   `record_ambient_dependency`).
//!
//! Forbidden surface — omitted methods are not only absent here; the
//! architecture guard ensures resolver-tier code cannot escape the
//! trait by naming `VerterHost` directly.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource;
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_workspace::{AmbientSymbolHit, ProjectStableKey};

use crate::host_manage::ValueDeclIdentity;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::project_type_store::{IndexedReady, ProjectTypeStore};
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::{FactVersionRef, ShallowFileState};
use crate::resolver_store::HostStoreView;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::types::Hash16;
use crate::FileAnalysisSnapshot;
use crate::HostConfig;

/// Private marker used to seal `ResolverContext` against external
/// implementations.
mod sealed {
    /// Marker trait `ResolverContext` is sealed against. Only types
    /// inside `verter_session` that implement this marker can implement
    /// `ResolverContext`. Today the only implementer is `VerterHost`.
    pub trait Sealed {}
}

/// A single, tear-free observation of a materialize-memo scope's
/// content identity.
///
/// The materialize-memo publish site
/// (`meta_resolve/materialize/field_types.rs`) needs the scope's
/// content version for two distinct consumers that MUST agree:
///
/// 1. the `NodeScopeId::File { whole_hash }` the materialiser lowers
///    the `TypeExpr` against — the lowered value's semantic identity;
/// 2. the `MaterializeMemoDb` entry's fact-signature self-root — the
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
pub(crate) struct MaterializeScopeObservation {
    /// The scope canonical this observation describes.
    pub canonical_id: Arc<str>,
    /// The single `IndexedReady` artifact whose `whole_hash` roots both
    /// the lowering `NodeScopeId` and the signature self-root.
    pub indexed: Arc<IndexedReady>,
    /// The scope's `SyntacticExportSet` parse fact, pinned to
    /// `indexed.whole_hash` via
    /// [`crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content`].
    /// `None` when the observed version's parse-fact registry is not
    /// recoverable — the publish site then refuses shared-cache
    /// admission while still returning the freshly-computed value.
    pub syntactic_export_set: Option<crate::resolver_core::ParseFactRef>,
}

impl MaterializeScopeObservation {
    /// The observed scope content version. Feeds both the lowering
    /// `NodeScopeId::File { whole_hash }` and the signature self-root —
    /// a single source, so the two cannot disagree.
    #[inline]
    pub(crate) fn whole_hash(&self) -> crate::resolver_core::ResolverHash16 {
        self.indexed.whole_hash
    }
}

/// Restricted host facade for resolver-tier code (`resolver_core/*`,
/// `meta_resolve/*` post-moves, `component_meta_caches.rs`,
/// `component_meta_materialize.rs`, `project_semantic_dispatch/*`).
///
/// `ResolverContext` is the only way for seal-scope code to reach host
/// state at runtime. It is a **flat trait** with no super-traits — see
/// the module-level rationale.
///
/// Visibility is `pub(crate)` because the trait references
/// `ValueDeclIdentity` which is itself `pub(crate)`; a `pub` trait would
/// trip clippy's `private_interfaces` lint. is purely an
/// internal seal — no external integrators construct
/// `&dyn ResolverContext`.
pub(crate) trait ResolverContext: sealed::Sealed {
    // -------- Identity --------------------------------------------

    /// `true` when this context is request-bound — i.e. a
    /// [`crate::resolver_core::HostResolverContext`] or
    /// [`crate::resolver_core::SessionResolverContext`] backed by a
    /// per-request [`HostStoreView`] (and overlay) constructed at the
    /// request entry boundary. `false` for the bare-host
    /// `impl ResolverContext for VerterHost` rail, which rebuilds an
    /// owned view on every `resolver_store_view()` / `store_view()`
    /// call (retired from production resolver-tier code — only the
    /// request-bound rails remain as live entry points).
    ///
    /// Used by `ComponentMetaQueryEngine::new` to bump the
    /// `bare_engine_constructions` diagnostic counter whenever the
    /// engine is bound to a non-request-bound ctx — the empirical
    /// signal the 3-way consult identified as the residual perf-gap
    /// suspect.
    fn is_request_bound(&self) -> bool {
        false
    }

    // -------- Cache accessors --------------------------------------

    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>>;

    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>>;

    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>>;

    /// Materialise (or warm-read) the canonical post-parse artifact,
    /// with the publication status flowed BY VALUE — see
    /// [`crate::host_manage::prepared_decl::IndexedReadyServe`]. This is
    /// the ONLY resolver-tier accessor for a cold/warm `IndexedReady`:
    /// a consumer that derives shared-cache entries from the artifact
    /// gates admission on `serve.store_published`; structurally
    /// read-only consumers take `serve.indexed` (the fenced consumption
    /// still reaches every enclosing traced admission point through the
    /// `note_fenced_serve_fan_out` chokepoint flag).
    fn ensure_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe>;

    fn ensure_loaded(&self, canonical_id: &str) -> bool;

    fn external_type_analysis(&self, canonical_id: &str)
        -> Option<Arc<AnalyzedExternalTypeSource>>;

    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>>;

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
        self.host_for_fact_tracer_install()
            .authoritative_current_content_hash(canonical)
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
    /// materialisation fence seeding (`component_meta_materialize.rs`)
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
    fn indexed_for_current_content(&self, canonical: &str) -> Option<Arc<IndexedReady>> {
        self.host_for_fact_tracer_install()
            .current_content_pinned_indexed(canonical)
    }

    /// Establish ONE tear-free [`MaterializeScopeObservation`] for a
    /// materialize-memo scope canonical.
    ///
    /// The materialize-memo publish site needs the scope's content
    /// version for two consumers that must agree (the lowering
    /// `NodeScopeId` and the signature self-root). This accessor
    /// produces a single `Arc<IndexedReady>` whose `whole_hash` roots
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
        self.host_for_fact_tracer_install()
            .observe_materialize_scope(canonical)
    }

    /// Build an owned [`HostStoreView`] for this context.
    ///
    /// Retained for backward compatibility — production resolver-tier
    /// code on the per-component-meta hot path MUST use
    /// [`Self::store_view`] (a borrow into the request-bound view)
    /// instead so the view is built ONCE at the request boundary and
    /// threaded down.
    ///
    /// `impl ResolverContext for VerterHost::resolver_store_view` rebuilds
    /// a full workspace snapshot on every call — the cost the per-request hoist
    /// hoists to per-request scope.
    #[track_caller]
    fn resolver_store_view(&self) -> HostStoreView;

    /// Borrowed access to the request-bound [`HostStoreView`].
    ///
    /// The view is built ONCE at the request boundary via
    /// [`crate::VerterHost::resolver_store_view`] and threaded through
    /// the resolver pipeline by a
    /// [`crate::resolver_core::HostResolverContext`] (or, for
    /// session-bearing requests, a
    /// [`crate::resolver_core::SessionResolverContext`]).
    /// Resolver-tier consumers consult the borrow on every cache
    /// validation; the per-call full-workspace snapshot the pre-6.c
    /// rail performed is replaced by one snapshot per request.
    ///
    /// The bare `impl ResolverContext for VerterHost::store_view` panics —
    /// a bare `&VerterHost` owns no view to borrow. Production code MUST
    /// construct a `HostResolverContext::new(host, &view)` at the request
    /// entry point. Tests / mocks that want the convenience of a bare host
    /// MUST build a view first and wrap it.
    ///
    /// `#[allow(dead_code)]` is intentional during the 6.c substrate
    /// window — the borrow-returning method has no production callers
    /// yet. The hot-path conversion commit (C) wires consumers; removing
    /// the allow at that point is a stub-prevention follow-up.
    ///
    /// Returns `&dyn StoreView` (not the concrete [`HostStoreView`]) so
    /// the trait stays dyn-compatible AND so a request-bound implementer
    /// can hand back a [`crate::resolver_core::RequestStoreView`]
    /// wrapper that chains a
    /// [`crate::resolver_core::CanonicalCompletionOverlay`] in front of
    /// the request-entry base view. The overlay records additive loads
    /// observed mid-request (`ensure_loaded` / `ensure_indexed_ready_serve`
    /// successes) so the self-root validator does not false-miss on
    /// canonicals loaded after the request-entry snapshot.
    #[allow(dead_code)]
    fn store_view(&self) -> &dyn crate::resolver_core::StoreView;

    fn project_type_store(&self) -> &Arc<ProjectTypeStore>;

    fn config(&self) -> &HostConfig;

    // -------- Symbol / route resolution ----------------------------

    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String);

    fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)>;

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
    ) -> Option<std::sync::Arc<crate::resolver_core::ShallowFileState>>;

    /// resolve a type declaration via the
    /// `meta_resolve::resolve_type_declaration` host-tier helper. Used by
    /// the component-meta query engine and `component_meta_registry` to
    /// resolve named declarations through the host's symbol resolver.
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
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

    // -------- Dispatch facade --------------------------------------

    fn dispatch(&self) -> ProjectSemanticDispatch<'_>;

    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>>;

    // -------- Component-meta-tier bridges --------------------------
    //
    // clippy cleanup — these two trait methods are part of
    // the resolver-context surface contract for component-meta-tier
    // adapters but have no caller in the landed tree. The trait is
    // sealed (only `VerterHost` implements it) and the methods are
    // retained for symmetry with the dependency-fact and analysis-snap
    // bridges defined in the impl block below. `#[allow(dead_code)]` is
    // applied at the trait definition so the corresponding
    // `impl ResolverContext for VerterHost` definitions do not need
    // their own `#[allow]` annotations.

    #[allow(dead_code)]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef>;

    #[allow(dead_code)]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot>;

    // -------- Push-style fact-read tracer (cold-path only) ---------
    //
    // Cold-compute callers record each fact they read from a
    // content-addressed source through [`observe`] /
    // [`observe_borrowed_signature`]. On warm-hit paths no tracer is
    // installed; both convenience methods become observable no-ops.
    //
    // The tracer is owned by an installer that brackets one cold
    // compute on one thread; see
    // [`crate::VerterHost::with_fact_tracer`] for the RAII entry
    // point. The trait method [`current_fact_tracer`] returns the
    // active tracer (if any) without exposing the installer
    // mechanism — implementers may install through TLS, a per-host
    // map, or any other substrate.
    //
    // R24 zero-allocation guarantee: when no tracer is installed,
    // `current_fact_tracer()` returns `None` and the default-impl
    // [`observe`] / [`observe_borrowed_signature`] methods short-
    // circuit without entering any allocator path.
    //
    // R25 cold-path-only contract: the tracer is active on cold
    // compute and write-admission paths only. Warm hits validate
    // their stored `fact_dep_signature` directly without
    // instantiating a tracer.

    /// Return the active fact-read tracer if one is installed.
    ///
    /// Returns `None` on warm-hit paths and on any non-cold-compute
    /// caller. Implementations install a tracer for the duration of
    /// one cold compute via a documented installer (the default
    /// implementer wires this through
    /// [`crate::VerterHost::with_fact_tracer`]).
    ///
    /// Resolver-tier consumers (route-db lookups, materialiser
    /// cache hits, audit-event emitters) call this method to
    /// route their observations onto the active tracer. The
    /// integration-test surface (`tests/`) and public-API mirror
    /// [`crate::VerterHost::current_fact_tracer`] exercise this
    /// method through the same TLS slot the resolver tier uses.
    #[allow(dead_code)]
    fn current_fact_tracer(&self) -> Option<&crate::resolver_core::FactReadSetCell>;

    /// Record one observed fact onto the active tracer, or no-op if
    /// none is active.
    ///
    /// Cold-compute callers MUST call this for each fact they read
    /// from a content-addressed source. Warm-hit fast-path callers
    /// SHOULD NOT call it — the call is cheap, but the design
    /// intent is that warm validation reads the existing
    /// `fact_dep_signature` directly.
    #[inline]
    #[allow(dead_code)]
    fn observe(&self, fact: crate::resolver_core::FactVersionRef) {
        fact_tracer_tls::observe_fan_out(fact);
    }

    /// Bulk-record a routed-hit's existing dep-signature onto the
    /// active tracer.
    ///
    /// Used when a higher-tier cold compute consumes a lower-tier
    /// cached result; the caller inherits the callee's observations
    /// without re-walking them.
    #[inline]
    #[allow(dead_code)]
    fn observe_borrowed_signature(&self, sig: &[crate::resolver_core::FactVersionRef]) {
        fact_tracer_tls::observe_fan_out_borrowed(sig);
    }

    /// Return the active session view for overlay-aware reads, if any.
    ///
    /// The default impl returns `None`; overlay-bearing session contexts
    /// override this to return their `SessionView` so resolver-tier
    /// helpers can read overlay content without carrying an explicit
    /// view parameter. Default is reached today through the
    /// `for_tests::active_session_view_is_none_for_tests` shim in
    /// `lib.rs` (see `tests/cases/g_misc0/resolver_context_active_session_view.rs`).
    fn active_session_view(&self) -> Option<&dyn crate::session_view::SessionView> {
        None
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
    /// [`crate::VerterHost::normalized_analysis_canonical`]; both
    /// implementers ([`crate::VerterHost`] and the overlay-aware
    /// `SessionResolverContext`) resolve through the same host method.
    fn normalized_analysis_canonical<'a>(
        &self,
        raw_canonical: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        self.host_for_fact_tracer_install()
            .normalized_analysis_canonical(raw_canonical)
    }

    /// Reach the concrete `VerterHost` underneath this context.
    ///
    /// Used by Family B/C/D producers (`MaterializeStructureDb`,
    /// `RefCycleResultDb`, `AppConfigNoOverrideProofDb`,
    /// `OwnerImportSurfaceDb`) to call
    /// [`crate::VerterHost::with_fact_tracer`] from inside their
    /// cooperative-admission cold-compute closures. The seal trait
    /// itself cannot expose `with_fact_tracer` directly because
    /// `FnOnce<R>` is non-dyn-compatible; this accessor lets
    /// cold-compute closures install the tracer through the existing
    /// `fact_signature_helpers::install_fact_tracer(host, ...)`
    /// surface without bypassing the seal.
    ///
    /// Both production implementers ([`crate::VerterHost`] and
    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`])
    /// return their inner `&crate::VerterHost`. There is no other
    /// implementer; the seal guarantees the trait contract.
    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost;
}

// Sealed marker — `VerterHost` is the base implementer,
// `HostResolverContext` is the request-bound wrapper that carries a
// borrowed `HostStoreView`, and `SessionResolverContext` is the
// overlay-aware wrapper that delegates every method to a borrowed host
// alongside an overlay-rooted view.
impl sealed::Sealed for crate::VerterHost {}
impl<'a> sealed::Sealed for crate::resolver_core::host_resolver_context::HostResolverContext<'a> {}
impl<'a> sealed::Sealed
    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
{
}

// Compile-time dyn-compatibility check. If a future trait edit
// accidentally introduces an associated type, generic method, or
// `where Self: Sized` bound that breaks dyn-compatibility, this assertion
// fires inside this file at compile time long before a callsite-cascade
// error.
static_assertions::assert_obj_safe!(ResolverContext);

impl ResolverContext for crate::VerterHost {
    // Cache accessors -------------------------------------------------

    #[inline]
    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
        // Bare-host arm — same pattern as the resolver methods below.
        // In production, reaching this means a request-bound caller
        // missed plumbing. Tests route through the `#[cfg(test)]` arm
        // via a one-shot owned-view rebuild.
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::prepared_decl_bundle_with_store_view(self, &view, canonical_id)
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = canonical_id;
            panic!(
                "Architectural violation: bare-host prepared_decl_bundle called from \
                 production; construct HostResolverContext at the request entry"
            );
        }
    }

    #[inline]
    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::prepared_type_decl_with_store_view(
                self,
                &view,
                canonical_id,
                symbol_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (canonical_id, symbol_name);
            panic!(
                "Architectural violation: bare-host prepared_type_decl called from \
                 production; construct HostResolverContext at the request entry"
            );
        }
    }

    #[inline]
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::prepared_value_decl_with_store_view(
                self,
                &view,
                canonical_id,
                symbol_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (canonical_id, symbol_name);
            panic!(
                "Architectural violation: bare-host prepared_value_decl called from \
                 production; construct HostResolverContext at the request entry"
            );
        }
    }

    #[inline]
    fn ensure_indexed_ready_serve(
        &self,
        canonical_id: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical_id)
    }

    #[inline]
    fn ensure_loaded(&self, canonical_id: &str) -> bool {
        crate::VerterHost::ensure_loaded(self, canonical_id)
    }

    #[inline]
    fn external_type_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<AnalyzedExternalTypeSource>> {
        crate::VerterHost::external_type_analysis(self, canonical_id)
    }

    #[inline]
    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>> {
        crate::VerterHost::shallow_file_state(self, canonical_id)
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
    #[track_caller]
    fn resolver_store_view(&self) -> HostStoreView {
        crate::request_context::bump_resolver_store_view_call();
        // Bare-host validation rail (the `!is_request_bound()` arm of the
        // fact-signature helpers, reachable only when no request-bound
        // context was installed — production reaches the `store_view()`
        // panic instead). Hand back the proven-current base view for the
        // fact validation; under churn it falls to the cold-seed's inner.
        crate::VerterHost::resolver_store_view(self).into_owned_view()
    }

    #[inline]
    fn store_view(&self) -> &dyn crate::resolver_core::StoreView {
        // The bare `impl ResolverContext for VerterHost` cannot satisfy
        // a borrow contract — `&VerterHost` owns no `HostStoreView`.
        // Production resolver-tier code MUST construct a
        // `HostResolverContext::new(host, &view, overlay)` at the request
        // boundary and pass `&host_ctx` (or
        // `&host_ctx as &dyn ResolverContext`) into the pipeline.
        //
        // In production (non-test) builds this is an architectural guard
        // — reaching it means the request-binding boundary was
        // bypassed.
        //
        // The audit's claim that "every production cold-compute path
        // now constructs a HostResolverContext" was incomplete: the
        // iter3 bench surfaced bare-host
        // `ComponentMetaQueryEngine::new(self)` constructions in
        // `host_manage/fallthrough.rs`, `host_manage/intrinsic_projection.rs`,
        // `host_manage/eval_env.rs`, and `host_manage/jsdoc_resolve.rs`.
        // Until those construction sites migrate to
        // `HostResolverContext`, callers reachable from those code
        // paths that need a `StoreView` must route through
        // `ctx.resolver_store_view()` (the owned-view rail) rather
        // than `ctx.store_view()`.
        //
        // In test builds the bare-host fallback is supported via a
        // `Box::leak`'d owned view per call: many test fixtures hand a
        // bare `&VerterHost` to `validate_*` helpers that route through
        // `ctx.store_view()`. Leaking gives the borrow a `'static`
        // lifetime — no `unsafe`, no thread-local lifetime hack, no
        // sequential-borrow assumption to maintain. The leak is bounded
        // by the number of test calls to this fallback (a few thousand
        // at most across the entire test suite, ~1KB per view), which
        // is fully acceptable for `cfg(test)`-only paths. Production
        // builds reach the `cfg(not(test))` panic arm below.
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            let leaked: &'static HostStoreView = Box::leak(Box::new(view));
            leaked as &dyn crate::resolver_core::StoreView
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            panic!(
                "ResolverContext::store_view() called on bare &VerterHost — \
                 construct HostResolverContext::new(host, &view, overlay) at the request entry"
            );
        }
    }

    #[inline]
    fn project_type_store(&self) -> &Arc<ProjectTypeStore> {
        crate::VerterHost::project_type_store(self)
    }

    #[inline]
    fn config(&self) -> &HostConfig {
        crate::VerterHost::config(self)
    }

    // Symbol / route resolution --------------------------------------

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        // Bare-host arm: in production, reaching this means a
        // request-bound caller missed plumbing. Tests still route
        // through the `#[cfg(test)]` arm via the one-shot owned-view
        // rebuild — exactly the same dispatch the bare wrapper used
        // to perform inline.
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::resolve_imported_type_root_with_store_view(
                self,
                &view,
                dep_canonical,
                imported_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (dep_canonical, imported_name);
            panic!(
                "Architectural violation: bare-host resolve_imported_type_root called from \
                 production; construct HostResolverContext::new(host, &view, overlay) at the \
                 request entry and route through `ctx.resolve_imported_type_root`"
            );
        }
    }

    #[inline]
    fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::resolve_named_type_export_target_with_store_view(
                self,
                &view,
                dep_canonical,
                requested_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (dep_canonical, requested_name);
            panic!(
                "Architectural violation: bare-host resolve_named_type_export_target called \
                 from production; construct HostResolverContext at the request entry"
            );
        }
    }

    #[inline]
    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::resolve_named_type_export_target_shallow_with_store_view(
                self,
                &view,
                dep_canonical,
                requested_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (dep_canonical, requested_name);
            panic!(
                "Architectural violation: bare-host resolve_named_type_export_target_shallow \
                 called from production; construct HostResolverContext at the request entry"
            );
        }
    }

    #[inline]
    fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        #[cfg(any(test, debug_assertions))]
        {
            let view = crate::VerterHost::resolver_store_view(self).into_owned_view();
            crate::VerterHost::resolve_owner_direct_import_with_store_view(
                self,
                &view,
                owner_canonical,
                local_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (owner_canonical, local_name);
            panic!(
                "Architectural violation: bare-host resolve_owner_direct_import called from \
                 production; construct HostResolverContext at the request entry"
            );
        }
    }

    #[inline]
    fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        crate::VerterHost::resolve_type_dependency_canonical(self, owner_canonical, import_source)
    }

    #[inline]
    fn routed_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        crate::VerterHost::routed_shallow_state(self, canonical_id)
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        // Bare-host arm. The internal walker constructs a
        // `HostComponentMetaResolver` over a `ctx` reference; passing
        // `self` (bare host) would route the walker's
        // `ctx.resolve_named_type_export_target(...)` etc. through the
        // panic-shimmed bare-host trait impl. Tests perform the
        // one-shot owned-view rebuild via `_with_context(host, self)`;
        // production callers go through
        // `HostResolverContext::resolve_type_declaration_for_dep` or
        // `SessionResolverContext::resolve_type_declaration_for_dep`
        // (both route through `_with_context(host, ctx)`).
        #[cfg(any(test, debug_assertions))]
        {
            crate::host_manage::jsdoc_resolve::resolve_type_declaration_with_context(
                self,
                self,
                dep_canonical,
                requested_name,
            )
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            let _ = (dep_canonical, requested_name);
            panic!(
                "Architectural violation: bare-host resolve_type_declaration_for_dep called \
                 from production; construct HostResolverContext at the request entry"
            );
        }
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
    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool {
        self.workspace().is_workspace_owned(canonical_id)
    }

    #[inline]
    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool {
        self.workspace().is_package_backed(canonical_id)
    }

    // Dispatch facade ------------------------------------------------

    #[inline]
    fn dispatch(&self) -> ProjectSemanticDispatch<'_> {
        ProjectSemanticDispatch::new(self)
    }

    #[inline]
    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.project_type_store().semantic_graph().node_data(node)
    }

    // Component-meta-tier bridges ------------------------------------

    #[inline]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef> {
        crate::VerterHost::current_dependency_fact_versions(self, canonical, tracked_deps)
    }

    #[inline]
    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        crate::VerterHost::get_raw_analysis_snapshot(self, canonical)
    }

    // Fact tracer ----------------------------------------------------

    #[inline]
    fn current_fact_tracer(&self) -> Option<&crate::resolver_core::FactReadSetCell> {
        fact_tracer_tls::current_tracer()
    }

    #[inline]
    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost {
        self
    }
}

/// Fan `fact` into every active tracer on the current thread's stack.
///
/// Used by the rewritten `compile_fact_emission` and any other producer
/// that must deliver a single observation to all nested tracer scopes.
/// No-op when the stack is empty.
#[inline]
pub(crate) fn observe_fan_out(fact: crate::resolver_core::FactVersionRef) {
    fact_tracer_tls::observe_fan_out(fact);
}

/// Fan `sig` into every active tracer on the current thread's stack.
///
/// Borrowed-slice variant of [`observe_fan_out`]. Used by
/// `bubble_fact_signature_via_tls` and other warm-hit bubble-up paths.
/// No-op when the stack is empty or `sig` is empty.
#[inline]
pub(crate) fn observe_fan_out_borrowed(sig: &[crate::resolver_core::FactVersionRef]) {
    fact_tracer_tls::observe_fan_out_borrowed(sig);
}

/// Mark every active tracer on the current thread's stack as having
/// consumed a FENCED (ReturnOnly) serve — the by-value rail enclosing
/// traced cold computes consult to refuse shared-cache admission.
/// No-op when the stack is empty (no traced compute is in scope).
#[inline]
pub(crate) fn note_fenced_serve_fan_out() {
    fact_tracer_tls::note_fenced_serve_fan_out();
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
//   1. The installer brackets exactly one cold compute on one thread.
//   2. Nested installers panic — observations must never silently
//      route to a sibling tracer.
//   3. Readers must go through `ResolverContext::current_fact_tracer`,
//      never through the TLS slot directly. The slot is private to
//      this module.
//
// The trait-method discipline is the architectural contract. The TLS
// implementation is hidden inside this module and is not part of any
// public surface.

mod fact_tracer_tls {
    use std::cell::RefCell;

    use smallvec::SmallVec;

    use crate::resolver_core::{FactReadSetCell, FactVersionRef};

    thread_local! {
        /// Per-thread tracer stack.
        ///
        /// Each entry is a raw pointer to the `FactReadSetCell` owned by
        /// one `with_fact_tracer` scope on this thread. The stack allows
        /// nested fact-tracer scopes: the innermost scope sits at the top;
        /// `observe_fan_out*` fans observations into **all** levels so every
        /// outer scope captures the inner scope's observations.
        ///
        /// SAFETY contract: each pointer is valid for exactly the duration of
        /// the `with_fact_tracer` call that installed it. `install` pushes the
        /// pointer and `clear` (called in the RAII drop) pops the top.
        /// Between push and pop no other thread can mutate the TLS slot, and
        /// the `FactReadSetCell` is stack-allocated in `with_fact_tracer` on
        /// the same thread — so the pointee outlives its slot entry.
        ///
        /// `RefCell` storage with a clone-then-release-then-iterate
        /// access pattern (see `observe_fan_out{,_borrowed}` below)
        /// is what makes this design reentrancy-safe: each fan-out
        /// borrows the slot only long enough to clone the small
        /// `SmallVec` of raw pointers, drops the borrow, and iterates
        /// the clone. No borrow is held when the per-cell `observe`
        /// runs, so a re-entrant `install` / `clear` inside an
        /// observer cannot trigger `BorrowMutError`. `Cell::take()`
        /// + `Cell::set()` would also satisfy this contract — and
        /// works with non-`Copy` payloads because `Cell::take()`
        /// internally calls `mem::replace`. The borrow-clone-release
        /// pattern is exercised by
        /// `tests/cases/g_misc0/tracer_stack_reentrant_observe_safe.rs`. All access
        /// is single-threaded (TLS).
        static ACTIVE_TRACERS: RefCell<SmallVec<[*const FactReadSetCell; 8]>> =
            RefCell::new(SmallVec::new());
    }

    /// Push `cell` onto the tracer stack.
    ///
    /// Nesting is intentional: a nested `with_fact_tracer` scope adds its
    /// cell to the stack so `observe_fan_out*` delivers observations to both
    /// the inner scope and all outer scopes simultaneously.
    ///
    /// SAFETY: the caller (`with_fact_tracer`) keeps `cell` alive for the
    /// entire scope duration. `clear` is called on the RAII guard's drop —
    /// even on panic — so the pointer is removed before the cell is freed.
    pub(super) fn install(cell: &FactReadSetCell) {
        ACTIVE_TRACERS.with(|slot| {
            slot.borrow_mut().push(cell as *const FactReadSetCell);
        });
    }

    /// Pop the top-of-stack entry. Called on the installer's `Drop`.
    pub(super) fn clear() {
        ACTIVE_TRACERS.with(|slot| {
            slot.borrow_mut().pop();
        });
    }

    /// Return the top-of-stack tracer, or `None` when the stack is empty.
    ///
    /// Used by existing single-tracer callers that only need the innermost
    /// active scope. These callers write into the top cell; the fan-out
    /// functions below reach all cells.
    #[inline]
    pub(super) fn current_tracer<'a>() -> Option<&'a FactReadSetCell> {
        ACTIVE_TRACERS.with(|slot| {
            let stack = slot.borrow();
            let ptr = stack.last().copied();
            drop(stack);
            match ptr {
                Some(p) if !p.is_null() => {
                    // SAFETY: each live stack entry is installed by
                    // `with_fact_tracer`; the RAII guard (`TracerScope`)
                    // calls `clear()` on drop (including on unwind), so
                    // no dangling pointer can remain on the stack.
                    Some(unsafe { &*p })
                }
                _ => None,
            }
        })
    }

    /// Fan an observed fact into **every** active tracer on the stack.
    ///
    /// Snapshot-then-iterate: collect the pointer set under a borrow,
    /// drop the borrow, then iterate the collected set. No borrow is held
    /// during the `observe` calls, so re-entrant `install`/`clear` calls
    /// from inside a tracer are safe.
    #[inline]
    pub(super) fn observe_fan_out(fact: FactVersionRef) {
        // Collect pointers under a short borrow, then drop the borrow
        // before calling into FactReadSetCell so re-entrant installs
        // from inside an observer don't cause RefCell panics.
        let ptrs: SmallVec<[*const FactReadSetCell; 8]> =
            ACTIVE_TRACERS.with(|slot| slot.borrow().clone());
        for ptr in ptrs {
            if !ptr.is_null() {
                // SAFETY: see module-level SAFETY contract.
                unsafe { &*ptr }.observe(fact.clone());
            }
        }
    }

    /// Fan a borrowed signature into **every** active tracer on the stack.
    #[inline]
    pub(super) fn observe_fan_out_borrowed(sig: &[FactVersionRef]) {
        if sig.is_empty() {
            return;
        }
        let ptrs: SmallVec<[*const FactReadSetCell; 8]> =
            ACTIVE_TRACERS.with(|slot| slot.borrow().clone());
        for ptr in ptrs {
            if !ptr.is_null() {
                // SAFETY: see module-level SAFETY contract.
                unsafe { &*ptr }.observe_borrowed_signature(sig);
            }
        }
    }

    /// Mark **every** active tracer on the stack as having consumed a
    /// FENCED (ReturnOnly, `store_published == false`) serve.
    ///
    /// Called from the serve chokepoints
    /// ([`crate::VerterHost::ensure_indexed_ready_serve`], the overlay
    /// materialiser, and the frontier route reader's per-walk memo) on
    /// the consuming thread, so every enclosing traced cold compute —
    /// the semantic-memo build, the owner-import-surface producer, the
    /// component-meta proof producers — observes the fenced consumption
    /// by value and can refuse shared-cache admission. Same
    /// snapshot-then-iterate reentrancy discipline as
    /// [`observe_fan_out`].
    #[inline]
    pub(super) fn note_fenced_serve_fan_out() {
        let ptrs: SmallVec<[*const FactReadSetCell; 8]> =
            ACTIVE_TRACERS.with(|slot| slot.borrow().clone());
        for ptr in ptrs {
            if !ptr.is_null() {
                // SAFETY: see module-level SAFETY contract.
                unsafe { &*ptr }.note_fenced_serve();
            }
        }
    }
}

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

impl crate::VerterHost {
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
    pub fn with_fact_tracer<F, R>(&self, f: F) -> (R, crate::resolver_core::FactReadSet)
    where
        F: FnOnce() -> R,
    {
        let cell = crate::resolver_core::FactReadSetCell::new();
        // Push onto the tracer stack. The RAII guard pops on drop
        // (including on panic unwind) so no dangling pointer remains.
        fact_tracer_tls::install(&cell);
        let scope = TracerScope;
        let result = f();
        // Explicit drop so the stack is popped before we consume
        // `cell.into_inner()`. After this point no `&FactReadSetCell`
        // can leak out of TLS.
        drop(scope);
        (result, cell.into_inner())
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
    pub fn current_fact_tracer(&self) -> Option<&crate::resolver_core::FactReadSetCell> {
        fact_tracer_tls::current_tracer()
    }
}
