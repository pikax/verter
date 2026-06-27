//! Declaration-aware component-meta query engine.
//!
//! `ComponentMetaQueryEngine` is the per-request execution surface for
//! one `get_component_meta()` call. It resolves type declarations
//! lazily from the ctx's prepared-decl bundles. All solve-like
//! operations dispatch through [`ProjectSemanticDispatch`].
//!
//! ## Authority model — what is durable vs. scratch
//!
//! The engine sits **above** the host's authoritative caches and
//! **below** the public component-meta API. It does NOT own any
//! durable cache state. The authoritative caches that survive
//! beyond a single request are listed below; the engine reads from
//! them via [`ResolverContext`] and (where applicable) writes to
//! them through cooperative-admission `post_publish` so concurrent
//! requests collapse onto one cold build.
//!
//! ### Authoritative host-owned caches (durable, dep-validated, reused across queries)
//!
//! - [`MaterializeMemoDb`](crate::component_meta_caches::MaterializeMemoDb)
//!   — interned semantic instantiations keyed by
//!   `(target_decl, mode, args)`. Final-result reuse across requests.
//! - [`ComponentMetaResultDb`](crate::component_meta_caches::ComponentMetaResultDb)
//!   — final `ComponentMetaAnalysis` payloads keyed by `(canonical, profile)`.
//!   `get_component_meta` consults this first; the engine only runs on cold misses.
//! - [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore)
//!   — interned semantic-node arena and resolved-named-type identity map.
//!   Engine subqueries dispatch via
//!   [`ProjectSemanticDispatch`](crate::project_semantic_dispatch::ProjectSemanticDispatch)
//!   which deduplicates against this store.
//! - [`RefCycleResultDb`](crate::component_meta_caches::RefCycleResultDb)
//!   — host-cached transitive cycle-detection BFS results keyed by
//!   parameterized generic helpers. Cooperative-admission cold build.
//! - [`MaterializeStructureDb`](crate::component_meta_caches::MaterializeStructureDb)
//!   — interned structural projections produced by the canonical
//!   materialiser; sole authoritative materialiser cache.
//!
//! All five participate in `ProjectTypeStore`'s invalidation cascade
//! and are fact-validated on warm hit by re-walking each candidate's
//! `read_set_signature.facts` against the live `StoreView`.
//!
//! ### Per-request scratch (NOT promoted, dies with the engine)
//!
//! The engine retains a small set of `RefCell`-wrapped maps used to
//! avoid recomputing the same projection within one request. These
//! are scratch only:
//!
//! - Type-param substitution maps and projection-chain scopes —
//!   per-frame state for the current dispatch path.
//!
//! **None of these are written back to the host store directly.**
//! When durable population is required, the engine uses the ctx's
//! cooperative-admission `post_publish` path so the host store sees
//! exactly one canonical write per cache key.
//!
//! ### Never-promoted results
//!
//! The following partial outcomes MUST NOT be admitted to the
//! authoritative caches above. They produce request-local outputs only
//! and are discarded when the engine drops:
//!
//! - cancelled requests (cooperative cancellation),
//! - superseded results (a later request's input changed before this
//!   one published),
//! - interrupted results (panic / stack overflow / OS error caught by
//!   the cooperative-admission guard),
//! - budget-exceeded results (FuseBudgets tripped before the projection
//!   converged), and
//! - partial results (any path that returned `Opaque(Miss)` or an
//!   intentionally incomplete shape because a strict-legality precondition
//!   was not met).
//!
//! The engine's `post_publish` discipline enforces this: it commits
//! only when the cooperative-admission guard records a complete success.
//!
//! See the `/component-meta` skill for the public API surface and the
//! `/type-resolution` skill for the cross-file resolver query modes.

use std::cell::RefCell;
use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_type_expr::TypeExpr;

use super::declaration_metadata::{
    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    ResolvedTypeDeclaration,
};
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::ResolverContext;
use crate::resolver_core::{FuseBudgets, FuseState};
use crate::semantic_query::SemanticNodeId;

// The output-sink capabilities for this subtree are defined PER-SINK in the
// exact output-SINK modules that project — NOT subtree-wide:
// `MetaQuerySurfaceOutputCap` in `surface.rs` and `MetaQueryRegistryOutputCap`
// in `registry_decl.rs` (each a single-file sink with no production
// submodule). A subtree-wide cap
// (`pub(in crate::resolver_core::component_meta_query_engine)`) would let any
// sibling in this subtree mint it; terminal-sink minting (each mint scope's
// whole reachable production module tree is output-only) makes the
// output-materialization fence compiler-enforced.

// Surface-projection helpers, prepared-substitution
// machinery, and arc cache-key constructors live in the private
// `surface` child module. The `pub(crate) use` block re-exports the
// existing public-API symbols so external `crate::resolver_core::component_meta_query_engine::<name>`
// paths remain stable.
mod helpers;
mod registry_decl;
mod route_keys;
mod shallow_preserve;
mod surface;

