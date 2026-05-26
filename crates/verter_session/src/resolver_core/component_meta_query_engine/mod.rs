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
//! - `prepared_surface_cache` — read-through view of the ctx's
//!   `prepared_surface_db`; mirrors the durable result for the current
//!   request only.
//! - `routed_expr_surface_cache` — same shape, for routed-expr surfaces.
//! - `prepared_member_cache` — request-local memo of prepared-member
//!   projections.
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
use std::hash::Hash;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_solver::query_engine::ProjectedMember;
use verter_type_expr::TypeExpr;

use super::declaration_metadata::{
    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    ResolvedTypeDeclaration,
};
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::ResolverContext;
use crate::resolver_core::{FuseBudgets, FuseState};
use crate::semantic_query::SemanticNodeId;

// Surface-projection helpers, prepared-substitution
// machinery, and arc cache-key constructors live in the private
// `surface` child module. The `pub(crate) use` block re-exports the
// existing public-API symbols so external `crate::resolver_core::component_meta_query_engine::<name>`
// paths remain stable.
mod helpers;
mod prepared_surface;
mod registry_decl;
mod route_keys;
mod routed_expr;
mod shallow_preserve;
mod surface;

pub(crate) use surface::{
    projected_surface_from_semantic_node, projected_surface_to_expanded_shape,
    projected_surface_to_type_expr, semantic_query_error_raw, surface_view_to_projected_surface,
    type_expr_contains_semantic_miss, type_expr_has_any_object_arm, type_expr_is_expanded_surface,
};

// Items needed inside this module (mod.rs) — engine impl methods and
// supporting code. All `pub(super)` in surface.rs.
#[cfg(test)]
use surface::type_expr_references_substitutions;
use surface::{
    apply_type_param_substitutions, build_default_type_param_substitutions,
    PreparedSurfaceProjection,
};

// Predicate/utility helpers (route-expr surface keys,
// package-canonical predicates, prepared-decl shape predicates,
// registry-symbol resolution with budget) live in the private
// `helpers` child module. All entries are `pub(super)` and used only
// from the engine impl in this file plus the inline test module.
use helpers::is_package_source;
#[cfg(test)]
use helpers::type_expr_references_type_params;

pub(crate) const SEMANTIC_MISS: &str = "semanticMiss";
pub(crate) const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
pub(crate) const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";

/// Build an R28 path-precise `Arc<[FactVersionRef]>` for a cache
/// whose validity depends on a single MEMBER of an exporter type.
/// Observes `MemberPresence(exporter, member)` and `Member(exporter,
/// member)` facts in the `Type` symbol space so the consumer
/// invalidates ONLY when the named member's header or body changes;
/// sibling-member edits in the same file keep the consumer warm.
///
/// The builder is **provenance-pure**: `observed_hash` is the keyed
/// canonical's content version the producer's value was computed
/// against, captured once at the value source. The self-root
/// `FileWholeHash` and both parse facts are pinned to that observed
/// version — the helper never re-reads current content. Returns
/// `None` (refuse shared-cache admission) when the observed version's
/// parse-fact registry cannot be recovered.
pub(crate) fn engine_fact_signature_for_canonical_member(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exporter: &str,
    member: &str,
    observed_hash: crate::resolver_core::ResolverHash16,
) -> Option<std::sync::Arc<[crate::resolver_core::FactVersionRef]>> {
    crate::fact_signature_helpers::fact_signature_for_canonical_member(
        ctx,
        canonical_id,
        exporter,
        member,
        verter_semantic::facts::registry::SymbolSpace::Type,
        observed_hash,
    )
}

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
/// Returns `None` (refuse shared-cache admission) when the observed
/// version's parse-fact registry cannot be recovered.
pub(crate) fn engine_fact_signature_for_exported_type(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
    observed_hash: crate::resolver_core::ResolverHash16,
) -> Option<std::sync::Arc<[crate::resolver_core::FactVersionRef]>> {
    crate::fact_signature_helpers::fact_signature_for_exported_type(
        ctx,
        canonical_id,
        type_name,
        verter_semantic::facts::registry::SymbolSpace::Type,
        observed_hash,
    )
}

