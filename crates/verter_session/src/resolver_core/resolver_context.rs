//! Phase 10a — sealed `ResolverContext` super-trait.
//!
//! Restricted host facade for resolver-tier code under the
//! `crates/verter_session/src/{resolver_core, meta_resolve,
//! project_semantic_dispatch}/` subtree, plus the two top-level files
//! `component_meta_caches.rs` and `component_meta_materialize.rs`. Every
//! such file routes its host access through this trait; the
//! `no_concrete_verter_host_in_seal_scope` architecture guard
//! (`tests/architecture_guards.rs`) enforces that the seal scope contains
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
//! Forbidden surface — see the sub-plan §10a.1.D. Omitted methods are
//! not only absent here; the architecture guard ensures resolver-tier
//! code cannot escape the trait by naming `VerterHost` directly.

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
use crate::resolver_core::{FactVersionRef, ShallowFileState};
use crate::resolver_store::HostStoreView;
use crate::semantic_query::{DepSignature, SemanticNodeData, SemanticNodeId};
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
/// trip clippy's `private_interfaces` lint. Phase 10a is purely an
/// internal seal — no external integrators construct
/// `&dyn ResolverContext`.
pub(crate) trait ResolverContext: sealed::Sealed {
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

    fn ensure_indexed_ready(&self, canonical_id: &str) -> Option<Arc<IndexedReady>>;

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

    fn resolver_store_view(&self) -> HostStoreView;

    fn project_type_store(&self) -> &Arc<ProjectTypeStore>;

    fn config(&self) -> &HostConfig;

    fn analyzed_macro_snapshot(&self, canonical_id: &str) -> Option<Arc<ScriptAnalysisSnapshot>>;

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

    /// Phase 10a — resolve a route-type-edge through the workspace's
    /// type-import phase. Used by macro-shape materialisation
    /// (`meta_resolve/materialize/macro_shapes.rs`) when projecting
    /// runtime values into structural surfaces.
    fn resolve_route_type_edge(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String>;

    /// Phase 10a — fetch the routed shallow state for a canonical id.
    /// Used by macro-shape materialisation when re-resolving paths
    /// through cross-file type-import edges.
    fn route_owned_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::ShallowFileState>>;

    /// Phase 10a — resolve a type declaration via the
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

    /// Workspace content generation — narrow capability used by
    /// `component_meta_caches.rs::peek` for the validated_at_generation
    /// fast path. Together with `lookup_ambient_symbol` and
    /// `record_ambient_dependency`, this replaces the broad `workspace()`
    /// accessor (which would expose the full `WorkspaceAccess` mutator
    /// surface to seal-scope code).
    fn workspace_content_generation(&self) -> u64;

    // -------- Dispatch facade --------------------------------------

    fn dispatch(&self) -> ProjectSemanticDispatch<'_>;

    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>>;

    // -------- Cache validation -------------------------------------

    fn validate_dep_signature(&self, signature: &DepSignature) -> bool;

    // -------- Component-meta-tier bridges --------------------------
    //
    // Post-cutover clippy cleanup — these two trait methods are part of
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
}

// Sealed marker — only `VerterHost` may implement `ResolverContext`.
impl sealed::Sealed for crate::VerterHost {}

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
        crate::VerterHost::prepared_decl_bundle(self, canonical_id)
    }

    #[inline]
    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedTypeDecl>> {
        crate::VerterHost::prepared_type_decl(self, canonical_id, symbol_name)
    }

    #[inline]
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<PreparedValueDecl>> {
        crate::VerterHost::prepared_value_decl(self, canonical_id, symbol_name)
    }

    #[inline]
    fn ensure_indexed_ready(&self, canonical_id: &str) -> Option<Arc<IndexedReady>> {
        crate::VerterHost::ensure_indexed_ready(self, canonical_id)
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
    fn resolver_store_view(&self) -> HostStoreView {
        crate::VerterHost::resolver_store_view(self)
    }

    #[inline]
    fn project_type_store(&self) -> &Arc<ProjectTypeStore> {
        crate::VerterHost::project_type_store(self)
    }

    #[inline]
    fn config(&self) -> &HostConfig {
        crate::VerterHost::config(self)
    }

    #[inline]
    fn analyzed_macro_snapshot(&self, canonical_id: &str) -> Option<Arc<ScriptAnalysisSnapshot>> {
        crate::VerterHost::analyzed_macro_snapshot(self, canonical_id)
    }

    // Symbol / route resolution --------------------------------------

    #[inline]
    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        crate::VerterHost::resolve_imported_type_root(self, dep_canonical, imported_name)
    }

    #[inline]
    fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        crate::VerterHost::resolve_named_type_export_target(self, dep_canonical, requested_name)
    }

    #[inline]
    fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        crate::VerterHost::resolve_named_type_export_target_shallow(
            self,
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
        crate::VerterHost::resolve_owner_direct_import(self, owner_canonical, local_name)
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
    fn resolve_route_type_edge(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        crate::VerterHost::resolve_route_type_edge(self, owner_canonical, source_specifier)
    }

    #[inline]
    fn route_owned_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        crate::VerterHost::route_owned_shallow_state(self, canonical_id)
    }

    #[inline]
    fn resolve_type_declaration_for_dep(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        crate::host_manage::jsdoc_resolve::resolve_type_declaration(
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
    fn workspace_content_generation(&self) -> u64 {
        self.workspace().content_generation()
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

    // Cache validation -----------------------------------------------

    #[inline]
    fn validate_dep_signature(&self, signature: &DepSignature) -> bool {
        crate::host_manage::dep_signature_valid_for_host(signature, self)
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
}