// The surface-PROJECTION helpers stay confined to this query-engine subtree:
// `projected_surface_from_semantic_node` (raw `SemanticNodeId` → surface) and
// `surface_view_to_projected_surface` (forgeable `&SurfaceView` → surface) are
// the raw forgeable-input helpers; `projected_surface_to_type_expr` /
// `projected_surface_to_expanded_shape` are their DTO-side companions. None are
// re-exported (the `surface` module is private; in-subtree callers reach them
// via `use super::surface::`). Out-of-subtree callers route through the
// engine's sink-local methods (`dispatch_projected_surface_to_type_expr` /
// `projected_expanded_shape_from_node` / the routed-surface methods).
// `materialize_route_projection_node` is NOT re-exported: it is scoped
// `pub(in …::component_meta_query_engine)` so only in-subtree route/surface
// adapters and the route fixpoint reach the node→`TypeExpr` materialisation
// (via `super::surface::`), compiler-enforcing the sink confinement.
pub(crate) use surface::{
    instantiate_local_generic_ref_published, lower_and_project_to_expanded_node,
    lower_and_project_to_expanded_published, project_admitted_node_to_expanded_node,
    project_class_a_terminal_published, project_expr_surface_expr_node,
    route_projection_node_eq_to_expr, route_projection_nodes_eq, semantic_query_error_raw,
    type_expr_contains_semantic_miss, type_expr_is_budget_exceeded_sentinel,
    type_expr_root_is_unmaterialized_sentinel, AdmittedRouteProjectionNode,
};
// `type_expr_is_expanded_surface` survives only as the `#[cfg(test)]` parity
// ORACLE the raised-shape suite compares the bottom-up `expanded_surface` fact
// against (production gates read the node-domain fact via `shape_engine`), so
// its re-export is test-only.
#[cfg(test)]
pub(crate) use surface::type_expr_is_expanded_surface;
// Re-export ONLY the per-sink output capability TYPES so the
// `output_materialization` owner module can name them for its explicit
// `impl OutputProjector for <Cap>` registration pairs. The `new()`
// CONSTRUCTORS stay leaf-private (`mint: pub(in …::{surface,registry_decl})`),
// so these re-exports do NOT widen who can mint — only who can name the types.
pub(crate) use registry_decl::MetaQueryRegistryOutputCap;
pub(crate) use surface::MetaQuerySurfaceOutputCap;

// Predicate/utility helpers (route-expr surface keys,
// package-canonical predicates, prepared-decl shape predicates,
// registry-symbol resolution with budget) live in the private
// `helpers` child module. All entries are `pub(super)` and used from
// the engine impl in sibling modules plus the inline test module.
#[cfg(test)]
use helpers::type_expr_references_type_params;

pub(crate) const SEMANTIC_MISS: &str = "semanticMiss";
pub(crate) const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
pub(crate) const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";

/// The exact `TypeExpr::Unknown { raw }` prefix `semantic_query_error_raw`
/// emits for a `QueryError::BudgetExceeded` sentinel (`format!("budgetExceeded({:?})", …)`).
/// This is the SINGLE source of truth for the budget-exceeded spelling:
/// the production recognizer (`dispatch_route_expr_is_materialized`) and
/// every test that scans a published surface for a leaked budget sentinel
/// reference this constant, so the spelling can never silently drift.
pub(crate) const BUDGET_EXCEEDED_SENTINEL_PREFIX: &str = "budgetExceeded(";

/// Build an R28 signature for a cache whose validity depends on the
/// IDENTITY of a top-level type at `(canonical, type_name)`. Observes
/// `Export(name)`, `LocalDecl(name)`, and `MemberShape(exporter=name)`
/// facts. The consumer invalidates when the type is added, removed,
/// renamed, or when its member shape changes; editing a single
/// member's body does NOT invalidate.
///
/// The builder is **provenance-pure**: `observed_hash` is the keyed
/// canonical's content version the producer's value was computed
/// against, captured once at the value source. The self-root
/// `FileWholeHash` and all three parse facts are pinned to that
/// observed version — the helper never re-reads current content.
/// Returns [`crate::cache_runtime::SignatureAdmission::NonCacheable`]
/// (refuse shared-cache admission) when the observed version's
/// parse-fact registry cannot be recovered.
pub(crate) fn engine_fact_signature_for_exported_type(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
    observed_hash: crate::resolver_core::ResolverHash16,
) -> crate::cache_runtime::SignatureAdmission {
    crate::fact_signature_helpers::fact_signature_for_exported_type(
        ctx,
        canonical_id,
        type_name,
        verter_semantic::facts::registry::SymbolSpace::Type,
        observed_hash,
    )
}

/// A prepared type declaration bundled with the keyed canonical's
/// observed content version.
///
/// The query-identity cache producers whose value is built from a
/// `prepared_type_decl` read must root the published entry's fact
/// signature on the content version the value was actually built
/// from. Capturing the value and then re-reading the canonical's
/// *current* content hash at signature-build time is a publish race:
/// an `upsert` landing in that window admits a stale value under a
/// fresh signature, and `revalidate_after_compute` (fresh-vs-fresh)
/// cannot catch it.
///
/// `ComponentMetaQueryEngine::observed_prepared_type_decl` returns
/// this wrapper so the producer threads ONE observation — the
/// `whole_hash` baked here — into both the value and the
/// provenance-pure signature builder. The `decl` and the `whole_hash`
/// are sourced from a single prepared-decl bundle, so they are
/// provably the same content version (untorn against a racing
/// `upsert`). `decl` is `Option` because a prepared decl may
/// legitimately be absent for a bundled canonical (the requested
/// symbol does not exist); the absence is still rooted on `whole_hash`
/// so a later declaration is detected.
pub(crate) struct ObservedPreparedTypeDecl {
    /// The prepared type declaration, or `None` when the requested
    /// symbol is absent from the keyed canonical.
    pub(crate) decl:
        Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
    /// The keyed canonical the prepared decl was resolved for.
    pub(crate) canonical_id: String,
    /// The defining-file content version the prepared-decl bundle was
    /// materialised from — the `whole_hash` of the bundle's
    /// `ShallowFileState`, recovered via
    /// `PreparedTypeDeclCache::defining_content_hash`. Both the cache
    /// value and the fact signature root on this one version, and it
    /// is view-correct because the bundle is fetched through the
    /// view-aware `prepared_decl_bundle` accessor.
    pub(crate) whole_hash: crate::resolver_core::ResolverHash16,
}