/// Build the fact signature for a `PreparedTargetDb` entry.
///
/// A `PreparedTargetDb` entry maps `(active_scope, target_name)` to a
/// resolved `(canonical, symbol)` pair. The entry has up to THREE
/// self-roots: the active scope, the original declaring canonical, AND
/// — when the requested name re-exports through an intermediate module
/// to a third file — the FINAL routed declaring canonical. The
/// resolved target depends on the top-level identity of `target_name`
/// in `active_scope`, on the original declaring `(decl_canonical,
/// decl_symbol)`, and on the routed `(routed_canonical, routed_symbol)`.
/// A content edit to ANY of the three files shifts its self-root
/// `FileWholeHash` and rejects the entry.
///
/// The builder is **provenance-pure**: `observed_active_scope_hash`,
/// `observed_decl_hash`, and (when present) the routed canonical's
/// observed hash are the keyed/declaring canonicals' content versions
/// the producer's value was resolved against, each captured once at
/// the value source — the routed canonical's hash comes from the
/// prepared-decl bundle actually used for the value
/// ([`crate::resolver_core::prepared_decl::PreparedDeclBundle::owner_whole_hash`]),
/// NOT a current-content re-read. Each
/// `engine_fact_signature_for_exported_type` sub-signature is pinned
/// to its own observed hash. Returns `None` (refuse shared-cache
/// admission) when any observed version's parse-fact registry cannot
/// be recovered.
///
/// `routed_decl` is `Some((routed_canonical, routed_symbol,
/// observed_routed_hash))` when the resolved declaring canonical
/// differs from the original declaring canonical (a re-export hop);
/// `None` when no re-route occurred (or the routed canonical equals
/// the active scope / original declaring canonical, already rooted).
pub(crate) fn engine_fact_signature_for_prepared_target(
    ctx: &dyn ResolverContext,
    active_scope: &str,
    target_name: &str,
    observed_active_scope_hash: crate::resolver_core::ResolverHash16,
    decl_canonical: &str,
    decl_symbol: &str,
    observed_decl_hash: crate::resolver_core::ResolverHash16,
    routed_decl: Option<(&str, &str, crate::resolver_core::ResolverHash16)>,
) -> Option<std::sync::Arc<[crate::resolver_core::FactVersionRef]>> {
    let mut entries: Vec<crate::resolver_core::FactVersionRef> =
        engine_fact_signature_for_exported_type(
            ctx,
            active_scope,
            target_name,
            observed_active_scope_hash,
        )?
        .to_vec();
    if decl_canonical != active_scope || decl_symbol != target_name {
        entries.extend(
            engine_fact_signature_for_exported_type(
                ctx,
                decl_canonical,
                decl_symbol,
                observed_decl_hash,
            )?
            .iter()
            .cloned(),
        );
    }
    // The FINAL routed declaring canonical — the third self-root the
    // cache key never encodes. Root it only when it is a genuinely
    // distinct file: a routed canonical equal to the active scope or
    // the original declaring canonical is already rooted above.
    if let Some((routed_canonical, routed_symbol, observed_routed_hash)) = routed_decl {
        if routed_canonical != active_scope && routed_canonical != decl_canonical {
            entries.extend(
                engine_fact_signature_for_exported_type(
                    ctx,
                    routed_canonical,
                    routed_symbol,
                    observed_routed_hash,
                )?
                .iter()
                .cloned(),
            );
        }
    }
    Some(std::sync::Arc::from(entries))
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
/// otherwise publish the stale `MaterializedTypeExpr` rooted by a
/// fresh-looking current hash, which then validates on warm reads
/// instead of missing.
///
/// Returns `None` when the signature cannot be built strictly enough
/// to admit the entry to the shared memo. A `None` result refuses
/// cache admission only — the caller still returns the
/// freshly-computed `MaterializedTypeExpr`. `None` is returned when:
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
) -> Option<std::sync::Arc<[crate::resolver_core::FactVersionRef]>> {
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
        return None;
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
                        return None;
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
                return None;
            }
        }
    }
    Some(std::sync::Arc::from(entries))
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
/// Bundles the two scopes the prepared-route walker keeps live:
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
/// See [`ComponentMetaQueryEngine::solve_or_project_leaf_expr_with_context`]
/// for the per-TypeExpr dispatch rules.
//
// Retained `#[allow(dead_code)]` for diagnostic / future re-entry use;
// the prepared-route walker now consumes the constituent scopes
// directly via [`ProjectionChainScopes`] / the `chain_scopes` field
// rather than through this composite.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PreparedProjectionContext {
    decl_scope: String,
    arg_scope: String,
    /// Scopes from outer levels of a declaration-chain projection.
    /// Populated as the recursion descends from
    /// `project_prepared_member_path_route_projection_from_*` so a
    /// `TypeOf(value)` reference inside an inner helper body (e.g.,
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum PreparedSubstitutionKey {
    Empty,
    Entries(Vec<(String, TypeExpr)>),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedSurfaceCacheKey {
    canonical_id: String,
    symbol_name: String,
    substitutions: PreparedSubstitutionKey,
    /// See
    /// [`crate::resolver_core::cache_keys::PreparedSurfaceCacheKey::from_root_body`].
    /// The engine-internal `RefCell`-backed read-through view mirrors
    /// the ctx-owned key shape exactly, so two distinct entry contexts
    /// share NO scratch state inside one request.
    from_root_body: bool,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedMemberCacheKey {
    canonical_id: String,
    symbol_name: String,
    member_name: String,
    kind: PreparedMemberCacheKind,
    substitutions: PreparedSubstitutionKey,
    /// See
    /// [`crate::resolver_core::cache_keys::PreparedMemberCacheKey::from_root_body`].
    from_root_body: bool,
}

// `InheritedRoute` is no longer
// constructed after trampoline conversion. Variant
// §F call-graph closure.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum PreparedMemberCacheKind {
    Requested,
    #[allow(dead_code)]
    InheritedRoute,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedTargetCacheKey {
    active_scope_canonical_id: String,
    decl_canonical_id: String,
    decl_symbol_name: String,
    requested_name: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RoutedExprSurfaceCacheKey {
    scope_canonical_id: String,
    root_symbol: String,
    route: super::RouteDemand,
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
/// | `prepared_surface_cache` / `prepared_member_cache` / `prepared_target_cache` / `routed_expr_surface_cache` | (b) | All four are pre-lowering route projections — same justification as above. |
/// | `prepared_type_decls` | (a) | Arc-cache for `Arc<PreparedTypeDecl>` from ctx; no semantic computation — only refcount avoidance. |
/// | `materialize_memo` | (b) | — `(scope, expr, navigate_flag) → MaterializedTypeExpr` memo. Dispatch's post-lowering memo cannot replace this because the key is the un-lowered `TypeExpr`. |
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
    current_prepared_request_root: Option<String>,
    // The 10 caches below are read-through views over the host-owned
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
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_surface_db()`.
    ///
    /// Unread after trampoline
    /// conversion of retired surface methods. Field
    /// §F call-graph closure.
    #[allow(dead_code)]
    prepared_surface_cache: RefCell<FxHashMap<PreparedSurfaceCacheKey, PreparedSurfaceProjection>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_member_db()`.
    prepared_member_cache: RefCell<FxHashMap<PreparedMemberCacheKey, Option<ProjectedMember>>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_target_db()`.
    prepared_target_cache: RefCell<FxHashMap<PreparedTargetCacheKey, Option<(String, String)>>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::routed_expr_surface_db()`.
    ///
    /// Unread after trampoline
    /// conversion of retired surface methods. Field
    /// §F call-graph closure.
    #[allow(dead_code)]
    routed_expr_surface_cache: RefCell<FxHashMap<RoutedExprSurfaceCacheKey, TypeExpr>>,
    /// Request-local memoization for prepared declaration lookups.
    prepared_type_decls: FxHashMap<
        (String, String),
        Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
    >,
    /// Read-through view; authority is
    /// `ProjectTypeStore::materialize_memo_db()`.
    pub(crate) materialize_memo: RefCell<
        FxHashMap<
            (String, verter_type_expr::TypeExpr, bool),
            crate::project_semantic_dispatch::raise::MaterializedTypeExpr,
        >,
    >,
    #[cfg(test)]
    prepared_type_decl_query_count: usize,
    #[cfg(test)]
    prepared_root_surface_projection_count: usize,
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
    static FORBID_STRUCTURAL_SLOW_LANE: Cell<usize> = const { Cell::new(0) };
    static FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE: Cell<usize> = const { Cell::new(0) };
    static FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct StructuralSlowLaneGuard;

#[cfg(test)]
impl Drop for StructuralSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_STRUCTURAL_SLOW_LANE.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_structural_slow_lane_for_tests() -> StructuralSlowLaneGuard {
    FORBID_STRUCTURAL_SLOW_LANE.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    StructuralSlowLaneGuard
}

#[cfg(test)]
pub(crate) struct DirectPickRoutedExprSlowLaneGuard;

#[cfg(test)]
impl Drop for DirectPickRoutedExprSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_direct_pick_routed_expr_slow_lane_for_tests(
) -> DirectPickRoutedExprSlowLaneGuard {
    FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    DirectPickRoutedExprSlowLaneGuard
}

#[cfg(test)]
pub(crate) struct PreparedStructuralSubstitutionSlowLaneGuard;

#[cfg(test)]
impl Drop for PreparedStructuralSubstitutionSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_prepared_structural_substitution_slow_lane_for_tests(
) -> PreparedStructuralSubstitutionSlowLaneGuard {
    FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    PreparedStructuralSubstitutionSlowLaneGuard
}

// Unused after trampoline
// conversion of `project_route_surface_expr`. Helper.
#[cfg(test)]
#[allow(dead_code)]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {
    assert!(
        !direct_pick_routed_expr_slow_lane_forbidden_for_current_thread(),
        "direct routed-expr pick slow lane should not be used when member projection can satisfy the route",
    );
}

#[cfg(not(test))]
#[allow(dead_code)]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {}

#[cfg(test)]
fn assert_prepared_structural_substitution_slow_lane_allowed(expr: &TypeExpr) {
    let is_structural = matches!(
        expr,
        TypeExpr::Object(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Union(_)
            | TypeExpr::Function(_)
            | TypeExpr::Parenthesized(_),
    );
    if is_structural {
        assert!(
            !prepared_structural_substitution_slow_lane_forbidden_for_current_thread(),
            "prepared generic projection should not whole-substitute structural bodies when shallow member-local substitution can satisfy the route",
        );
    }
}

#[cfg(test)]
pub(crate) fn structural_slow_lane_forbidden_for_current_thread() -> bool {
    FORBID_STRUCTURAL_SLOW_LANE.with(|depth| depth.get() > 0)
}

#[cfg(test)]
pub(crate) fn direct_pick_routed_expr_slow_lane_forbidden_for_current_thread() -> bool {
    FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.with(|depth| depth.get() > 0)
}

#[cfg(test)]
pub(crate) fn prepared_structural_substitution_slow_lane_forbidden_for_current_thread() -> bool {
    FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.with(|depth| depth.get() > 0)
}

#[cfg(not(test))]
fn assert_prepared_structural_substitution_slow_lane_allowed(_expr: &TypeExpr) {}

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
    /// When `Some`, `project_routed_expr_surface_expr` invokes this
    /// callback exactly ONCE — at the seam between the routed-expression
    /// projection and the `cache_routed_expr_surface_expr`
    /// write-through. It deterministically reproduces a racing `upsert`
    /// of the scope file landing in that window: a torn-read producer
    /// observes the post-edit hash here and roots the pre-edit value on
    /// it; a producer that captured the observed hash before the
    /// projection roots on the pre-edit hash and refuses admission.
    static INJECT_ROUTED_EXPR_PROJECTION_SEAM_EDIT:
        RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
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

/// RAII guard that clears the routed-expr projection-seam edit
/// injection for the current thread on drop.
#[cfg(test)]
pub(crate) struct InjectRoutedExprProjectionSeamEditGuard;

#[cfg(test)]
impl Drop for InjectRoutedExprProjectionSeamEditGuard {
    fn drop(&mut self) {
        INJECT_ROUTED_EXPR_PROJECTION_SEAM_EDIT.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Arrange for `project_routed_expr_surface_expr` to run `seam_edit`
/// exactly once, at the seam between the routed-expression projection
/// and the `cache_routed_expr_surface_expr` write-through. Reproduces —
/// deterministically, single-threaded — a racing `upsert` of the scope
/// file landing between the value compute and the cache write-through.
#[cfg(test)]
pub(crate) fn inject_routed_expr_projection_seam_edit_for_tests<F>(
    seam_edit: F,
) -> InjectRoutedExprProjectionSeamEditGuard
where
    F: Fn() + 'static,
{
    INJECT_ROUTED_EXPR_PROJECTION_SEAM_EDIT
        .with(|slot| *slot.borrow_mut() = Some(Box::new(seam_edit)));
    InjectRoutedExprProjectionSeamEditGuard
}

/// Fire the routed-expr projection-seam edit hook, if installed, then
/// clear it so it runs at most once per `project_routed_expr_surface_expr`
/// call. Invoked by `routed_expr.rs` at the projection / write-through
/// seam.
#[cfg(test)]
pub(crate) fn fire_routed_expr_projection_seam_edit_for_tests() {
    let hook = INJECT_ROUTED_EXPR_PROJECTION_SEAM_EDIT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
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
            current_prepared_request_root: None,
            imported_registry_symbols: RefCell::new(FxHashMap::default()),
            declarations: RefCell::new(FxHashMap::default()),
            resolvable: RefCell::new(FxHashMap::default()),
            owner_collection_exprs: RefCell::new(FxHashMap::default()),
            scope_payloads: FxHashMap::default(),
            prepared_surface_cache: RefCell::new(FxHashMap::default()),
            prepared_member_cache: RefCell::new(FxHashMap::default()),
            prepared_target_cache: RefCell::new(FxHashMap::default()),
            routed_expr_surface_cache: RefCell::new(FxHashMap::default()),
            prepared_type_decls: FxHashMap::default(),
            materialize_memo: RefCell::new(FxHashMap::with_capacity_and_hasher(
                64,
                Default::default(),
            )),
            #[cfg(test)]
            prepared_type_decl_query_count: 0,
            #[cfg(test)]
            prepared_root_surface_projection_count: 0,
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
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::TypeAlias => {
            ResolvedDeclarationKind::TypeAlias
        }
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Interface => {
            ResolvedDeclarationKind::Interface
        }
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Class => {
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

/// Engine-internal helper that mirrors the deprecated
/// `project_type_member` entry: dispatch the single-member projection,
/// falling back to the prepared-decl walker when dispatch misses.
/// Used by `project_routed_expr_surface_expr` and friends after the
/// deprecated engine method's deletion.
fn dispatch_member_for_root_symbol(
    engine: &mut ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    symbol_name: &str,
    member_name: &str,
) -> Option<ProjectedMember> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    engine
        .dispatch_projected_member(scope_canonical_id, symbol_name, member_name)
        .or_else(|| {
            let mut active = FxHashSet::default();
            // Top-level dispatch fallback for a route's single-member
            // projection. The route was constructed at the consumer's
            // macro-T position, so the member is queried AS A BODY
            // MEMBER of the rooted symbol — `from_root_body = true`.
            // The recursive `from_expr` path narrows the flag (e.g.
            // heritage utility-type descent) and the leaf branch
            // carries it on the projected member.
            engine.project_prepared_requested_member_from_symbol(
                scope_canonical_id,
                symbol_name,
                member_name,
                &FxHashMap::default(),
                true,
                &mut active,
            )
        })
}

/// Engine-internal substitution helper that mirrors the
/// deleted `instantiate_local_generic_ref` engine method body. Unlike
/// the dispatch-only `instantiate_local_generic_ref_via_dispatch`, this
/// helper walks the re-export chain via
/// `resolve_final_prepared_type_target` before looking up the prepared
/// decl — preserving the cross-file type-alias substitution semantics
/// the engine method's call sites depended on.
fn instantiate_local_generic_ref_via_engine(
    engine: &mut ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = expr
    else {
        return None;
    };
    if type_arguments.is_empty() {
        return None;
    }

    let declaration = engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
    let declared_canonical_id = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declared_symbol_name = if declaration.resolved_name.is_empty() {
        name.as_ref().to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (target_canonical_id, target_symbol_name) = engine.resolve_final_prepared_type_target(
        declared_canonical_id.as_str(),
        declared_symbol_name.as_str(),
    );
    if is_package_source(engine.ctx, Some(target_canonical_id.as_str())) {
        return None;
    }
    let prepared = engine.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
    let substitutions = build_default_type_param_substitutions(prepared.as_ref(), type_arguments)?;
    Some(apply_type_param_substitutions(
        &prepared.body,
        &substitutions,
    ))
}

#[cfg(test)]
mod tests;