/// Build the fact signature for a `MaterializeMemoDb` entry.
///
/// A `MaterializeMemoDb` entry caches the materialised form of a type
/// expression in a `scope` canonical. The builder is **provenance-pure**:
/// it never consults the authoritative current-content oracle and
/// never calls a helper that can re-read current content. Every file
/// identity it emits is supplied by the caller as an *observed*
/// value — the content version the materialiser actually worked
/// against.
///
/// The scope's content identity arrives as ONE
/// [`crate::resolver_core::MaterializeScopeObservation`] — a single
/// `Arc<IndexedReady>`. The keyed-scope `whole_hash` and the keyed-scope
/// `SyntacticExportSet` parse fact therefore both descend from the same
/// observation: the builder physically cannot be handed a raw hash
/// from one source and a parse fact from another. The publish site
/// builds the value's `NodeScopeId::File` from the same observation's
/// `whole_hash`, so the memo value and its fact signature root on the
/// identical scope hash — no torn read.
///
/// Parameters:
///
/// - `observed_scope` — the single tear-free scope observation. Its
///   [`crate::resolver_core::MaterializeScopeObservation::whole_hash`]
///   is the keyed-scope self-root hash AND the hash baked into the
///   value's `NodeScopeId::File`.
/// - `observed_scope_syntactic_export_set` — the scope's
///   `SyntacticExportSet` parse fact, pinned to the observation's
///   `whole_hash` (the publish closure unwraps it from
///   `observed_scope.syntactic_export_set` — passing it explicitly
///   keeps the `None`-refuses-admission control flow at the call
///   site). A `debug_assert` confirms it agrees with the observation.
/// - `materialized_dep_signature` — every canonical the materialisation
///   walk observed, each tagged with the
///   [`crate::semantic_query::DepVersion`] the materialiser recorded.
///
/// The keyed scope is self-rooted by an observed-hash `FileWholeHash`
/// plus the observed-version `Parse` fact. Re-reading the scope's
/// *current* hash would be wrong: an edit landing in the race window
/// between materialisation and this signature write-through would
/// otherwise publish the stale `MaterializedOutputTypeExpr` rooted by a
/// fresh-looking current hash, which then validates on warm reads
/// instead of missing.
///
/// Returns `None` when the signature cannot be built strictly enough
/// to admit the entry to the shared memo. A `None` result refuses
/// cache admission only — the caller still returns the
/// freshly-computed `MaterializedOutputTypeExpr`. `None` is returned when:
///
/// - `observed_scope_syntactic_export_set` is a `Parse` fact for a
///   canonical other than the observed scope (caller-supplied
///   observation does not describe the keyed scope), or
/// - an observed dependency names the scope canonical with a
///   `WholeHash` that disagrees with the observation's `whole_hash` (a
///   torn / mixed observation of the scope), or
/// - an observed dependency carries a `RouteGeneration` version (see
///   below).
///
/// Per-`DepVersion` rooting:
///
/// - `DepVersion::WholeHash(observed)` — the materialiser observed
///   that file's content version. The OBSERVED hash is preserved
///   verbatim in the emitted `FileWholeHash`. A dependency entry that
///   names the scope itself is collapsed onto the scope self-root: it
///   must agree with the observation's `whole_hash` or admission is
///   refused.
/// - `DepVersion::ProjectGeneration(observed)` — the materialiser
///   observed the project-wide resolver/config/lib generation, not
///   that file's content. It is rooted by a
///   [`crate::resolver_core::FactVersionRef::ProjectGeneration`]
///   carrying the OBSERVED generation: a project-shape change bumps
///   the counter and rejects the memo. A pure file-content edit does
///   not bump the generation, so this fact does not over-invalidate.
/// - `DepVersion::RouteGeneration(_)` — route generation is not a
///   real validating fact: there is no authoritative route-generation
///   counter and no production emitter. The fact-rail validator
///   rejects it fail-safe (the `RouteGeneration` arm returns `false`)
///   so a stale entry rooted on it cannot survive. Rooting it would
///   be unsound (it cannot detect a content edit to the observed
///   file). The function therefore returns `None` so the entry is NOT
///   admitted to the shared `MaterializeMemoDb`; no production path
///   constructs the variant.
pub(crate) fn engine_fact_signature_for_materialize_memo(
    observed_scope: &crate::resolver_core::MaterializeScopeObservation,
    observed_scope_syntactic_export_set: crate::resolver_core::ParseFactRef,
    materialized_dep_signature: &crate::semantic_query::DepSignature,
) -> crate::cache_runtime::SignatureAdmission {
    use crate::cache_runtime::{NonAdmissionReason, SignatureAdmission};
    use crate::resolver_core::FactVersionRef;
    use crate::semantic_query::DepVersion;

    let scope_canonical_id = observed_scope.canonical_id.as_ref();
    let observed_scope_whole_hash = observed_scope.whole_hash();
    // The observation carries one `Arc<IndexedReady>`; its top-level
    // `whole_hash` and its `shallow_state.whole_hash` are the same
    // parse by construction (`FileArtifactStore` is content-addressed).
    debug_assert_eq!(
        observed_scope.indexed.shallow_state.whole_hash, observed_scope_whole_hash,
        "MaterializeScopeObservation must carry one internally-consistent IndexedReady",
    );

    if observed_scope_syntactic_export_set.canonical_id.as_str() != scope_canonical_id {
        // The supplied parse fact resolves to a DIFFERENT canonical
        // than the keyed scope — provenance is resolved (we have a
        // parse fact), just attributed to the wrong file. This is a
        // self-root / canonical conflict, not unresolved provenance:
        // the fact is fully attributed, only its self-root identity
        // disagrees with the keyed scope. Audit telemetry tracks the
        // two failure modes distinctly.
        return SignatureAdmission::NonCacheable(NonAdmissionReason::SelfRootConflict);
    }

    let mut entries = Vec::with_capacity(2 + materialized_dep_signature.len());

    entries.push(FactVersionRef::FileWholeHash {
        canonical_id: scope_canonical_id.to_string(),
        hash: observed_scope_whole_hash,
    });
    entries.push(FactVersionRef::Parse(observed_scope_syntactic_export_set));

    for (observed_canonical, dep_version) in materialized_dep_signature.iter() {
        match dep_version {
            DepVersion::WholeHash(observed_hash) => {
                if observed_canonical.as_ref() == scope_canonical_id {
                    // The keyed scope is already self-rooted above by
                    // the observed-hash `FileWholeHash`. A dependency
                    // entry for the scope itself must agree with that
                    // single observation; a disagreement is a torn
                    // read and refuses shared admission.
                    if *observed_hash != observed_scope_whole_hash {
                        return SignatureAdmission::NonCacheable(
                            NonAdmissionReason::SelfRootConflict,
                        );
                    }
                    continue;
                }
                entries.push(FactVersionRef::FileWholeHash {
                    canonical_id: observed_canonical.as_ref().to_string(),
                    hash: *observed_hash,
                });
            }
            DepVersion::ProjectGeneration(observed_generation) => {
                entries.push(FactVersionRef::ProjectGeneration {
                    generation: *observed_generation,
                });
            }
            DepVersion::RouteGeneration(_) => {
                // Route generation has no real validating source —
                // refuse shared memo admission rather than rooting
                // the entry with a fact that cannot catch a content
                // edit to the observed canonical.
                return SignatureAdmission::NonCacheable(
                    NonAdmissionReason::RouteGenerationDependency,
                );
            }
        }
    }
    SignatureAdmission::Cacheable(crate::fact_signature_helpers::ReadSetSignature::new(
        std::sync::Arc::from(entries),
    ))
}

/// Build a two-canonical `DepSignature` (used for DB caches whose
/// validity depends on both an active scope and a declaration source).
#[allow(dead_code)]
pub(crate) fn engine_dep_signature_for_two_canonicals(
    ctx: &dyn ResolverContext,
    canonical_a: &str,
    canonical_b: &str,
) -> crate::semantic_query::DepSignature {
    let mut entries: Vec<(std::sync::Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let push = |entries: &mut Vec<_>, c: &str| {
        let whole_hash = ctx
            .shallow_file_state(c)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        entries.push((
            std::sync::Arc::<str>::from(c),
            crate::semantic_query::DepVersion::WholeHash(whole_hash),
        ));
    };
    push(&mut entries, canonical_a);
    if canonical_b != canonical_a {
        push(&mut entries, canonical_b);
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    std::sync::Arc::from(entries.into_boxed_slice())
}

#[cfg(test)]
use std::cell::Cell;

/// Composite-scope context for prepared-member-path projection.
/// Bundles the scopes the route-key leaf stabiliser keeps live:
///
/// - `decl_scope`: the canonical id of the file where the prepared
///   declaration (e.g., `type Button = ComponentConfig<typeof theme>`)
///   was originally defined. Helper-body-internal refs (like the inner
///   `ComponentUI` in `ComponentConfig`'s body) resolve against this
///   scope because that's where the helper imports are visible.
/// - `arg_scope`: the canonical id of the caller — the file that
///   instantiated the prepared decl. `typeof value_ref` references and
///   type arguments passed at the call site resolve in this scope.
///
/// Built and consumed by the route-key leaf stabilisers in
/// `route_keys.rs`: `solve_or_project_prepared_member_leaf_expr`
/// constructs it from the engine's live scope state and
/// [`ComponentMetaQueryEngine::solve_or_project_leaf_expr_with_context`]
/// reads its three scopes for the per-TypeExpr dispatch rules.
#[derive(Debug, Clone)]
struct PreparedProjectionContext {
    decl_scope: String,
    arg_scope: String,
    /// Scopes from outer levels of a declaration-chain projection,
    /// snapshotted from the engine's `projection_chain_scopes` when
    /// `solve_or_project_prepared_member_leaf_expr` builds this context,
    /// so a `TypeOf(value)` reference inside an inner helper body (e.g.,
    /// the lowered `ComponentUI<typeof theme>` inside
    /// `ComponentConfig`'s body, where the original `Button` alias
    /// lives in `button-types.ts`) can fall back through the chain
    /// to find the scope where the value symbol was actually
    /// visible.
    ///
    /// Innermost-first ordering: `chain_scopes[0]` is the scope of the
    /// most recently entered declaration. Deduplicated against
    /// `decl_scope` and `arg_scope` at lookup time.
    chain_scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedImportedRegistrySymbol {
    pub canonical_id: String,
    pub exported_name: String,
    pub body: TypeExpr,
    pub canonical_dependencies: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FastShallowFieldExprExactness {
    Symbolic,
    Concrete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FastShallowFieldExpr {
    pub expr: TypeExpr,
    pub exactness: FastShallowFieldExprExactness,
}

/// Query-local component-meta solve engine.
///
/// Declaration-scoped lookups resolve through dispatch via
/// [`project_type_surface_expr`]. retired the
/// request-scoped owner engine bridge; all solve-like operations now
/// route through `ProjectSemanticDispatch`. Imported registry entries
/// memoize by declaration scope so the same textual reference does not
/// alias across files.
///
/// **Engine-local cache audit.**
///
/// The plan's binary partition (a = request-local non-semantic scratch,
/// b = reusable semantic producer cache subsumed by dispatch) classifies
/// each field as follows. Fields marked **(a)** are scratch state and
/// retained; fields marked **(b)** are pre-lowering-level memos that
/// genuinely complement dispatch's post-lowering memo (the two operate
/// on different identity spaces — `TypeExpr` vs. `SemanticNodeId` — so
/// dispatch cannot subsume them). The CLAUDE.md "ctx-owned cache
/// principle" violation (these are `FxHashMap` rather than DashMap-backed
/// ctx caches) is documented architectural debt distinct from the
/// dispatch-routing scope of this commit; migrating the (b) entries to
/// ctx-owned `DashMap`s is its own follow-up plan.
///
/// | Field | Class | Rationale |
/// |---|---|---|
/// | `ctx` | (a) | Borrowed runtime reference, not a cache. |
/// | `current_prepared_request_root` | (a) | Call-scoped recursion-guard. |
/// | `imported_registry_symbols` | (b) | Caches `(canonical, name) → ResolvedImportedRegistrySymbol` at TypeExpr level. Dispatch's `ResolveDecl` memo operates on `SemanticNodeId`s; cannot subsume the pre-lowering identity. |
/// | `declarations` / `resolvable` / `owner_collection_exprs` | (b) | Same kind — pre-lowering memos keyed on `(canonical, name)` strings. |
/// | `scope_payloads` | (a) | Per-request `Arc<DeclarationScopePayload>` clones; the bundle is ctx-owned, this just reuses the Arc within one request. |
/// | `prepared_type_decls` | (a) | Arc-cache for `Arc<PreparedTypeDecl>` from ctx; no semantic computation — only refcount avoidance. |
/// | `prepared_*_query_count`, `prepared_*_hit_count` | (a) | `#[cfg(test)]` instrumentation counters. |
/// | `fuse_budgets` / `fuse_state` | (a) | Engine-construction-scoped fuse rails (§1.4). |
/// | `projection_chain_scopes` | (a) | Call-scoped scope chain for prepared-route projection. |
///
/// **Audit conclusion:** all (b) producer caches operate at the
/// pre-lowering `TypeExpr` identity space, which dispatch's
/// `SemanticNodeId`-keyed memo cannot subsume. They are NOT dual-path
/// duplicates of dispatch's work; they are a complementary memoization
/// layer. The plan's "delete (b) fields" directive applies only when
/// dispatch can replace the work — for these fields it cannot. The
/// (b) → ctx-owned migration is documented architectural debt
/// (CLAUDE.md ctx-owned cache principle) addressed in a separate
/// follow-up plan.
pub struct ComponentMetaQueryEngine<'a> {
    pub(crate) ctx: &'a dyn ResolverContext,
    // The caches below are read-through views over the host-owned
    // typed DBs on `ProjectTypeStore` (see `crate::component_meta_caches`).
    // Each engine field is a per-request **non-authoritative read-through
    // view** that mirrors the ctx DB result for repeated lookups within
    // one request. `RefCell` provides interior mutability so `&self`
    // lookups can populate the view after a ctx DB hit. NO independent
    // invalidation, NO
    // independent dep_signature, NO entries the ctx DB doesn't have.
    imported_registry_symbols:
        RefCell<FxHashMap<(String, String), Option<ResolvedImportedRegistrySymbol>>>,
    /// Cached type declarations (read-through view; authority is
    /// `ProjectTypeStore::declaration_db()`).
    declarations: RefCell<FxHashMap<(String, String), ResolvedTypeDeclaration>>,
    /// Cached resolvability checks (read-through view; authority is
    /// `ProjectTypeStore::resolvable_db()`).
    resolvable: RefCell<FxHashMap<(String, String), bool>>,
    /// Cached owner collection expressions (read-through view;
    /// authority is `ProjectTypeStore::owner_collection_db()`).
    owner_collection_exprs: RefCell<FxHashMap<String, Option<verter_type_expr::TypeExpr>>>,
    /// Request-local cache of declaration-scope payloads per scope canonical id.
    /// The prepared bundle stays authoritative; this cache only reuses the
    /// bundle-derived names/bindings within one request so repeated projections
    /// do not keep recloning them.
    scope_payloads: FxHashMap<String, Option<std::sync::Arc<DeclarationScopePayload>>>,
    /// Request-local memoization for prepared declaration lookups.
    prepared_type_decls: FxHashMap<
        (String, String),
        Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
    >,
    #[cfg(test)]
    prepared_type_decl_query_count: usize,
    #[cfg(test)]
    #[allow(dead_code)]
    prepared_shared_surface_hit_count: usize,
    #[cfg(test)]
    #[allow(dead_code)]
    prepared_shared_member_hit_count: usize,
    fuse_budgets: FuseBudgets,
    fuse_state: FuseState,
    /// Ambient declaration-scope chain accumulated during
    /// prepared-member-path projection recursion. Innermost entry at
    /// index 0; outermost (originating call's `decl_scope`) at the
    /// end. Used by `solve_or_project_leaf_expr_with_context` to find
    /// the scope where a `TypeOf(value)` reference is visible when
    /// neither `decl_scope` (the current declaration owner) nor
    /// `arg_scope` (the
    /// caller's SFC) contains the value symbol.
    ///
    /// Unread after trampoline
    /// conversion. Field.
    #[allow(dead_code)]
    projection_chain_scopes: Vec<String>,
}

#[cfg(test)]
thread_local! {
    /// Counts how many times `resolve_imported_registry_symbol`'s
    /// producer invokes `resolve_imported_registry_symbol_with_budget`.
    /// The compute-once contract requires the producer to resolve the
    /// imported symbol exactly once per request even when shared-cache
    /// admission is refused — re-running the resolver would consume the
    /// wildcard-route fuse a second time and spuriously resolve to
    /// `None` near `wildcard_route_fanout`.
    static IMPORTED_REGISTRY_RESOLVE_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    /// When set, `resolve_imported_registry_symbol`'s
    /// `get_or_compute_admit` `compute` closure returns
    /// `ComputeAdmission::ReturnOnly` instead of `Cacheable`,
    /// deterministically reproducing the production
    /// cache-admission-refusal contract (the provenance-pure signature
    /// builder returns `None`) without manufacturing a stale observed
    /// hash. The freshly-resolved value is still returned to the
    /// caller — `ReturnOnly` does not poison joiners.
    static FORCE_IMPORTED_REGISTRY_ADMISSION_REFUSAL: Cell<bool> = const { Cell::new(false) };
    /// When `Some`, `resolve_imported_registry_symbol` publishes this
    /// value into the shared `ImportedRegistryDb` AFTER its own `peek`
    /// miss but BEFORE its `get_or_compute_admit` call —
    /// deterministically reproducing a concurrent request that
    /// validated-and-published the same key inside the producer's cold
    /// window. `get_or_compute_admit` then takes its warm-hit
    /// `validate` arm and returns this published value WITHOUT running
    /// the `compute` closure; the producer MUST surface that returned
    /// value.
    static INJECT_IMPORTED_REGISTRY_CONCURRENT_PUBLISH:
        RefCell<Option<ResolvedImportedRegistrySymbol>> = const { RefCell::new(None) };
}

/// Reset the imported-registry resolver invocation counter and read its
/// prior value. Tests call this before a producer invocation and read
/// the counter after.
#[cfg(test)]
pub(crate) fn reset_imported_registry_resolve_invocations_for_tests() {
    IMPORTED_REGISTRY_RESOLVE_INVOCATIONS.with(|n| n.set(0));
}

/// Read the imported-registry resolver invocation counter.
#[cfg(test)]
pub(crate) fn imported_registry_resolve_invocations_for_tests() -> usize {
    IMPORTED_REGISTRY_RESOLVE_INVOCATIONS.with(|n| n.get())
}

/// RAII guard that forces `resolve_imported_registry_symbol`'s shared
/// cache admission to be refused for the current thread until dropped.
#[cfg(test)]
pub(crate) struct ForceImportedRegistryAdmissionRefusalGuard;

#[cfg(test)]
impl Drop for ForceImportedRegistryAdmissionRefusalGuard {
    fn drop(&mut self) {
        FORCE_IMPORTED_REGISTRY_ADMISSION_REFUSAL.with(|f| f.set(false));
    }
}

/// Force `resolve_imported_registry_symbol`'s shared cache admission to
/// be refused for the current thread until the returned guard drops.
#[cfg(test)]
pub(crate) fn force_imported_registry_admission_refusal_for_tests(
) -> ForceImportedRegistryAdmissionRefusalGuard {
    FORCE_IMPORTED_REGISTRY_ADMISSION_REFUSAL.with(|f| f.set(true));
    ForceImportedRegistryAdmissionRefusalGuard
}

/// RAII guard that clears the imported-registry concurrent-publish
/// injection for the current thread on drop.
#[cfg(test)]
pub(crate) struct InjectImportedRegistryConcurrentPublishGuard;

#[cfg(test)]
impl Drop for InjectImportedRegistryConcurrentPublishGuard {
    fn drop(&mut self) {
        INJECT_IMPORTED_REGISTRY_CONCURRENT_PUBLISH.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Arrange for `resolve_imported_registry_symbol` to observe a
/// concurrently-published `ImportedRegistryDb` entry: `symbol` is
/// published into the shared DB after the producer's `peek` miss but
/// before its `get_or_compute`, so `get_or_compute` takes the warm-hit
/// arm. Reproduces — deterministically, single-threaded — the race
/// where another request validated-and-published the key inside this
/// request's cold window.
#[cfg(test)]
pub(crate) fn inject_imported_registry_concurrent_publish_for_tests(
    symbol: ResolvedImportedRegistrySymbol,
) -> InjectImportedRegistryConcurrentPublishGuard {
    INJECT_IMPORTED_REGISTRY_CONCURRENT_PUBLISH.with(|slot| *slot.borrow_mut() = Some(symbol));
    InjectImportedRegistryConcurrentPublishGuard
}

/// Process-global rendezvous barrier for the imported-registry
/// singleflight discriminator. Unlike the thread-local hooks above,
/// this hook crosses threads — the singleflight contract is a
/// cross-thread property, so the test that exercises it spawns real
/// contending threads.
///
/// `resolve_imported_registry_symbol` consults this gate exactly once,
/// at the seam AFTER its `peek` miss and BEFORE `get_or_compute_admit`.
/// When the gate is armed for the request's keyed canonical, the
/// producer blocks on the barrier; every contending thread therefore
/// passes its `peek` miss before any of them enters the cooperative
/// admission slot. That is the precondition the discriminator needs:
/// pre-fix every thread then runs `resolve_imported_registry_symbol_with_budget`
/// independently (the wildcard-route fuse is consumed N times), post-fix
/// exactly one winner runs it inside the singleflight slot (fuse
/// consumed once).
///
/// The gate is keyed to a marker canonical so an unrelated test running
/// concurrently — whose `resolve_imported_registry_symbol` targets a
/// different canonical — sails past untouched.
#[cfg(test)]
static IMPORTED_REGISTRY_POST_PEEK_BARRIER: std::sync::Mutex<
    Option<(String, std::sync::Arc<std::sync::Barrier>)>,
> = std::sync::Mutex::new(None);

/// RAII guard that disarms the imported-registry post-peek barrier on
/// drop so a panicking test cannot leave the gate armed for the next
/// test.
#[cfg(test)]
pub(crate) struct ImportedRegistryPostPeekBarrierGuard;

#[cfg(test)]
impl Drop for ImportedRegistryPostPeekBarrierGuard {
    fn drop(&mut self) {
        *IMPORTED_REGISTRY_POST_PEEK_BARRIER.lock().unwrap() = None;
    }
}

/// Arm the imported-registry post-peek barrier for `marker_canonical`
/// with a `parties`-party rendezvous. Every `resolve_imported_registry_symbol`
/// call whose keyed canonical equals `marker_canonical` blocks on the
/// returned barrier at the post-`peek` seam until `parties` callers
/// have arrived. The returned guard disarms the gate on drop.
#[cfg(test)]
pub(crate) fn arm_imported_registry_post_peek_barrier_for_tests(
    marker_canonical: &str,
    parties: usize,
) -> (
    std::sync::Arc<std::sync::Barrier>,
    ImportedRegistryPostPeekBarrierGuard,
) {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(parties));
    *IMPORTED_REGISTRY_POST_PEEK_BARRIER.lock().unwrap() = Some((
        marker_canonical.to_string(),
        std::sync::Arc::clone(&barrier),
    ));
    (barrier, ImportedRegistryPostPeekBarrierGuard)
}

/// Block on the imported-registry post-peek barrier when it is armed
/// for `canonical_id`. Invoked by `resolve_imported_registry_symbol`
/// exactly once per call, at the seam between the `peek` miss and
/// `get_or_compute_admit`. A no-op when the gate is unarmed or armed
/// for a different canonical.
#[cfg(test)]
pub(crate) fn await_imported_registry_post_peek_barrier_for_tests(canonical_id: &str) {
    let barrier = {
        let slot = IMPORTED_REGISTRY_POST_PEEK_BARRIER.lock().unwrap();
        match slot.as_ref() {
            Some((marker, barrier)) if marker == canonical_id => {
                Some(std::sync::Arc::clone(barrier))
            }
            _ => None,
        }
    };
    if let Some(barrier) = barrier {
        barrier.wait();
    }
}

/// Process-global winner-park gate for the imported-registry
/// singleflight discriminator — the SECOND phase of the deterministic
/// rendezvous that [`IMPORTED_REGISTRY_POST_PEEK_BARRIER`] begins.
///
/// The post-peek barrier guarantees every contending thread is past its
/// `peek` miss before any enters cooperative admission, but it does NOT
/// bound the race INSIDE `cooperative_admit_with_post_publish` between a
/// worker's loop-top `map.get` miss and that worker's claim of the
/// in-flight slot. Under heavy load a worker descheduled in that window
/// can wake to find the slot already retired (the winner published AND
/// retired it), create a FRESH slot, and become a SECOND cold winner —
/// running the fuse-consuming resolution again. That redundant compute
/// is an accepted property of the singleflight primitive (it guarantees
/// at most one ACTIVE compute per slot, not exactly one compute per key
/// for all time), but it breaks the test's exactly-once fuse assertion.
///
/// This gate closes that window deterministically. When armed for the
/// keyed canonical, the cold winner blocks inside its
/// `get_or_compute_admit` compute closure — AFTER it has claimed the
/// in-flight slot (so `claimed == true` is already published and every
/// later arrival is forced onto the joiner branch) and BEFORE it runs
/// the resolution / publishes / retires the slot. The test releases the
/// winner only once it has PROVEN, via
/// [`InflightTable::slot_strong_count`](crate::cooperative_admission::InflightTable::slot_strong_count),
/// that all `WORKERS - 1` joiners have coalesced onto the winner's slot.
/// No worker is then left mid-flight between its map miss and its slot
/// claim, so no second winner can form: exactly one resolution runs and
/// every joiner reuses its published value.
///
/// Keyed to a marker canonical, exactly like the post-peek barrier, so a
/// concurrent unrelated test sails past untouched.
#[cfg(test)]
static IMPORTED_REGISTRY_WINNER_PARK: std::sync::Mutex<
    Option<(String, std::sync::Arc<ImportedRegistryWinnerPark>)>,
> = std::sync::Mutex::new(None);

/// Condvar-backed release latch the parked winner blocks on. The
/// `released` bool closes a lost-wakeup race: the test may set `released`
/// before the winner reaches the park (e.g. the winner is slow to enter
/// compute), in which case the winner observes `released == true` and
/// proceeds without blocking. Either ordering preserves the invariant
/// the discriminator relies on — `release()` is called only AFTER the
/// test observed every joiner coalesce, so the winner can never publish
/// before that point.
#[cfg(test)]
struct ImportedRegistryWinnerPark {
    released: std::sync::Mutex<bool>,
    ready: std::sync::Condvar,
}

#[cfg(test)]
impl ImportedRegistryWinnerPark {
    fn wait(&self) {
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.ready.wait(released).unwrap();
        }
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.ready.notify_all();
    }
}

/// Handle returned to the test for releasing the parked cold winner once
/// the joiner-coalescing rendezvous has completed.
#[cfg(test)]
pub(crate) struct ImportedRegistryWinnerParkHandle(std::sync::Arc<ImportedRegistryWinnerPark>);

#[cfg(test)]
impl ImportedRegistryWinnerParkHandle {
    /// Release the parked cold winner so it runs the resolution and
    /// publishes its value. Idempotent.
    pub(crate) fn release(&self) {
        self.0.release();
    }
}

/// RAII guard that disarms the winner-park gate on drop AND releases any
/// still-parked winner, so a panicking test (e.g. a tripped rendezvous
/// deadline) cannot leave a production thread blocked inside the compute
/// closure or leave the gate armed for the next test.
#[cfg(test)]
pub(crate) struct ImportedRegistryWinnerParkGuard(std::sync::Arc<ImportedRegistryWinnerPark>);

#[cfg(test)]
impl Drop for ImportedRegistryWinnerParkGuard {
    fn drop(&mut self) {
        // Disarm first so no new winner can park, then release any winner
        // already blocked on the gate.
        *IMPORTED_REGISTRY_WINNER_PARK.lock().unwrap() = None;
        self.0.release();
    }
}

/// Arm the imported-registry winner-park gate for `marker_canonical`.
/// The cold winner of a `resolve_imported_registry_symbol` whose keyed
/// canonical equals `marker_canonical` blocks inside its cooperative
/// admission compute closure until the returned handle's `release()` is
/// called. The returned guard disarms the gate (and releases any parked
/// winner) on drop.
#[cfg(test)]
pub(crate) fn arm_imported_registry_winner_park_for_tests(
    marker_canonical: &str,
) -> (
    ImportedRegistryWinnerParkHandle,
    ImportedRegistryWinnerParkGuard,
) {
    let park = std::sync::Arc::new(ImportedRegistryWinnerPark {
        released: std::sync::Mutex::new(false),
        ready: std::sync::Condvar::new(),
    });
    *IMPORTED_REGISTRY_WINNER_PARK.lock().unwrap() =
        Some((marker_canonical.to_string(), std::sync::Arc::clone(&park)));
    (
        ImportedRegistryWinnerParkHandle(std::sync::Arc::clone(&park)),
        ImportedRegistryWinnerParkGuard(park),
    )
}

/// Block the cold winner on the imported-registry winner-park gate when
/// it is armed for `canonical_id`. Invoked by the
/// `resolve_imported_registry_symbol` cooperative-admission compute
/// closure exactly once per cold compute, AFTER the in-flight slot is
/// claimed and BEFORE the resolution runs. A no-op when the gate is
/// unarmed or armed for a different canonical.
#[cfg(test)]
pub(crate) fn await_imported_registry_winner_park_for_tests(canonical_id: &str) {
    let park = {
        let slot = IMPORTED_REGISTRY_WINNER_PARK.lock().unwrap();
        match slot.as_ref() {
            Some((marker, park)) if marker == canonical_id => Some(std::sync::Arc::clone(park)),
            _ => None,
        }
    };
    if let Some(park) = park {
        park.wait();
    }
}

impl<'a> ComponentMetaQueryEngine<'a> {
    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
        // Bump `bare_engine_constructions` whenever the engine is
        // bound to a non-request-bound ctx. Final-state invariant:
        // `0` — every production engine binds to a request-bound
        // ctx (`HostResolverContext` / `SessionResolverContext`).
        if !ctx.is_request_bound() {
            crate::request_context::bump_bare_engine_construction();
        }
        Self {
            ctx,
            imported_registry_symbols: RefCell::new(FxHashMap::default()),
            declarations: RefCell::new(FxHashMap::default()),
            resolvable: RefCell::new(FxHashMap::default()),
            owner_collection_exprs: RefCell::new(FxHashMap::default()),
            scope_payloads: FxHashMap::default(),
            prepared_type_decls: FxHashMap::default(),
            #[cfg(test)]
            prepared_type_decl_query_count: 0,
            #[cfg(test)]
            prepared_shared_surface_hit_count: 0,
            #[cfg(test)]
            prepared_shared_member_hit_count: 0,
            fuse_budgets: FuseBudgets::default(),
            fuse_state: FuseState::default(),
            projection_chain_scopes: Vec::new(),
        }
    }

    /// Returns the cached [`DeclarationScopePayload`] for
    /// `scope_canonical_id`, lazily loading the underlying
    /// `prepared_decl_bundle` on first access ( D35:
    /// promoted to `pub(crate)` so the session-layer materialize wrapper
    /// in `meta_resolve.rs` can reuse the cache without re-walking the
    /// bundle).
    pub(crate) fn scope_payload_for_scope(
        &mut self,
        scope_canonical_id: &str,
    ) -> Option<std::sync::Arc<DeclarationScopePayload>> {
        let ctx = self.ctx;
        self.scope_payloads
            .entry(scope_canonical_id.to_string())
            .or_insert_with(|| {
                ctx.prepared_decl_bundle(scope_canonical_id)
                    .or_else(|| {
                        // Lazy first-time loading for dependency files discovered
                        // during resolution. This is NOT re-walking cached state —
                        // it triggers the normal load/parse/cache pipeline for files
                        // not yet in the ctx's cache.
                        ctx.ensure_loaded(scope_canonical_id)
                            .then(|| ctx.prepared_decl_bundle(scope_canonical_id))
                            .flatten()
                    })
                    .map(|bundle| {
                        std::sync::Arc::new(DeclarationScopePayload::from_bundle(&bundle))
                    })
            })
            .clone()
    }
}

fn local_type_symbol_metadata_for_known_source(
    ctx: &dyn ResolverContext,
    canonical_source: &str,
    resolved_name: &str,
) -> Option<ResolvedLocalTypeSymbolMetadata> {
    let analysis = ctx.external_type_analysis(canonical_source)?;
    let symbol = analysis.local_type_symbol(resolved_name)?;
    let kind = match symbol.kind {
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::TypeAlias => {
            ResolvedDeclarationKind::TypeAlias
        }
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::Interface => {
            ResolvedDeclarationKind::Interface
        }
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSymbolKind::Class => {
            ResolvedDeclarationKind::Class
        }
    };
    Some(ResolvedLocalTypeSymbolMetadata {
        kind,
        span: symbol.span,
    })
}

struct DirectPreparedDeclarationResolver<'a> {
    ctx: &'a dyn ResolverContext,
}

impl DeclarationMetadataResolver for DirectPreparedDeclarationResolver<'_> {
    fn resolve_export_target(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<super::declaration_metadata::ResolvedExportTarget> {
        None
    }

    fn get_export_span_follow_reexports(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<verter_span::Span> {
        None
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        self.ctx
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<super::declaration_metadata::ResolvedLocalTypeSymbolMetadata> {
        local_type_symbol_metadata_for_known_source(self.ctx, canonical_source, resolved_name)
    }
}

fn empty_semantic_args() -> std::sync::Arc<[SemanticNodeId]> {
    std::sync::Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice())
}

#[cfg(test)]
mod tests;
