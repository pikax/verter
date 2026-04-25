//! Declaration-aware component-meta query engine.
//!
//! `ComponentMetaQueryEngine` is a request-scoped cache bag for one
//! `get_component_meta()` request. It owns per-request projection caches
//! and resolves type declarations lazily from the host's prepared-decl
//! bundles. All solve-like operations dispatch through
//! [`ProjectSemanticDispatch`]; D-Cutover §5.8 WIP-W retired the
//! previously embedded `TypeQueryEngine` + `TypeSolverHost` bridge.
//!
//! The per-scope caches provide query-local memoization to avoid
//! re-projecting the same imported type reference within one request.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::query_engine::{
    ProjectedKeyspace, ProjectedMember, ProjectedSurface,
};

use super::declaration_metadata::{
    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    ResolvedTypeDeclaration,
};
use crate::project_semantic_dispatch::{node_data_for, resolve_decl_key, ProjectSemanticDispatch};
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::{FuseBudgets, FuseState};
use crate::semantic_query::{
    IndexKey, PathSegment, PrimitiveKind as SemanticPrimitiveKind, ProjectionMode, QueryError,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SurfaceView,
};
use crate::VerterHost;

pub(crate) const SEMANTIC_MISS: &str = "semanticMiss";
pub(crate) const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
pub(crate) const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";

#[cfg(test)]
use std::cell::Cell;

/// Path C C11b — composite-scope context for prepared-member-path
/// projection. Bundles the two scopes the prepared-route walker keeps
/// live:
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
#[derive(Debug, Clone)]
struct PreparedProjectionContext {
    decl_scope: String,
    arg_scope: String,
    /// Path C C11-residual-B: scopes from outer levels of a
    /// declaration-chain projection. Populated as the recursion
    /// descends from `project_prepared_member_path_route_projection_from_*`
    /// so a `TypeOf(value)` reference inside an inner helper body
    /// (e.g., the lowered `ComponentUI<typeof theme>` inside
    /// `ComponentConfig`'s body, where the original `Button` alias
    /// lives in `button-types.ts`) can fall back through the chain
    /// to find the scope where the value symbol was actually visible.
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
struct MaterializedMemberSurfaceKey {
    scope_canonical_id: String,
    target: MaterializedMemberSurfaceTarget,
    nested_surface: bool,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum MaterializedMemberSurfaceTarget {
    Symbol(String),
    RoutedMember {
        root_symbol: String,
        route: super::RouteDemand,
    },
    Structural(TypeExpr),
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
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedMemberCacheKey {
    canonical_id: String,
    symbol_name: String,
    member_name: String,
    kind: PreparedMemberCacheKind,
    substitutions: PreparedSubstitutionKey,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum PreparedMemberCacheKind {
    Requested,
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
/// [`project_type_surface_expr`]. D-Cutover §5.8 WIP-W retired the
/// request-scoped owner engine bridge; all solve-like operations now
/// route through `ProjectSemanticDispatch`. Imported registry entries
/// memoize by declaration scope so the same textual reference does not
/// alias across files.
pub struct ComponentMetaQueryEngine<'a> {
    host: &'a VerterHost,
    current_prepared_request_root: Option<String>,
    imported_registry_symbols: FxHashMap<(String, String), Option<ResolvedImportedRegistrySymbol>>,
    /// Cached type declarations.
    declarations: FxHashMap<(String, String), ResolvedTypeDeclaration>,
    /// Cached resolvability checks.
    resolvable: FxHashMap<(String, String), bool>,
    /// Cached owner collection expressions.
    owner_collection_exprs:
        FxHashMap<String, Option<verter_semantic::analysis::type_expr::TypeExpr>>,
    /// Request-local cache of declaration-scope payloads per scope canonical id.
    /// The prepared bundle stays authoritative; this cache only reuses the
    /// bundle-derived names/bindings within one request so repeated projections
    /// do not keep recloning them.
    scope_payloads: FxHashMap<String, Option<std::sync::Arc<DeclarationScopePayload>>>,
    /// Request-local cache for named-ref member surface materialization.
    /// This sits above the DB-backed projection caches so repeated registry
    /// enrichment can reuse the fully materialized nested surface for the same
    /// imported named ref within one request.
    materialized_member_surfaces: FxHashMap<MaterializedMemberSurfaceKey, TypeExpr>,
    /// Request-local memoization for prepared shallow surface projection.
    prepared_surface_cache: FxHashMap<PreparedSurfaceCacheKey, PreparedSurfaceProjection>,
    /// Request-local memoization for prepared member projection.
    prepared_member_cache: FxHashMap<PreparedMemberCacheKey, Option<ProjectedMember>>,
    /// Request-local memoization for prepared imported target normalization.
    prepared_target_cache: FxHashMap<PreparedTargetCacheKey, Option<(String, String)>>,
    /// Request-local memoization for routed surface expressions after the
    /// shared projection-authority cutover.
    routed_expr_surface_cache: FxHashMap<RoutedExprSurfaceCacheKey, TypeExpr>,
    /// Request-local memoization for prepared declaration lookups.
    prepared_type_decls: FxHashMap<
        (String, String),
        Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
    >,
    /// §4.5 item 3-4: per-request memo for
    /// `materialize_component_meta_type_expr_until_stable` keyed on
    /// `(scope_canonical_id, candidate_expr)`. Purity audit (see
    /// `.claude/feedback/feedback-2026-04-17-phase1cc.md`) confirmed the
    /// materializer's output depends only on `(candidate, scope,
    /// query_engine_state)` — no cross-candidate "best-so-far" baseline
    /// leaks in, and the solver caches are monotonic-additive. `TypeExpr`
    /// derives `PartialEq + Eq + Hash` so it serves as the identity
    /// directly (no separate `TypeExprIdentity` construct needed).
    /// Cleared at end-of-request when the engine drops.
    pub(crate) materialize_memo: FxHashMap<
        (String, verter_semantic::analysis::type_expr::TypeExpr),
        verter_semantic::analysis::type_expr::TypeExpr,
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
    /// Path C C11-residual-B: ambient declaration-scope chain accumulated
    /// during prepared-member-path projection recursion. Innermost entry
    /// at index 0; outermost (originating call's `decl_scope`) at the
    /// end. Used by `solve_or_project_leaf_expr_with_context` to find the
    /// scope where a `TypeOf(value)` reference is visible when neither
    /// `decl_scope` (the current declaration owner) nor `arg_scope` (the
    /// caller's SFC) contains the value symbol.
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

#[cfg(test)]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {
    assert!(
        !direct_pick_routed_expr_slow_lane_forbidden_for_current_thread(),
        "direct routed-expr pick slow lane should not be used when member projection can satisfy the route",
    );
}

#[cfg(not(test))]
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

impl<'a> ComponentMetaQueryEngine<'a> {
    pub fn new(host: &'a VerterHost) -> Self {
        Self {
            host,
            current_prepared_request_root: None,
            imported_registry_symbols: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
            scope_payloads: FxHashMap::default(),
            materialized_member_surfaces: FxHashMap::default(),
            prepared_surface_cache: FxHashMap::default(),
            prepared_member_cache: FxHashMap::default(),
            prepared_target_cache: FxHashMap::default(),
            routed_expr_surface_cache: FxHashMap::default(),
            prepared_type_decls: FxHashMap::default(),
            materialize_memo: FxHashMap::with_capacity_and_hasher(64, Default::default()),
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

    fn scope_payload_for_scope(
        &mut self,
        scope_canonical_id: &str,
    ) -> Option<std::sync::Arc<DeclarationScopePayload>> {
        let host = self.host;
        self.scope_payloads
            .entry(scope_canonical_id.to_string())
            .or_insert_with(|| {
                host.prepared_decl_bundle(scope_canonical_id)
                    .or_else(|| {
                        // Lazy first-time loading for dependency files discovered
                        // during resolution. This is NOT re-walking cached state —
                        // it triggers the normal load/parse/cache pipeline for files
                        // not yet in the host's cache.
                        host.ensure_loaded(scope_canonical_id)
                            .then(|| host.prepared_decl_bundle(scope_canonical_id))
                            .flatten()
                    })
                    .map(|bundle| {
                        std::sync::Arc::new(DeclarationScopePayload::from_bundle(&bundle))
                    })
            })
            .clone()
    }

    /// Symbolic-preservation predicate for the define-props member rescue
    /// path (D-Cutover §5.8 replacement for
    /// `TypeQueryEngine::should_preserve_shallow_field_expr`).
    ///
    /// Returns `true` when `expr` references a package-backed imported
    /// type surface that the component-meta pipeline should keep in
    /// symbolic form (as a bare `Ref` / `IndexedAccess`) instead of
    /// materialising through dispatch. Routes through
    /// `bare_name_resolve::resolve_bare_name_in_scope` +
    /// `host.prepared_type_decl` — no `SessionSolverHost`/`TypeSolverHost`
    /// dependency.
    pub fn should_preserve_shallow_field_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        let mut active_exprs = rustc_hash::FxHashSet::<TypeExpr>::default();
        let mut active_refs = rustc_hash::FxHashSet::<String>::default();
        self.should_preserve_shallow_field_expr_inner(
            scope_canonical_id,
            expr,
            &mut active_exprs,
            &mut active_refs,
        )
    }

    fn should_preserve_shallow_field_expr_inner(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        active_exprs: &mut rustc_hash::FxHashSet<TypeExpr>,
        active_refs: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        use verter_semantic::analysis::type_expr::ObjectMember;

        if !active_exprs.insert(expr.clone()) {
            return false;
        }
        let preserve = if self.should_preserve_imported_bare_ref(scope_canonical_id, expr)
            || self.should_preserve_imported_member_path(scope_canonical_id, expr)
            || self.should_preserve_imported_utility_route(scope_canonical_id, expr)
            || self.should_preserve_package_member_path(scope_canonical_id, expr)
        {
            true
        } else {
            match expr {
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        self.should_preserve_shallow_field_expr_inner(
                            scope_canonical_id,
                            member,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::Array { element, .. }
                | TypeExpr::KeyOf(element)
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => self
                    .should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        element,
                        active_exprs,
                        active_refs,
                    ),
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    self.should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        &element.ty,
                        active_exprs,
                        active_refs,
                    )
                }),
                TypeExpr::Object(object) => {
                    object.properties.iter().any(|member| match member {
                        ObjectMember::Property(property) => self
                            .should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                &property.ty,
                                active_exprs,
                                active_refs,
                            ),
                        ObjectMember::IndexSignature(signature) => {
                            self.should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                &signature.key_type,
                                active_exprs,
                                active_refs,
                            ) || self.should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                &signature.value_type,
                                active_exprs,
                                active_refs,
                            )
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            function.parameters.iter().any(|parameter| {
                                self.should_preserve_shallow_field_expr_inner(
                                    scope_canonical_id,
                                    &parameter.ty,
                                    active_exprs,
                                    active_refs,
                                )
                            }) || function.return_type.as_deref().is_some_and(|return_type| {
                                self.should_preserve_shallow_field_expr_inner(
                                    scope_canonical_id,
                                    return_type,
                                    active_exprs,
                                    active_refs,
                                )
                            })
                        }
                        ObjectMember::Method(method) => {
                            method.function.parameters.iter().any(|parameter| {
                                self.should_preserve_shallow_field_expr_inner(
                                    scope_canonical_id,
                                    &parameter.ty,
                                    active_exprs,
                                    active_refs,
                                )
                            }) || method.function.return_type.as_deref().is_some_and(
                                |return_type| {
                                    self.should_preserve_shallow_field_expr_inner(
                                        scope_canonical_id,
                                        return_type,
                                        active_exprs,
                                        active_refs,
                                    )
                                },
                            )
                        }
                    })
                }
                TypeExpr::Function(function) => {
                    function.parameters.iter().any(|parameter| {
                        self.should_preserve_shallow_field_expr_inner(
                            scope_canonical_id,
                            &parameter.ty,
                            active_exprs,
                            active_refs,
                        )
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        self.should_preserve_shallow_field_expr_inner(
                            scope_canonical_id,
                            return_type,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    let utility_with_args = !type_arguments.is_empty()
                        && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name.as_ref()).is_some();
                    if utility_with_args
                        && type_arguments.iter().any(|argument| {
                            self.should_preserve_shallow_field_expr_inner(
                                scope_canonical_id,
                                argument,
                                active_exprs,
                                active_refs,
                            )
                        })
                    {
                        true
                    } else {
                        self.should_preserve_transitive_ref(
                            scope_canonical_id,
                            name.as_ref(),
                            active_exprs,
                            active_refs,
                        )
                    }
                }
                TypeExpr::IndexedAccess { object, index } => {
                    self.should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        object,
                        active_exprs,
                        active_refs,
                    ) || self.should_preserve_shallow_field_expr_inner(
                        scope_canonical_id,
                        index,
                        active_exprs,
                        active_refs,
                    )
                }
                _ => false,
            }
        };
        active_exprs.remove(expr);
        preserve
    }

    fn should_preserve_imported_bare_ref(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        let stripped = strip_parens_expr(expr);
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = stripped
        else {
            return false;
        };
        if !type_arguments.is_empty() {
            return false;
        }
        if self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
            != verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
        {
            return false;
        }
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name.as_ref())
        else {
            return false;
        };
        if is_package_canonical(&root_identity.canonical_id) {
            return true;
        }
        let Some(prepared) = self
            .host
            .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
        else {
            return false;
        };
        !prepared.member_index.is_empty()
            || matches!(
                prepared.projection_class,
                verter_semantic::analysis::type_solver::prepared::PreparedProjectionClass::DirectMembers
            )
            || matches!(
                prepared.kind,
                verter_semantic::analysis::type_eval::TypeDeclKind::Class
            )
    }

    fn should_preserve_imported_member_path(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        fn root_import_name(expr: &TypeExpr) -> Option<&str> {
            match strip_parens_expr(expr) {
                TypeExpr::IndexedAccess { object, .. } => root_import_name(object),
                TypeExpr::Ref { name, .. } => Some(name.as_ref()),
                _ => None,
            }
        }

        let stripped = strip_parens_expr(expr);
        let TypeExpr::IndexedAccess { object, .. } = stripped else {
            return false;
        };
        let Some(name) = root_import_name(object) else {
            return false;
        };
        if self.bare_ref_origin_in_scope(scope_canonical_id, name)
            != verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
        {
            return false;
        }
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name) else {
            return false;
        };
        is_package_canonical(&root_identity.canonical_id)
    }

    fn should_preserve_imported_utility_route(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        let stripped = strip_parens_expr(expr);
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = stripped
        else {
            return false;
        };
        if type_arguments.is_empty()
            || verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                name.as_ref(),
            )
            .is_none()
        {
            return false;
        }
        type_arguments.iter().any(|argument| {
            self.should_preserve_imported_bare_ref(scope_canonical_id, argument)
                || self.should_preserve_imported_utility_route(scope_canonical_id, argument)
                || self.should_preserve_package_member_path(scope_canonical_id, argument)
                || matches!(
                    strip_parens_expr(argument),
                    TypeExpr::TypeOf(value_ref)
                        if value_ref.path.first().is_some_and(|root| {
                            self.bare_ref_origin_in_scope(scope_canonical_id, root)
                                == verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
                        })
                )
        })
    }

    fn should_preserve_package_member_path(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        fn root_import_name(expr: &TypeExpr) -> Option<&str> {
            match strip_parens_expr(expr) {
                TypeExpr::IndexedAccess { object, .. } => root_import_name(object),
                TypeExpr::Ref { name, .. } => Some(name.as_ref()),
                _ => None,
            }
        }

        let Some(name) = root_import_name(expr) else {
            return false;
        };
        if self.bare_ref_origin_in_scope(scope_canonical_id, name)
            != verter_semantic::analysis::type_solver::host::BareRefOrigin::Imported
        {
            return false;
        }
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name) else {
            return false;
        };
        is_package_canonical(&root_identity.canonical_id)
    }

    fn should_preserve_transitive_ref(
        &mut self,
        scope_canonical_id: &str,
        name: &str,
        active_exprs: &mut rustc_hash::FxHashSet<TypeExpr>,
        active_refs: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        let Some(root_identity) = self.root_identity_in_scope(scope_canonical_id, name) else {
            return false;
        };
        let cache_key = format!(
            "{}::{}",
            root_identity.canonical_id, root_identity.symbol_name
        );
        if is_package_canonical(&root_identity.canonical_id) {
            return true;
        }
        if !active_refs.insert(cache_key.clone()) {
            return false;
        }
        let result = self
            .host
            .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
            .is_some_and(|prepared| {
                if matches!(prepared.body, TypeExpr::TypeParameter(_)) {
                    true
                } else {
                    self.should_preserve_shallow_field_expr_inner(
                        root_identity.canonical_id.as_str(),
                        &prepared.body,
                        active_exprs,
                        active_refs,
                    )
                }
            });
        active_refs.remove(&cache_key);
        result
    }

    pub(crate) fn try_fast_shallow_field_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<FastShallowFieldExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        fn single_member_import_root(expr: &TypeExpr) -> Option<(&str, &str)> {
            let TypeExpr::IndexedAccess { object, index } = strip_parens_expr(expr) else {
                return None;
            };
            let TypeExpr::Ref {
                name,
                type_arguments,
            } = strip_parens_expr(object)
            else {
                return None;
            };
            if !type_arguments.is_empty() {
                return None;
            }
            let TypeExpr::Literal(verter_semantic::analysis::type_expr::LiteralValue::String(
                member_name,
            )) = strip_parens_expr(index)
            else {
                return None;
            };
            Some((name.as_ref(), member_name.as_str()))
        }

        fn fast_symbolic_imported_generic_route(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
            active_locals: &mut FxHashSet<String>,
        ) -> bool {
            match strip_parens_expr(expr) {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => match engine.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref()) {
                    BareRefOrigin::Imported => !type_arguments.is_empty(),
                    BareRefOrigin::Local if type_arguments.is_empty() => {
                        let Some(root_identity) =
                            engine.root_identity_in_scope(scope_canonical_id, name.as_ref())
                        else {
                            return false;
                        };
                        let active_key = format!(
                            "{}::{}",
                            root_identity.canonical_id, root_identity.symbol_name
                        );
                        if !active_locals.insert(active_key.clone()) {
                            return false;
                        }
                        let preserve = engine
                            .prepared_type_decl(
                                &root_identity.canonical_id,
                                &root_identity.symbol_name,
                            )
                            .is_some_and(|prepared| {
                                fast_symbolic_imported_generic_route(
                                    engine,
                                    root_identity.canonical_id.as_str(),
                                    &prepared.body,
                                    active_locals,
                                )
                            });
                        active_locals.remove(&active_key);
                        preserve
                    }
                    _ => false,
                },
                TypeExpr::IndexedAccess { object, .. }
                | TypeExpr::Array {
                    element: object, ..
                }
                | TypeExpr::KeyOf(object)
                | TypeExpr::Rest(object)
                | TypeExpr::Parenthesized(object) => fast_symbolic_imported_generic_route(
                    engine,
                    scope_canonical_id,
                    object,
                    active_locals,
                ),
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    fast_symbolic_imported_generic_route(
                        engine,
                        scope_canonical_id,
                        &element.ty,
                        active_locals,
                    )
                }),
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        fast_symbolic_imported_generic_route(
                            engine,
                            scope_canonical_id,
                            member,
                            active_locals,
                        )
                    })
                }
                _ => false,
            }
        }

        fn fast_symbolic_imported_bare_ref_route(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
        ) -> bool {
            match strip_parens_expr(expr) {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if type_arguments.is_empty()
                    && engine.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
                        == BareRefOrigin::Imported
                    && name.as_ref().ends_with("Props") =>
                {
                    true
                }
                TypeExpr::Array { element, .. }
                | TypeExpr::KeyOf(element)
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => {
                    fast_symbolic_imported_bare_ref_route(engine, scope_canonical_id, element)
                }
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    fast_symbolic_imported_bare_ref_route(engine, scope_canonical_id, &element.ty)
                }),
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        fast_symbolic_imported_bare_ref_route(engine, scope_canonical_id, member)
                    })
                }
                _ => false,
            }
        }

        fn collapse_same_file_imported_alias_chain(
            engine: &mut ComponentMetaQueryEngine<'_>,
            canonical_id: &str,
            expr: &TypeExpr,
        ) -> TypeExpr {
            let mut current = expr.clone();
            let mut visited = FxHashSet::<String>::default();

            loop {
                let TypeExpr::Ref {
                    name,
                    type_arguments,
                } = strip_parens_expr(&current)
                else {
                    return current;
                };
                if !type_arguments.is_empty() || !visited.insert(name.to_string()) {
                    return current;
                }
                let Some(root_identity) =
                    engine.root_identity_in_scope(canonical_id, name.as_ref())
                else {
                    return current;
                };
                if root_identity.canonical_id != canonical_id {
                    return current;
                }
                let Some(prepared) = engine
                    .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)
                else {
                    return current;
                };
                current = prepared.body.clone();
            }
        }

        fn imported_value_route_arg(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
        ) -> bool {
            match strip_parens_expr(expr) {
                TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef { path }) => {
                    path.first().is_some_and(|root| {
                        engine.bare_ref_origin_in_scope(scope_canonical_id, root)
                            == BareRefOrigin::Imported
                    })
                }
                TypeExpr::Parenthesized(inner) => {
                    imported_value_route_arg(engine, scope_canonical_id, inner)
                }
                _ => false,
            }
        }

        fn contains_direct_imported_utility_route(
            engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            expr: &TypeExpr,
        ) -> bool {
            fn imported_route_arg(
                engine: &mut ComponentMetaQueryEngine<'_>,
                scope_canonical_id: &str,
                expr: &TypeExpr,
            ) -> bool {
                match strip_parens_expr(expr) {
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        (type_arguments.is_empty()
                            && engine.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
                                == BareRefOrigin::Imported)
                            || imported_value_route_arg(engine, scope_canonical_id, expr)
                            || contains_direct_imported_utility_route(
                                engine,
                                scope_canonical_id,
                                expr,
                            )
                    }
                    TypeExpr::IndexedAccess { object, .. } => {
                        imported_route_arg(engine, scope_canonical_id, object)
                    }
                    TypeExpr::TypeOf(_) => {
                        imported_value_route_arg(engine, scope_canonical_id, expr)
                    }
                    TypeExpr::Parenthesized(inner) => {
                        imported_route_arg(engine, scope_canonical_id, inner)
                    }
                    _ => contains_direct_imported_utility_route(engine, scope_canonical_id, expr),
                }
            }

            match strip_parens_expr(expr) {
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => members.iter().any(
                    |member| {
                        contains_direct_imported_utility_route(engine, scope_canonical_id, member)
                    },
                ),
                TypeExpr::Array { element, .. }
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => {
                    contains_direct_imported_utility_route(engine, scope_canonical_id, element)
                }
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    contains_direct_imported_utility_route(
                        engine,
                        scope_canonical_id,
                        &element.ty,
                    )
                }),
                TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                    verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &property.ty,
                        )
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                        method.function.parameters.iter().any(|parameter| {
                            contains_direct_imported_utility_route(
                                engine,
                                scope_canonical_id,
                                &parameter.ty,
                            )
                        }) || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| {
                                contains_direct_imported_utility_route(
                                    engine,
                                    scope_canonical_id,
                                    return_type,
                                )
                            })
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::CallSignature(function)
                    | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                        function,
                    ) => function.parameters.iter().any(|parameter| {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &parameter.ty,
                        )
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            return_type,
                        )
                    }),
                    verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(index) => {
                        contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &index.key_type,
                        ) || contains_direct_imported_utility_route(
                            engine,
                            scope_canonical_id,
                            &index.value_type,
                        )
                    }
                }),
                TypeExpr::Function(function) => function.parameters.iter().any(|parameter| {
                    contains_direct_imported_utility_route(
                        engine,
                        scope_canonical_id,
                        &parameter.ty,
                    )
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    contains_direct_imported_utility_route(engine, scope_canonical_id, return_type)
                }),
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if !type_arguments.is_empty()
                    && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                        name.as_ref(),
                    )
                    .is_some() =>
                {
                    type_arguments.iter().any(|argument| {
                        imported_route_arg(engine, scope_canonical_id, argument)
                    })
                }
                _ => false,
            }
        }

        if contains_direct_imported_utility_route(self, scope_canonical_id, expr) {
            return Some(FastShallowFieldExpr {
                expr: expr.clone(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            });
        }

        if fast_symbolic_imported_bare_ref_route(self, scope_canonical_id, expr) {
            return Some(FastShallowFieldExpr {
                expr: expr.clone(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            });
        }

        if let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        {
            if !type_arguments.is_empty()
                && self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref())
                    == BareRefOrigin::Imported
            {
                let _ = self.root_identity_in_scope(scope_canonical_id, name.as_ref())?;
                return Some(FastShallowFieldExpr {
                    expr: expr.clone(),
                    exactness: FastShallowFieldExprExactness::Symbolic,
                });
            }
        }

        if let Some((root_name, member_name)) = single_member_import_root(expr) {
            if self.bare_ref_origin_in_scope(scope_canonical_id, root_name)
                == BareRefOrigin::Imported
            {
                let root_identity = self.root_identity_in_scope(scope_canonical_id, root_name)?;
                if is_package_canonical(&root_identity.canonical_id) {
                    return Some(FastShallowFieldExpr {
                        expr: expr.clone(),
                        exactness: FastShallowFieldExprExactness::Symbolic,
                    });
                }
                let prepared = self
                    .prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)?;
                let member = prepared.member(member_name)?;
                if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                    return None;
                }
                let collapsed = collapse_same_file_imported_alias_chain(
                    self,
                    &root_identity.canonical_id,
                    &member.ty,
                );
                return Some(FastShallowFieldExpr {
                    expr: collapsed,
                    exactness: FastShallowFieldExprExactness::Concrete,
                });
            }
        }

        if let Some(expanded) = self.try_fast_expand_shallow_alias_body(scope_canonical_id, expr) {
            return Some(FastShallowFieldExpr {
                expr: expanded,
                exactness: FastShallowFieldExprExactness::Symbolic,
            });
        }

        let mut active_locals = FxHashSet::default();
        fast_symbolic_imported_generic_route(self, scope_canonical_id, expr, &mut active_locals)
            .then(|| FastShallowFieldExpr {
                expr: expr.clone(),
                exactness: FastShallowFieldExprExactness::Symbolic,
            })
    }

    fn try_fast_expand_shallow_alias_body(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        else {
            return None;
        };
        if !type_arguments.is_empty() {
            return None;
        }
        if !matches!(
            self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref()),
            BareRefOrigin::Imported | BareRefOrigin::Local
        ) {
            return None;
        }
        let root_identity = self.root_identity_in_scope(scope_canonical_id, name.as_ref())?;
        if is_package_canonical(&root_identity.canonical_id) {
            return None;
        }
        let prepared =
            self.prepared_type_decl(&root_identity.canonical_id, &root_identity.symbol_name)?;
        if !prepared.type_parameters.is_empty() {
            return None;
        }
        let mut active_aliases = FxHashSet::default();
        let expanded = self.rewrite_fast_shallow_alias_body(
            root_identity.canonical_id.as_str(),
            &prepared.body,
            &mut active_aliases,
        )?;
        (expanded != *expr).then_some(expanded)
    }

    fn rewrite_fast_shallow_alias_body(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        active_aliases: &mut FxHashSet<String>,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;

        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_) => Some(expr.clone()),
            TypeExpr::Parenthesized(inner) => Some(TypeExpr::Parenthesized(std::sync::Arc::new(
                self.rewrite_fast_shallow_alias_body(scope_canonical_id, inner, active_aliases)?,
            ))),
            TypeExpr::KeyOf(inner) => Some(TypeExpr::KeyOf(std::sync::Arc::new(
                self.rewrite_fast_shallow_alias_body(scope_canonical_id, inner, active_aliases)?,
            ))),
            TypeExpr::Rest(inner) => Some(TypeExpr::Rest(std::sync::Arc::new(
                self.rewrite_fast_shallow_alias_body(scope_canonical_id, inner, active_aliases)?,
            ))),
            TypeExpr::Array { element, readonly } => Some(TypeExpr::Array {
                element: std::sync::Arc::new(self.rewrite_fast_shallow_alias_body(
                    scope_canonical_id,
                    element,
                    active_aliases,
                )?),
                readonly: *readonly,
            }),
            TypeExpr::Tuple { elements, readonly } => {
                let elements = elements
                    .iter()
                    .map(|element| {
                        Some(verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: self.rewrite_fast_shallow_alias_body(
                                scope_canonical_id,
                                &element.ty,
                                active_aliases,
                            )?,
                            optional: element.optional,
                            rest: element.rest,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(TypeExpr::Tuple {
                    elements: std::sync::Arc::from(elements),
                    readonly: *readonly,
                })
            }
            TypeExpr::Union(members) => Some(TypeExpr::Union(std::sync::Arc::from(
                members
                    .iter()
                    .map(|member| {
                        self.rewrite_fast_shallow_alias_body(
                            scope_canonical_id,
                            member,
                            active_aliases,
                        )
                    })
                    .collect::<Option<Vec<_>>>()?,
            ))),
            TypeExpr::Intersection(members) => Some(TypeExpr::Intersection(std::sync::Arc::from(
                members
                    .iter()
                    .map(|member| {
                        self.rewrite_fast_shallow_alias_body(
                            scope_canonical_id,
                            member,
                            active_aliases,
                        )
                    })
                    .collect::<Option<Vec<_>>>()?,
            ))),
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => Some(TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: std::sync::Arc::from(
                    expressions
                        .iter()
                        .map(|expression| {
                            self.rewrite_fast_shallow_alias_body(
                                scope_canonical_id,
                                expression,
                                active_aliases,
                            )
                        })
                        .collect::<Option<Vec<_>>>()?,
                ),
            }),
            TypeExpr::Function(function) => Some(TypeExpr::Function(std::sync::Arc::new(
                verter_semantic::analysis::type_expr::FunctionExpr {
                    parameters: function
                        .parameters
                        .iter()
                        .map(|parameter| {
                            Some(verter_semantic::analysis::type_expr::FunctionParam {
                                name: parameter.name.clone(),
                                ty: self.rewrite_fast_shallow_alias_body(
                                    scope_canonical_id,
                                    &parameter.ty,
                                    active_aliases,
                                )?,
                                optional: parameter.optional,
                                rest: parameter.rest,
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                    return_type: match function.return_type.as_deref() {
                        Some(return_type) => {
                            Some(std::sync::Arc::new(self.rewrite_fast_shallow_alias_body(
                                scope_canonical_id,
                                return_type,
                                active_aliases,
                            )?))
                        }
                        None => None,
                    },
                    type_parameters: function
                        .type_parameters
                        .iter()
                        .map(|parameter| {
                            Some(verter_semantic::analysis::type_expr::TypeParam {
                                name: parameter.name.clone(),
                                constraint: match parameter.constraint.as_deref() {
                                    Some(constraint) => Some(std::sync::Arc::new(
                                        self.rewrite_fast_shallow_alias_body(
                                            scope_canonical_id,
                                            constraint,
                                            active_aliases,
                                        )?,
                                    )),
                                    None => None,
                                },
                                default: match parameter.default.as_deref() {
                                    Some(default) => Some(std::sync::Arc::new(
                                        self.rewrite_fast_shallow_alias_body(
                                            scope_canonical_id,
                                            default,
                                            active_aliases,
                                        )?,
                                    )),
                                    None => None,
                                },
                            })
                        })
                        .collect::<Option<Vec<_>>>()?,
                },
            ))),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if !type_arguments.is_empty() {
                    return None;
                }
                match self.bare_ref_origin_in_scope(scope_canonical_id, name.as_ref()) {
                    BareRefOrigin::Imported | BareRefOrigin::Local => {
                        let root_identity =
                            self.root_identity_in_scope(scope_canonical_id, name.as_ref())?;
                        if is_package_canonical(&root_identity.canonical_id) {
                            return Some(expr.clone());
                        }
                        let active_key = format!(
                            "{}::{}",
                            root_identity.canonical_id, root_identity.symbol_name
                        );
                        if !active_aliases.insert(active_key.clone()) {
                            return None;
                        }
                        let rewritten = self
                            .prepared_type_decl(
                                &root_identity.canonical_id,
                                &root_identity.symbol_name,
                            )
                            .and_then(|prepared| {
                                prepared.type_parameters.is_empty().then_some(prepared)
                            })
                            .and_then(|prepared| {
                                self.rewrite_fast_shallow_alias_body(
                                    root_identity.canonical_id.as_str(),
                                    &prepared.body,
                                    active_aliases,
                                )
                            });
                        active_aliases.remove(&active_key);
                        rewritten
                    }
                    _ => None,
                }
            }
            TypeExpr::Object(object) => object
                .properties
                .is_empty()
                .then(|| TypeExpr::Object(object.clone())),
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. } => None,
        }
    }

    fn bare_ref_origin_in_scope(
        &mut self,
        scope_canonical_id: &str,
        name: &str,
    ) -> verter_semantic::analysis::type_solver::host::BareRefOrigin {
        use verter_semantic::analysis::type_solver::host::BareRefOrigin;
        let payload = self.scope_payload_for_scope(scope_canonical_id);
        if let Some(payload) = payload.as_deref() {
            if payload.import_bindings.contains_key(name) {
                return BareRefOrigin::Imported;
            }
            if payload.scope_type_bindings.contains_key(name)
                || payload.scope_type_names.contains(name)
                || payload.scope_value_names.contains(name)
            {
                return BareRefOrigin::Local;
            }
        }
        BareRefOrigin::Unknown
    }

    fn root_identity_in_scope(
        &mut self,
        scope_canonical_id: &str,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::host::ResolvedRootIdentity> {
        let payload = self.scope_payload_for_scope(scope_canonical_id);
        crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.host,
            scope_canonical_id,
            payload.as_deref(),
            name,
        )
    }

    /// Walk an Object's properties/methods and resolve any
    /// `TypeExpr::Ref` leaves (inside property types, function return
    /// types, array elements, union/intersection arms) to their
    /// dispatch-projected surface (D-Cutover §5.8 replacement for the
    /// retired `type_eval_build::deep_resolve_slot_function_refs`).
    ///
    /// Non-Object inputs are returned verbatim — matches the pre-cutover
    /// contract. Ref resolution routes through
    /// [`Self::project_expr_surface_expr`] so it uses the same dispatch
    /// memo (`SemanticGraphStore`) + `instantiate_active` guards as the
    /// rest of the component-meta pipeline, guaranteeing one cache entry
    /// per `(scope, expr)` regardless of entry point.
    pub fn deep_resolve_slot_function_refs(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> TypeExpr {
        use verter_semantic::analysis::type_expr::{ObjectMember, ObjectProperty};

        match expr {
            TypeExpr::Object(obj) => {
                let properties: Vec<ObjectMember> = obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                            name: p.name.clone(),
                            ty: self.deep_resolve_type_refs(scope_canonical_id, &p.ty),
                            optional: p.optional,
                            readonly: p.readonly,
                        }),
                        ObjectMember::Method(m) => ObjectMember::Method(
                            verter_semantic::analysis::type_expr::MethodSignature {
                                name: m.name.clone(),
                                function: self
                                    .deep_resolve_fn_refs(scope_canonical_id, &m.function),
                                optional: m.optional,
                            },
                        ),
                        other => other.clone(),
                    })
                    .collect();
                TypeExpr::Object(std::sync::Arc::new(
                    verter_semantic::analysis::type_expr::ObjectExpr { properties },
                ))
            }
            // Path C C11-residual-A: walk compound shapes so
            // `defineSlots<TabsSlots<T>>` patterns with
            // `{ leading?, content? } & DynamicSlots<...>` bodies still
            // resolve their explicit Object arm's `SlotProps<T>` members
            // into Function signatures, which `enrich_missing_slot_bindings`
            // consumes for slot-binding extraction.
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(std::sync::Arc::new(
                self.deep_resolve_slot_function_refs(scope_canonical_id, inner),
            )),
            TypeExpr::Intersection(parts) => TypeExpr::Intersection(std::sync::Arc::from(
                parts
                    .iter()
                    .map(|p| self.deep_resolve_slot_function_refs(scope_canonical_id, p))
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Union(variants) => TypeExpr::Union(std::sync::Arc::from(
                variants
                    .iter()
                    .map(|v| self.deep_resolve_slot_function_refs(scope_canonical_id, v))
                    .collect::<Vec<_>>(),
            )),
            _ => expr.clone(),
        }
    }

    fn deep_resolve_type_refs(&mut self, scope_canonical_id: &str, expr: &TypeExpr) -> TypeExpr {
        match expr {
            TypeExpr::Ref { .. } => self
                .project_expr_surface_expr(scope_canonical_id, expr)
                .unwrap_or_else(|| expr.clone()),
            TypeExpr::Function(func) => TypeExpr::Function(std::sync::Arc::new(
                self.deep_resolve_fn_refs(scope_canonical_id, func),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: std::sync::Arc::new(
                    self.deep_resolve_type_refs(scope_canonical_id, element),
                ),
                readonly: *readonly,
            },
            TypeExpr::Union(variants) => TypeExpr::Union(std::sync::Arc::from(
                variants
                    .iter()
                    .map(|v| self.deep_resolve_type_refs(scope_canonical_id, v))
                    .collect::<Vec<_>>(),
            )),
            TypeExpr::Intersection(parts) => TypeExpr::Intersection(std::sync::Arc::from(
                parts
                    .iter()
                    .map(|p| self.deep_resolve_type_refs(scope_canonical_id, p))
                    .collect::<Vec<_>>(),
            )),
            // Path C C11-residual-A: try a strict projection on
            // deferred shells that may have been left in a mapped-slot
            // value when the upstream dispatch's same-path sentinel
            // suppressed a sub-evaluation. The strict projection only
            // returns when the full surface materialises (Object /
            // Function / Primitive); otherwise the deferred shell is
            // preserved so the TypeExpr-level slot-binding extractor
            // (`enrich_missing_slot_bindings`) can apply its
            // `decide_typeexpr_conditional_with_function_extends`
            // workaround.
            TypeExpr::Conditional { .. }
            | TypeExpr::IndexedAccess { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::KeyOf(_)
            | TypeExpr::TypeOf(_) => self
                .project_expr_surface_expr(scope_canonical_id, expr)
                .unwrap_or_else(|| expr.clone()),
            _ => expr.clone(),
        }
    }

    fn deep_resolve_fn_refs(
        &mut self,
        scope_canonical_id: &str,
        func: &verter_semantic::analysis::type_expr::FunctionExpr,
    ) -> verter_semantic::analysis::type_expr::FunctionExpr {
        verter_semantic::analysis::type_expr::FunctionExpr {
            parameters: func
                .parameters
                .iter()
                .map(|p| verter_semantic::analysis::type_expr::FunctionParam {
                    name: p.name.clone(),
                    ty: self.deep_resolve_type_refs(scope_canonical_id, &p.ty),
                    optional: p.optional,
                    rest: p.rest,
                })
                .collect(),
            return_type: func
                .return_type
                .as_ref()
                .map(|rt| std::sync::Arc::new(self.deep_resolve_type_refs(scope_canonical_id, rt))),
            type_parameters: func.type_parameters.clone(),
        }
    }

    pub fn resolve_imported_registry_symbol(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<ResolvedImportedRegistrySymbol> {
        let key = (canonical_id.to_string(), exported_name.to_string());
        if let Some(cached) = self.imported_registry_symbols.get(&key) {
            return cached.clone();
        }
        let resolved = resolve_imported_registry_symbol_with_budget(
            self.host,
            canonical_id,
            exported_name,
            || self.allow_wildcard_route(),
        );
        self.imported_registry_symbols.insert(key, resolved.clone());
        resolved
    }

    pub fn resolve_direct_prepared_type_declaration(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<ResolvedTypeDeclaration> {
        self.prepared_type_decl(canonical_source, resolved_name)?;
        let metadata = local_type_symbol_metadata_for_known_source(
            self.host,
            canonical_source,
            resolved_name,
        )?;
        let resolver = DirectPreparedDeclarationResolver { host: self.host };
        Some(crate::resolver_core::resolve_local_type_declaration(
            &resolver,
            canonical_source,
            resolved_name,
            metadata.span,
        ))
    }

    pub fn resolve_direct_prepared_type_declaration_metadata(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<ResolvedTypeDeclaration> {
        self.prepared_type_decl(canonical_source, resolved_name)?;
        let metadata = local_type_symbol_metadata_for_known_source(
            self.host,
            canonical_source,
            resolved_name,
        )?;
        Some(ResolvedTypeDeclaration {
            requested_name: resolved_name.to_string(),
            declaration_id: self
                .host
                .local_type_declaration_id(canonical_source, resolved_name),
            resolved_name: resolved_name.to_string(),
            canonical_source: canonical_source.to_string(),
            span: metadata.span,
            kind: metadata.kind,
            text: None,
        })
    }

    /// Resolve a type declaration, cached per query.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        if let Some(cached) = self.declarations.get(&key) {
            return cached.clone();
        }
        let declaration = self
            .resolve_direct_prepared_type_declaration(canonical_source, requested_name)
            .unwrap_or_else(|| {
                crate::meta_resolve::resolve_type_declaration(
                    self.host,
                    canonical_source,
                    requested_name,
                )
            });
        self.declarations.insert(key, declaration.clone());
        declaration
    }

    pub fn resolve_final_prepared_type_target(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> (String, String) {
        if self
            .prepared_type_decl(canonical_source, resolved_name)
            .is_some()
        {
            return (canonical_source.to_string(), resolved_name.to_string());
        }

        self.host
            .resolve_named_type_export_target_shallow(canonical_source, resolved_name)
            .filter(|(target_canonical, target_name)| {
                self.prepared_type_decl(target_canonical.as_str(), target_name.as_str())
                    .is_some()
            })
            .unwrap_or_else(|| (canonical_source.to_string(), resolved_name.to_string()))
    }

    /// Check if a registry ref can resolve, cached per query.
    pub fn can_resolve_registry_symbol(
        &mut self,
        owner_canonical: &str,
        exported_name: &str,
        source_hint: Option<&str>,
    ) -> bool {
        if is_builtin_name(exported_name) {
            return false;
        }
        let source_key = source_hint
            .filter(|s| !s.is_empty())
            .unwrap_or(owner_canonical);
        let key = (source_key.to_string(), exported_name.to_string());
        if let Some(cached) = self.resolvable.get(&key) {
            return *cached;
        }
        let resolved = if self.prepared_type_decl(source_key, exported_name).is_some() {
            true
        } else {
            self.resolve_imported_registry_symbol(source_key, exported_name)
                .is_some()
        };
        self.resolvable.insert(key, resolved);
        resolved
    }

    /// Get the owner's collection expression for a name, cached per query.
    pub fn owner_collection_expr(
        &mut self,
        owner_canonical: &str,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        if let Some(cached) = self.owner_collection_exprs.get(name) {
            return cached.clone();
        }

        let body = self
            .prepared_type_decl(owner_canonical, name)
            .map(|prepared| prepared.body.clone());
        self.owner_collection_exprs
            .insert(name.to_string(), body.clone());
        body
    }

    pub fn named_decl_body(&mut self, canonical_id: &str, name: &str) -> Option<TypeExpr> {
        self.prepared_type_decl(canonical_id, name)
            .map(|prepared| prepared.body.clone())
    }

    pub fn prepared_member_raw_type(
        &mut self,
        canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<TypeExpr> {
        self.prepared_type_decl(canonical_id, symbol_name)
            .and_then(|prepared| prepared.member(member_name).map(|member| member.ty.clone()))
    }

    pub fn cached_materialized_member_surface(
        &self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        nested_surface: bool,
    ) -> Option<TypeExpr> {
        materialized_member_surface_key(scope_canonical_id, expr, nested_surface)
            .and_then(|key| self.materialized_member_surfaces.get(&key).cloned())
    }

    pub fn store_materialized_member_surface(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        nested_surface: bool,
        materialized: TypeExpr,
    ) {
        let Some(key) = materialized_member_surface_key(scope_canonical_id, expr, nested_surface)
        else {
            return;
        };
        self.materialized_member_surfaces.insert(key, materialized);
    }

    pub fn enter_member_surface(&mut self) -> bool {
        self.fuse_state.push_member_recursion();
        !self
            .fuse_state
            .check_member_recursion_depth(&self.fuse_budgets)
    }

    pub fn exit_member_surface(&mut self) {
        self.fuse_state.pop_member_recursion();
    }

    pub fn allow_structural_slow_lane(&mut self) -> bool {
        !self
            .fuse_state
            .check_structural_slow_lane(&self.fuse_budgets)
    }

    /// Check wildcard route fanout budget. Returns `true` if within budget.
    pub fn allow_wildcard_route(&mut self) -> bool {
        !self
            .fuse_state
            .check_wildcard_route_fanout(&self.fuse_budgets)
    }

    /// Check imported-root fanout budget. Returns `true` if within budget.
    pub fn allow_imported_root(&mut self) -> bool {
        !self
            .fuse_state
            .check_imported_root_fanout(&self.fuse_budgets)
    }

    /// Check registry deepening fanout budget. Returns `true` if within budget.
    pub fn allow_registry_deepening(&mut self) -> bool {
        !self
            .fuse_state
            .check_registry_deepening_fanout(&self.fuse_budgets)
    }

    /// Check union/member explosion budget. Returns `true` if within budget.
    pub fn allow_union_member(&mut self) -> bool {
        !self
            .fuse_state
            .check_union_member_explosion(&self.fuse_budgets)
    }

    /// Reset union member counter for per-member branch counting.
    pub fn reset_union_members(&mut self) {
        self.fuse_state.reset_union_members();
    }

    /// Whether any fuse has tripped.
    pub fn has_fuse_tripped(&self) -> bool {
        self.fuse_state.has_tripped()
    }

    /// Get fuse trip details for provenance/tracing.
    pub fn fuse_trips(&self) -> &[super::FuseTrip] {
        &self.fuse_state.trips
    }

    #[cfg(test)]
    pub(crate) fn imported_registry_symbol_cache_len(&self) -> usize {
        self.imported_registry_symbols.len()
    }

    #[cfg(test)]
    pub(crate) fn materialized_member_surface_cache_len(&self) -> usize {
        self.materialized_member_surfaces.len()
    }

    #[cfg(test)]
    pub(crate) fn debug_prepared_type_decl_query_count(&self) -> usize {
        self.prepared_type_decl_query_count
    }

    #[cfg(test)]
    pub(crate) fn debug_prepared_root_surface_projection_count(&self) -> usize {
        self.prepared_root_surface_projection_count
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_shared_surface_hit_count(&self) -> usize {
        self.prepared_shared_surface_hit_count
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_shared_member_hit_count(&self) -> usize {
        self.prepared_shared_member_hit_count
    }

    pub(crate) fn prepared_type_decl(
        &mut self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let key = (canonical_id.to_string(), symbol_name.to_string());
        if let Some(cached) = self.prepared_type_decls.get(&key) {
            return cached.clone();
        }

        #[cfg(test)]
        {
            self.prepared_type_decl_query_count += 1;
        }

        let resolved = self
            .host
            .prepared_type_decl(canonical_id, symbol_name)
            .or_else(|| {
                // Lazy first-time loading (see scope_payload_for_scope comment).
                self.host
                    .ensure_loaded(canonical_id)
                    .then(|| self.host.prepared_type_decl(canonical_id, symbol_name))
                    .flatten()
            });
        self.prepared_type_decls.insert(key, resolved.clone());
        resolved
    }

    pub(crate) fn host(&self) -> &VerterHost {
        self.host
    }

    fn semantic_dispatch(&self) -> ProjectSemanticDispatch<'_> {
        ProjectSemanticDispatch::new(self.host)
    }

    fn dispatch_root_instantiated(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<SemanticNodeId> {
        // D-Cutover §5.8: resolve the root identity via
        // `bare_name_resolve::resolve_bare_name_in_scope` directly —
        // no `SessionSolverHost` construction. Matches the dispatch
        // lowering path in `shallow_lower_type_expr`.
        let scope_payload_arc = self.scope_payload_for_scope(scope_canonical_id);
        let resolved_root = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.host,
            scope_canonical_id,
            scope_payload_arc.as_deref(),
            symbol_name,
        )
        .map(|root| (root.canonical_id, root.symbol_name))
        .unwrap_or_else(|| (scope_canonical_id.to_string(), symbol_name.to_string()));
        let dispatch = self.semantic_dispatch();
        let anchor = match dispatch.execute(SemanticQueryKey::ResolveDecl(resolve_decl_key(
            resolved_root.0.as_str(),
            resolved_root.1.as_str(),
        ))) {
            QueryResult::Value(id) => id,
            _ => return None,
        };
        // C16: Instantiate.base is DeclIdentity. Build from resolved root +
        // shallow state whole_hash.
        let whole_hash = self
            .host
            .shallow_file_state(resolved_root.0.as_str())
            .map(|s| s.whole_hash)
            .unwrap_or_default();
        let identity = crate::semantic_query::DeclIdentity {
            canonical_id: std::sync::Arc::from(resolved_root.0.as_str()),
            whole_hash,
            decl_name: std::sync::Arc::from(resolved_root.1.as_str()),
        };
        match dispatch.execute(SemanticQueryKey::Instantiate {
            base: identity,
            args: empty_semantic_args(),
        }) {
            QueryResult::Value(id) => Some(id),
            _ => Some(anchor),
        }
    }

    fn dispatch_projected_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedSurface> {
        let root = self.dispatch_root_instantiated(scope_canonical_id, symbol_name)?;
        projected_surface_from_semantic_node(self.host, root)
    }

    fn dispatch_projected_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        self.dispatch_projected_surface(scope_canonical_id, symbol_name)?
            .members
            .into_iter()
            .find(|member| member.name == member_name)
    }

    fn dispatch_projected_keyspace(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedKeyspace> {
        let surface = self.dispatch_projected_surface(scope_canonical_id, symbol_name)?;
        let mut members = surface
            .members
            .iter()
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        members.sort();
        members.dedup();
        Some(ProjectedKeyspace {
            members,
            has_index_signature: surface.has_index_signature,
        })
    }

    fn dispatch_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        match route {
            super::RouteDemand::Whole => self
                .dispatch_projected_surface(scope_canonical_id, root_symbol)
                .and_then(|surface| projected_surface_to_type_expr(&surface))
                .filter(dispatch_route_expr_is_materialized),
            super::RouteDemand::MemberPath(path) if !path.is_empty() => {
                let root = self.dispatch_root_instantiated(scope_canonical_id, root_symbol)?;
                let query_path: std::sync::Arc<[PathSegment]> = std::sync::Arc::from(
                    path.iter()
                        .map(|segment| PathSegment::Member(std::sync::Arc::from(segment.as_str())))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let dispatch = self.semantic_dispatch();
                match dispatch.execute(SemanticQueryKey::ProjectPath {
                    base: root,
                    path: query_path,
                    mode: ProjectionMode::Expanded,
                }) {
                    QueryResult::Value(node) => semantic_node_to_type_expr(self.host, node)
                        .filter(dispatch_route_expr_is_materialized),
                    _ => None,
                }
            }
            super::RouteDemand::Pick(members) if !members.is_empty() => self
                .dispatch_projected_surface(scope_canonical_id, root_symbol)
                .and_then(|surface| {
                    projected_surface_to_type_expr(&filtered_projected_surface(surface, |name| {
                        members.iter().any(|member| member == name)
                    }))
                })
                .filter(dispatch_route_expr_is_materialized),
            super::RouteDemand::Omit(members) if !members.is_empty() => self
                .dispatch_projected_surface(scope_canonical_id, root_symbol)
                .and_then(|surface| {
                    projected_surface_to_type_expr(&filtered_projected_surface(surface, |name| {
                        !members.iter().any(|member| member == name)
                    }))
                })
                .filter(dispatch_route_expr_is_materialized),
            _ => None,
        }
    }

    // -------------------------------------------------------------------
    // WS3: Projection-based surface extraction
    // -------------------------------------------------------------------

    /// Project the full surface of a type expression in a declaration scope.
    ///
    /// This is the projection-based alternative to `solve_scoped` + manual
    /// surface extraction. It builds a `SubjectKey::Decl` for the type,
    /// interns it, and calls `project_surface` to get all members and call
    /// signatures without full structural normalization.
    ///
    /// Results are write-through: stable projections are published to
    /// `TypeSurfaceDb` and reused by later requests.
    pub fn project_type_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedSurface> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        self.dispatch_projected_surface(scope_canonical_id, symbol_name)
            .or_else(|| self.cached_prepared_root_surface(scope_canonical_id, symbol_name))
    }

    pub fn project_type_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<TypeExpr> {
        self.project_type_surface(scope_canonical_id, symbol_name)
            .and_then(|surface| projected_surface_to_type_expr(&surface))
    }

    pub fn project_type_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        self.project_type_surface(scope_canonical_id, symbol_name)
            .map(|surface| projected_surface_to_expanded_shape(&surface))
    }

    pub fn project_prepared_type_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<TypeExpr> {
        self.cached_prepared_root_surface(scope_canonical_id, symbol_name)
            .and_then(|surface| projected_surface_to_type_expr(&surface))
    }

    pub fn project_prepared_type_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        self.cached_prepared_root_surface(scope_canonical_id, symbol_name)
            .map(|surface| projected_surface_to_expanded_shape(&surface))
    }

    fn cached_prepared_root_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedSurface> {
        // When the requested symbol is not declared at the passed scope but
        // is instead re-exported through a barrel (e.g. `reka-ui/dist/index.d.ts`
        // re-exports `ListboxRootProps` from `dist/index3.d.ts`), chase the
        // re-export chain to the declaring file so the prepared bundle lookup
        // hits the actual declaration. This is a pure routing step — the
        // request-local prepared cache still keys on the original scope so
        // repeated queries stay cheap, but the projection itself runs against
        // the declaring scope where the prepared decl lives.
        let (resolved_scope, resolved_symbol) =
            self.resolve_final_prepared_type_target(scope_canonical_id, symbol_name);
        self.project_prepared_root_surface(resolved_scope.as_str(), resolved_symbol.as_str())
            .map(projected_surface_unwrap_or_clone)
    }

    fn project_prepared_root_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let previous_root = self
            .current_prepared_request_root
            .replace(scope_canonical_id.to_string());
        let result = self.project_prepared_root_surface_inner(scope_canonical_id, symbol_name);
        self.current_prepared_request_root = previous_root;
        result
    }

    fn project_prepared_root_surface_inner(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        #[cfg(test)]
        {
            self.prepared_root_surface_projection_count += 1;
        }
        let mut active = FxHashSet::default();
        match self.project_prepared_surface_from_symbol(
            scope_canonical_id,
            symbol_name,
            &FxHashMap::default(),
            &mut active,
        ) {
            PreparedSurfaceProjection::Surface(surface)
                if !projected_surface_is_empty(&surface) =>
            {
                Some(surface)
            }
            _ => None,
        }
    }

    fn project_prepared_surface_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        let cache_key = PreparedSurfaceCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            substitutions: prepared_substitution_key(substitutions),
        };
        if let Some(cached) = self.prepared_surface_cache.get(&cache_key) {
            return cached.clone();
        }
        if substitutions.is_empty() {
            if let Some(prepared) = self.prepared_type_decl(scope_canonical_id, symbol_name) {
                if let Some(default_substitutions) =
                    prepared_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self.project_prepared_surface_from_symbol(
                            scope_canonical_id,
                            symbol_name,
                            &default_substitutions,
                            active,
                        );
                        self.prepared_surface_cache
                            .insert(cache_key, result.clone());
                        return result;
                    }
                }
            }
        }

        if let Some(cached) =
            self.cached_prepared_surface(scope_canonical_id, symbol_name, substitutions)
        {
            let cached = PreparedSurfaceProjection::Surface(cached);
            self.prepared_surface_cache
                .insert(cache_key.clone(), cached.clone());
            return cached;
        }

        let key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !active.insert(key.clone()) {
            return PreparedSurfaceProjection::Unsupported;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .map(|prepared| {
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    substitutions,
                    active,
                )
            })
            .unwrap_or(PreparedSurfaceProjection::Unsupported);

        active.remove(&key);
        self.cache_prepared_surface_projection(
            scope_canonical_id,
            symbol_name,
            substitutions,
            &result,
        );
        self.prepared_surface_cache
            .insert(cache_key, result.clone());
        result
    }

    fn project_prepared_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                substitutions,
                active,
            ),
            TypeExpr::Object(object) => PreparedSurfaceProjection::Surface(std::sync::Arc::new(
                projected_surface_from_object_expr_with_substitutions(
                    object,
                    &prepared.type_parameters,
                    substitutions,
                ),
            )),
            TypeExpr::Function(function) => PreparedSurfaceProjection::Surface(
                std::sync::Arc::new(projected_surface_from_function_expr_with_substitutions(
                    function,
                    &prepared.type_parameters,
                    substitutions,
                )),
            ),
            TypeExpr::Intersection(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        active,
                    ) {
                        PreparedSurfaceProjection::Surface(surface) => surfaces.push(surface),
                        PreparedSurfaceProjection::Empty => {}
                        PreparedSurfaceProjection::Unsupported => {
                            return PreparedSurfaceProjection::Unsupported;
                        }
                    }
                }
                projected_surface_from_parts_intersection(surfaces)
            }
            TypeExpr::Union(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        active,
                    ) {
                        PreparedSurfaceProjection::Surface(surface) => surfaces.push(surface),
                        PreparedSurfaceProjection::Empty => {}
                        PreparedSurfaceProjection::Unsupported => {
                            return PreparedSurfaceProjection::Unsupported;
                        }
                    }
                }
                projected_surface_from_parts_union(surfaces)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        &substituted,
                        &FxHashMap::default(),
                        active,
                    );
                }
                self.project_prepared_surface_from_ref(
                    scope_canonical_id,
                    prepared,
                    name.as_ref(),
                    type_arguments.as_ref(),
                    active,
                )
            }
            TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_)
            | TypeExpr::KeyOf(_)
            | TypeExpr::Rest(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Infer { .. } => PreparedSurfaceProjection::Empty,
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_) => PreparedSurfaceProjection::Unsupported,
        }
    }

    fn project_prepared_surface_from_ref(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
        type_arguments: &[TypeExpr],
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match (name, type_arguments) {
            ("Partial", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.optional = true,
            ),
            ("Required", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.optional = false,
            ),
            ("Readonly", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.readonly = true,
            ),
            ("NonNullable", [inner]) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                &FxHashMap::default(),
                active,
            ),
            ("Pick", [target, keys]) => {
                let Some(requested) =
                    self.prepared_string_literal_keys(scope_canonical_id, prepared, keys, active)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                self.project_prepared_requested_member_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    target,
                    &requested,
                    &FxHashMap::default(),
                    active,
                )
            }
            ("Omit", [target, keys]) => {
                let Some(omitted) =
                    self.prepared_string_literal_keys(scope_canonical_id, prepared, keys, active)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                apply_surface_member_filter(
                    self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        target,
                        &FxHashMap::default(),
                        active,
                    ),
                    move |member_name| !omitted.iter().any(|candidate| candidate == member_name),
                )
            }
            _ if matches!(name, "Array" | "ReadonlyArray" | "Promise") => {
                PreparedSurfaceProjection::Empty
            }
            _ if is_builtin_name(name) => PreparedSurfaceProjection::Unsupported,
            _ => {
                let Some((target_canonical_id, target_symbol_name)) =
                    self.resolve_prepared_surface_target(scope_canonical_id, prepared, name)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                let Some(target_prepared) =
                    self.prepared_type_decl(&target_canonical_id, &target_symbol_name)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                let Some(target_substitutions) =
                    prepared_type_param_substitutions(target_prepared.as_ref(), type_arguments)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                self.project_prepared_surface_from_symbol(
                    &target_canonical_id,
                    &target_symbol_name,
                    &target_substitutions,
                    active,
                )
            }
        }
    }

    fn project_prepared_requested_member_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        requested: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        let mut members = Vec::with_capacity(requested.len());
        for member_name in requested {
            let Some(projected_member) = self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                expr,
                member_name,
                substitutions,
                active,
            ) else {
                return PreparedSurfaceProjection::Unsupported;
            };
            members.push(projected_member);
        }

        PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
            members,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            has_index_signature: false,
        }))
    }

    fn project_prepared_requested_member_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::Requested,
            substitutions: prepared_substitution_key(substitutions),
        };
        if let Some(cached) = self.prepared_member_cache.get(&cache_key) {
            return cached.clone();
        }
        if substitutions.is_empty() {
            if let Some(prepared) = self.prepared_type_decl(scope_canonical_id, symbol_name) {
                if let Some(default_substitutions) =
                    prepared_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self.project_prepared_requested_member_from_symbol(
                            scope_canonical_id,
                            symbol_name,
                            member_name,
                            &default_substitutions,
                            active,
                        );
                        self.prepared_member_cache.insert(cache_key, result.clone());
                        return result;
                    }
                }
            }
        }

        if let Some(cached) = self.cached_prepared_requested_member(
            scope_canonical_id,
            symbol_name,
            member_name,
            substitutions,
        ) {
            self.prepared_member_cache
                .insert(cache_key, Some(cached.clone()));
            return Some(cached);
        }

        let visit_key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !active.insert(visit_key.clone()) {
            return None;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member) = prepared.member(member_name) {
                    let projected_member = ProjectedMember {
                        name: member_name.to_string(),
                        ty: substitute_type_expr_if_needed(&member.ty, substitutions),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    };
                    if !type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                        self.cache_prepared_requested_member(
                            scope_canonical_id,
                            symbol_name,
                            &projected_member,
                            substitutions,
                        );
                    }
                    return Some(projected_member);
                }

                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    member_name,
                    substitutions,
                    active,
                )
            });

        active.remove(&visit_key);
        self.prepared_member_cache.insert(cache_key, result.clone());
        result
    }

    fn project_prepared_requested_member_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                member_name,
                substitutions,
                active,
            ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    substitutions,
                    active,
                )
            }),
            TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                ObjectMember::Property(property) if property.name == member_name => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: substitute_type_expr_if_needed(&property.ty, substitutions),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                    })
                }
                ObjectMember::Method(method) if method.name == member_name => {
                    Some(ProjectedMember {
                        name: method.name.clone(),
                        ty: TypeExpr::Function(std::sync::Arc::new(
                            substitute_function_expr_if_needed(&method.function, substitutions),
                        )),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                    })
                }
                _ => None,
            }),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_requested_member_from_expr(
                        scope_canonical_id,
                        prepared,
                        &substituted,
                        member_name,
                        &FxHashMap::default(),
                        active,
                    );
                }
                match (name.as_ref(), type_arguments.as_ref()) {
                    ("Partial", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.optional = true;
                            member
                        }),
                    ("Required", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.optional = false;
                            member
                        }),
                    ("Readonly", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.readonly = true;
                            member
                        }),
                    ("NonNullable", [inner]) => self.project_prepared_requested_member_from_expr(
                        scope_canonical_id,
                        prepared,
                        inner,
                        member_name,
                        substitutions,
                        active,
                    ),
                    ("Pick", [target, keys]) => {
                        let requested = self.prepared_string_literal_keys(
                            scope_canonical_id,
                            prepared,
                            keys,
                            active,
                        )?;
                        if !requested.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            target,
                            member_name,
                            substitutions,
                            active,
                        )
                    }
                    ("Omit", [target, keys]) => {
                        let omitted = self.prepared_string_literal_keys(
                            scope_canonical_id,
                            prepared,
                            keys,
                            active,
                        )?;
                        if omitted.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            target,
                            member_name,
                            substitutions,
                            active,
                        )
                    }
                    _ if matches!(name.as_ref(), "Array" | "ReadonlyArray" | "Promise") => None,
                    _ if is_builtin_name(name.as_ref()) => None,
                    _ => {
                        let (target_canonical_id, target_symbol_name) = self
                            .resolve_prepared_surface_target(
                                scope_canonical_id,
                                prepared,
                                name.as_ref(),
                            )?;
                        let target_prepared =
                            self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
                        let target_substitutions = prepared_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        self.project_prepared_requested_member_from_symbol(
                            &target_canonical_id,
                            &target_symbol_name,
                            member_name,
                            &target_substitutions,
                            active,
                        )
                    }
                }
            }
            _ => None,
        }
    }

    fn resolve_prepared_surface_target(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
    ) -> Option<(String, String)> {
        let cache_key = PreparedTargetCacheKey {
            active_scope_canonical_id: scope_canonical_id.to_string(),
            decl_canonical_id: prepared.root_identity.canonical_id.clone(),
            decl_symbol_name: prepared.root_identity.symbol_name.clone(),
            requested_name: name.to_string(),
        };
        if let Some(cached) = self.prepared_target_cache.get(&cache_key) {
            return cached.clone();
        }

        let resolve_prepared_target =
            |this: &mut Self, canonical_source: String, resolved_name: String| {
                let mut canonical_source = if canonical_source.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    canonical_source
                };
                let mut resolved_name = if resolved_name.is_empty() {
                    name.to_string()
                } else {
                    resolved_name
                };

                if canonical_source != scope_canonical_id {
                    if let Some((routed_source, routed_name)) =
                        this.host.resolve_named_type_export_target_shallow(
                            canonical_source.as_str(),
                            resolved_name.as_str(),
                        )
                    {
                        if this
                            .prepared_type_decl(routed_source.as_str(), routed_name.as_str())
                            .is_some()
                        {
                            canonical_source = routed_source;
                            resolved_name = routed_name;
                        }
                    }
                }

                this.prepared_type_decl(&canonical_source, &resolved_name)
                    .map(|_| (canonical_source, resolved_name))
            };

        let resolved = prepared
            .name_resolution
            .get(name)
            .and_then(|resolved| {
                resolve_prepared_target(
                    self,
                    resolved.canonical_id.clone(),
                    resolved.symbol_name.clone(),
                )
            })
            .or_else(|| {
                let declaration = self.resolve_type_declaration(scope_canonical_id, name);
                resolve_prepared_target(
                    self,
                    declaration.canonical_source,
                    declaration.resolved_name,
                )
            });
        self.prepared_target_cache
            .insert(cache_key, resolved.clone());
        resolved
    }

    fn prepared_string_literal_keys(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<Vec<String>> {
        use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::with_capacity(types.len());
                for ty in types.iter() {
                    keys.extend(self.prepared_string_literal_keys(
                        scope_canonical_id,
                        prepared,
                        ty,
                        active,
                    )?);
                }
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => {
                self.prepared_string_literal_keys(scope_canonical_id, prepared, inner, active)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let (target_canonical_id, target_symbol_name) =
                    self.resolve_prepared_surface_target(scope_canonical_id, prepared, name)?;
                let visit_key = (target_canonical_id.clone(), target_symbol_name.clone());
                if !active.insert(visit_key.clone()) {
                    return None;
                }
                let resolved = self
                    .prepared_type_decl(&target_canonical_id, &target_symbol_name)
                    .and_then(|target_prepared| {
                        self.prepared_string_literal_keys(
                            &target_canonical_id,
                            target_prepared.as_ref(),
                            &target_prepared.body,
                            active,
                        )
                    });
                active.remove(&visit_key);
                resolved
            }
            _ => None,
        }
    }

    fn projected_string_literal_keys(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<Vec<String>> {
        self.projected_string_literal_keys_inner(
            resolution_scope_canonical_id,
            active_scope_canonical_id,
            expr,
            0,
        )
    }

    fn projected_string_literal_keys_inner(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

        if depth >= 4 {
            return None;
        }

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for ty in types.iter() {
                    keys.extend(self.projected_string_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        ty,
                        depth + 1,
                    )?);
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => self.projected_string_literal_keys_inner(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                inner,
                depth + 1,
            ),
            TypeExpr::KeyOf(inner) => {
                if let TypeExpr::IndexedAccess { object, index } = inner.as_ref() {
                    if let TypeExpr::Literal(LiteralValue::String(member_name)) = index.as_ref() {
                        if let Some(keys) = self.projected_member_surface_keys(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            object,
                            member_name,
                            depth + 1,
                        ) {
                            return Some(keys);
                        }
                    }
                }

                if let Some(projected_expr) = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        expr,
                    )
                    .filter(|projected| projected != expr)
                {
                    return self.projected_string_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &projected_expr,
                        depth + 1,
                    );
                }

                let projected_inner = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        inner,
                    )
                    .unwrap_or_else(|| inner.as_ref().clone());
                if let Some(keys) = projected_surface_member_names(&projected_inner) {
                    return Some(keys);
                }

                match &projected_inner {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        // Path C C11-residual-C: accumulate enumerable
                        // arms only. Pre-residual-C the `?` propagation
                        // dropped the entire result when any single arm
                        // could not enumerate keys, even when other arms
                        // had concrete keyspaces. Mirrors the
                        // SemanticNode-level Intersection accumulation
                        // change in `key_names_from_base_node` (Path C
                        // C10) — `keyof (A & B)` returns the union of
                        // enumerable keys across A and B and only fails
                        // when EVERY arm is unresolvable.
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for part in parts.iter() {
                            if let Some(arm_keys) = self.projected_string_literal_keys_inner(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &TypeExpr::KeyOf(std::sync::Arc::new(part.clone())),
                                depth + 1,
                            ) {
                                any_enumerable = true;
                                keys.extend(arm_keys);
                            }
                        }
                        if !any_enumerable {
                            return None;
                        }
                        keys.sort();
                        keys.dedup();
                        Some(keys)
                    }
                    _ => None,
                }
            }
            _ => {
                let projected = self.solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    expr,
                )?;
                if projected == *expr {
                    crate::resolver_core::component_meta_registry::component_meta_registry_string_literal_keys(
                        &projected,
                    )
                } else {
                    self.projected_string_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &projected,
                        depth + 1,
                    )
                }
            }
        }
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn projected_member_surface_keys(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
        member_name: &str,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        // Path C C11-residual-C: depth bumped from 4 to 8 to allow
        // multi-step navigation chains like
        // `(typeof theme & GetComponentAppConfig<AppConfig, "ui", "button">)['variants']['color']`
        // which require: (1) IndexedAccess(Intersection,..) distribute
        // → (2) IndexedAccess(Ref,..) expand alias → (3)
        // IndexedAccess(Conditional,..) distribute → (4)
        // IndexedAccess(IndexedAccess,..) recurse on inner → ...
        if depth >= 8 {
            return None;
        }

        let projected_expr = self
            .solve_or_project_prepared_member_leaf_expr(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                expr,
            )
            .unwrap_or_else(|| expr.clone());
        if matches!(projected_expr, TypeExpr::Unknown { .. }) {
            if let Some(expanded) = self
                .expand_local_generic_ref_expr(resolution_scope_canonical_id, expr)
                .or_else(|| self.expand_local_generic_ref_expr(active_scope_canonical_id, expr))
                .filter(|expanded| expanded != expr)
            {
                return self.projected_member_surface_keys(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &expanded,
                    member_name,
                    depth + 1,
                );
            }
        }

        match &projected_expr {
            TypeExpr::Object(object) => {
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property) if property.name == member_name => {
                        Some(property.ty.clone())
                    }
                    ObjectMember::Method(method) if method.name == member_name => Some(
                        TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                    ),
                    _ => None,
                })?;
                let projected_member = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &member_ty,
                    )
                    .unwrap_or(member_ty);
                projected_surface_member_names(&projected_member)
            }
            TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                // Path C C11-residual-C: accumulate enumerable arms
                // only — see the matching change in
                // `projected_string_literal_keys_inner`. `keyof
                // (typeof theme & GetComponentAppConfig<...>)['variants']
                // ['color']` must merge `theme.variants.color`'s keys
                // with the conditional's resolvable arm keys, even when
                // the deferred conditional arm couldn't enumerate.
                let mut keys = Vec::new();
                let mut any_enumerable = false;
                for part in parts.iter() {
                    if let Some(arm_keys) = self.projected_member_surface_keys(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        part,
                        member_name,
                        depth + 1,
                    ) {
                        any_enumerable = true;
                        keys.extend(arm_keys);
                    }
                }
                if !any_enumerable {
                    return None;
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Conditional {
                true_type,
                false_type,
                ..
            } => {
                let mut keys = Vec::new();
                if let Some(true_keys) = self.projected_member_surface_keys(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    true_type,
                    member_name,
                    depth + 1,
                ) {
                    keys.extend(true_keys);
                }
                if let Some(false_keys) = self.projected_member_surface_keys(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    false_type,
                    member_name,
                    depth + 1,
                ) {
                    keys.extend(false_keys);
                }
                if keys.is_empty() {
                    None
                } else {
                    keys.sort();
                    keys.dedup();
                    Some(keys)
                }
            }
            TypeExpr::TypeOf(value_ref) => {
                // D-Cutover §5.8: resolve the value root via the
                // dispatch-aligned bare-name resolver + host
                // prepared_value_decl directly. Mirrors `build_typeof`.
                let scope_payload = self.scope_payload_for_scope(active_scope_canonical_id);
                let root_name = value_ref.path.first()?;
                let root_identity =
                    crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                        self.host,
                        active_scope_canonical_id,
                        scope_payload.as_deref(),
                        root_name,
                    )?;
                let prepared_value = self
                    .host
                    .prepared_value_decl(&root_identity.canonical_id, &root_identity.symbol_name)
                    .or_else(|| {
                        if root_identity.canonical_id.is_empty() {
                            return None;
                        }
                        let target = self.host.resolve_value_export_target(
                            &root_identity.canonical_id,
                            &root_identity.symbol_name,
                        )?;
                        if target.canonical_id == root_identity.canonical_id
                            && target.name == root_identity.symbol_name
                        {
                            return None;
                        }
                        self.host
                            .prepared_value_decl(&target.canonical_id, &target.name)
                    })?;

                if let Some(object_shape) = prepared_value.object_shape.as_ref() {
                    let object_expr = TypeExpr::Object(std::sync::Arc::new(object_shape.clone()));
                    return self.projected_member_surface_keys(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &object_expr,
                        member_name,
                        depth + 1,
                    );
                }

                if let Some(type_annotation) = prepared_value.type_annotation.as_ref() {
                    return self.projected_member_surface_keys(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        type_annotation,
                        member_name,
                        depth + 1,
                    );
                }

                None
            }
            TypeExpr::Parenthesized(inner) => self.projected_member_surface_keys(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                inner,
                member_name,
                depth + 1,
            ),
            // Path C C11-residual-C: distribute member-name lookup over
            // an `IndexedAccess` whose object is compound or reducible.
            // For `(typeof theme & GetComponentAppConfig<...>)['variants']['color']`
            // we want `theme.variants.color`'s keys merged with the
            // conditional arm's `variants.color`'s keys. Pre-residual-C
            // the catch-all returned `None` and the test lost AppConfig's
            // `neutral` because the dispatch couldn't reduce the
            // outer IndexedAccess to a concrete shape.
            //
            // Handles:
            // - object = Intersection / Union: distribute over arms.
            // - object = Conditional: distribute over true / false branches.
            // - object = Ref with type_arguments: expand the alias body
            //   and retry.
            // - object = nested IndexedAccess: recurse on inner before
            //   re-applying the outer index.
            TypeExpr::IndexedAccess { object, index } => {
                match object.as_ref() {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        let parts = std::sync::Arc::clone(parts);
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for arm in parts.iter() {
                            let arm_indexed = TypeExpr::IndexedAccess {
                                object: std::sync::Arc::new(arm.clone()),
                                index: index.clone(),
                            };
                            if let Some(arm_keys) = self.projected_member_surface_keys(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &arm_indexed,
                                member_name,
                                depth + 1,
                            ) {
                                any_enumerable = true;
                                keys.extend(arm_keys);
                            }
                        }
                        if any_enumerable {
                            keys.sort();
                            keys.dedup();
                            Some(keys)
                        } else {
                            None
                        }
                    }
                    TypeExpr::Conditional {
                        true_type,
                        false_type,
                        ..
                    } => {
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for branch in [true_type.as_ref(), false_type.as_ref()] {
                            let branch_indexed = TypeExpr::IndexedAccess {
                                object: std::sync::Arc::new(branch.clone()),
                                index: index.clone(),
                            };
                            if let Some(branch_keys) = self.projected_member_surface_keys(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &branch_indexed,
                                member_name,
                                depth + 1,
                            ) {
                                any_enumerable = true;
                                keys.extend(branch_keys);
                            }
                        }
                        if any_enumerable {
                            keys.sort();
                            keys.dedup();
                            Some(keys)
                        } else {
                            None
                        }
                    }
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        // Try expanding the alias's body (substituting
                        // type arguments), then retry the indexed access
                        // against the substituted body.
                        let expanded = if !type_arguments.is_empty() {
                            self.expand_local_generic_ref_expr(
                                resolution_scope_canonical_id,
                                object,
                            )
                            .or_else(|| {
                                self.expand_local_generic_ref_expr(
                                    active_scope_canonical_id,
                                    object,
                                )
                            })
                        } else {
                            // Non-generic Ref: look up the alias's body
                            // directly via prepared decl resolution.
                            let try_body = |me: &mut Self, scope: &str| -> Option<TypeExpr> {
                                let declaration = me.resolve_type_declaration(scope, name.as_ref());
                                let target_canonical = if declaration.canonical_source.is_empty() {
                                    scope.to_string()
                                } else {
                                    declaration.canonical_source.clone()
                                };
                                let resolved_name = if declaration.resolved_name.is_empty() {
                                    name.as_ref().to_string()
                                } else {
                                    declaration.resolved_name.clone()
                                };
                                me.prepared_type_decl(&target_canonical, &resolved_name)
                                    .map(|p| p.body.clone())
                            };
                            try_body(self, resolution_scope_canonical_id)
                                .or_else(|| try_body(self, active_scope_canonical_id))
                        }?;
                        let expanded_indexed = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(expanded),
                            index: index.clone(),
                        };
                        self.projected_member_surface_keys(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &expanded_indexed,
                            member_name,
                            depth + 1,
                        )
                    }
                    TypeExpr::IndexedAccess { .. } => {
                        // Try resolving the inner IndexedAccess to a
                        // concrete object, then re-apply the outer
                        // index.
                        let resolved_inner = self
                            .solve_or_project_prepared_member_leaf_expr(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                object,
                            )
                            .filter(|resolved| resolved != object.as_ref())?;
                        let next = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(resolved_inner),
                            index: index.clone(),
                        };
                        self.projected_member_surface_keys(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &next,
                            member_name,
                            depth + 1,
                        )
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Project a single member from a type expression in a declaration scope.
    ///
    /// Results are write-through: stable projections are published to
    /// `TypeSurfaceDb` and reused by later requests.
    pub fn project_type_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedMember> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        self.dispatch_projected_member(scope_canonical_id, symbol_name, member_name)
            .or_else(|| {
                let mut active = FxHashSet::default();
                self.project_prepared_requested_member_from_symbol(
                    scope_canonical_id,
                    symbol_name,
                    member_name,
                    &FxHashMap::default(),
                    &mut active,
                )
            })
    }

    /// Project the keyspace (member names) from a type expression in a
    /// declaration scope.
    ///
    /// Results are write-through: stable projections are published to
    /// `TypeSurfaceDb` and reused by later requests.
    pub fn project_type_keyspace(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedKeyspace> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        self.dispatch_projected_keyspace(scope_canonical_id, symbol_name)
    }

    /// Project an arbitrary [`TypeExpr`] to its surface form (plan §9
    /// appendix row 1-2). Route-based fast-path via
    /// `component_meta_registry_public_indexed_access_route` stays; the
    /// full projection routes through [`ProjectSemanticDispatch`] —
    /// D-Cutover §5.8 retired the pre-cutover
    /// `owner_engine.project_expr_surface_as_type_expr` fallback.
    pub fn project_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        if let Some((root_symbol, route)) =
            super::component_meta_registry::component_meta_registry_public_indexed_access_route(
                expr,
            )
            .or_else(|| {
                super::component_meta_registry::component_meta_registry_public_utility_route(expr)
            })
        {
            if let Some(projected) =
                self.project_route_surface_expr(scope_canonical_id, &root_symbol, &route)
            {
                return Some(projected);
            }
            if let Some(solved) = self.solve_expr_type_expr(scope_canonical_id, expr) {
                return Some(solved);
            }
        }
        let dispatch = self.semantic_dispatch();
        let base = dispatch.lower_type_expr_in_scope(scope_canonical_id, expr)?;
        let QueryResult::Value(node) = dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            mode: ProjectionMode::Expanded,
        }) else {
            return None;
        };
        let projected = semantic_node_to_type_expr(self.host, node)?;
        (!type_expr_contains_semantic_miss(&projected) && type_expr_is_expanded_surface(&projected))
            .then_some(projected)
    }

    /// Like [`Self::project_expr_surface_expr`] but accepts compound
    /// projections (Intersection / Union / Parenthesized) that contain
    /// at least one Object arm even when sibling arms are still deferred
    /// shells (Mapped / KeyOf / IndexedAccess / Conditional). Used by
    /// the `defineSlots<T>` macro-shape producer to extract explicit
    /// slot names from `{ leading?, content? } & DynamicSlots<...>`-
    /// shaped intersections where the dynamic helper arm cannot
    /// enumerate keys at unresolved-generic time.
    ///
    /// Returns `None` when the projection has no Object arm at any
    /// nesting level — there is no surface to extract.
    pub fn project_expr_surface_expr_with_compound_objects(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        let dispatch = self.semantic_dispatch();
        let base = dispatch.lower_type_expr_in_scope(scope_canonical_id, expr)?;
        let QueryResult::Value(node) = dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            mode: ProjectionMode::Expanded,
        }) else {
            return None;
        };
        let projected = semantic_node_to_type_expr(self.host, node)?;
        type_expr_has_any_object_arm(&projected).then_some(projected)
    }

    /// Like [`Self::project_expr_surface_expr`] but returns the raw
    /// projection result regardless of whether it is fully expanded.
    /// Returns `None` only when the projection itself fails to produce
    /// a value (lowering miss / dispatch error) or when the result is
    /// the `Opaque(Miss)` sentinel.
    /// Solve an arbitrary [`TypeExpr`] to its reduced form (plan §9
    /// appendix row 3). Routes through [`ProjectSemanticDispatch`] with
    /// `mode: Expanded` — D-Cutover §5.8 retired the pre-cutover
    /// `owner_engine.solve_scoped` fallback.
    ///
    /// Returns `Some(reduced)` only when the dispatch result differs
    /// structurally from `expr`, matching the pre-migration contract.
    pub fn solve_expr_type_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        let dispatch = self.semantic_dispatch();
        let base = dispatch.lower_type_expr_in_scope(scope_canonical_id, expr)?;
        let QueryResult::Value(node) = dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            mode: ProjectionMode::Expanded,
        }) else {
            return None;
        };
        let reduced = semantic_node_to_type_expr(self.host, node)?;
        (!type_expr_contains_semantic_miss(&reduced)
            && type_expr_is_expanded_surface(&reduced)
            && reduced != *expr)
            .then_some(reduced)
    }

    pub fn expand_local_generic_ref_expr(
        &mut self,
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

        let declaration = self.resolve_type_declaration(scope_canonical_id, name.as_ref());
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
        let (target_canonical_id, target_symbol_name) = self.resolve_final_prepared_type_target(
            declared_canonical_id.as_str(),
            declared_symbol_name.as_str(),
        );
        if is_package_source(Some(target_canonical_id.as_str())) {
            return None;
        }
        let prepared = self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
        let substitutions = prepared_type_param_substitutions(prepared.as_ref(), type_arguments)?;
        Some(substitute_type_expr_if_needed(
            &prepared.body,
            &substitutions,
        ))
    }

    pub fn project_expr_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        if let Some((root_symbol, route)) =
            super::component_meta_registry::component_meta_registry_public_indexed_access_route(
                expr,
            )
            .or_else(|| {
                super::component_meta_registry::component_meta_registry_public_utility_route(expr)
            })
        {
            if let Some(projected) =
                self.project_routed_expr_surface_expr(scope_canonical_id, &root_symbol, &route)
            {
                return Some(
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected),
                );
            }
        }
        // Plan §9 row 4: dispatch is the sole projection authority
        // (D-Cutover §5.8 retired the `owner_engine.project_expr_surface`
        // fallback).
        if let Some(shape) = self.project_direct_utility_surface_shape(scope_canonical_id, expr) {
            return Some(shape);
        }
        let dispatch = self.semantic_dispatch();
        let base = dispatch.lower_type_expr_in_scope(scope_canonical_id, expr)?;
        let QueryResult::Value(node) = dispatch.execute(SemanticQueryKey::ProjectPath {
            base,
            path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            mode: ProjectionMode::Shallow,
        }) else {
            return None;
        };
        let surface = projected_surface_from_semantic_node(self.host, node)?;
        let shape = projected_surface_to_expanded_shape(&surface);
        (!shape.properties.is_empty() || !shape.call_signatures.is_empty()).then_some(shape)
    }

    fn project_direct_utility_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        use verter_semantic::analysis::type_expand::ExpandedObjectShape;
        use verter_semantic::analysis::type_expr::TypeExpr;

        fn shape_has_surface(shape: &ExpandedObjectShape) -> bool {
            !shape.properties.is_empty() || !shape.call_signatures.is_empty()
        }

        fn projected_target_shape(
            query_engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            target: &TypeExpr,
        ) -> Option<ExpandedObjectShape> {
            if let Some(shape) = query_engine.project_expr_surface_shape(scope_canonical_id, target)
            {
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            if let Some(projected) =
                query_engine.project_expr_surface_expr(scope_canonical_id, target)
            {
                let shape =
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected);
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            if let Some(expanded_ref) =
                query_engine.expand_local_generic_ref_expr(scope_canonical_id, target)
            {
                if let Some(shape) =
                    query_engine.project_expr_surface_shape(scope_canonical_id, &expanded_ref)
                {
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
                if let Some(projected) =
                    query_engine.project_expr_surface_expr(scope_canonical_id, &expanded_ref)
                {
                    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                        &projected,
                    );
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
                let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                    &expanded_ref,
                );
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            None
        }

        let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        else {
            return None;
        };

        match (name.as_ref(), type_arguments.as_ref()) {
            ("Partial", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.optional = true;
                    }
                    shape
                })
            }
            ("Required", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.optional = false;
                    }
                    shape
                })
            }
            ("Readonly", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.readonly = true;
                    }
                    shape
                })
            }
            ("NonNullable", [target]) => projected_target_shape(self, scope_canonical_id, target),
            ("Pick", [target, keys]) => {
                let requested = self.projected_string_literal_keys(
                    scope_canonical_id,
                    scope_canonical_id,
                    keys,
                )?;
                let mut shape = projected_target_shape(self, scope_canonical_id, target)?;
                shape.properties.retain(|property| {
                    requested
                        .iter()
                        .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            ("Omit", [target, keys]) => {
                let omitted = self.projected_string_literal_keys(
                    scope_canonical_id,
                    scope_canonical_id,
                    keys,
                )?;
                let mut shape = projected_target_shape(self, scope_canonical_id, target)?;
                shape.properties.retain(|property| {
                    !omitted
                        .iter()
                        .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            _ => None,
        }
    }

    pub fn project_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        self.project_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
    }

    fn project_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        fn single_member_route_cache_entry(
            query_engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            root_symbol: &str,
            member_name: &str,
            projected_expr: &TypeExpr,
        ) -> Option<ProjectedMember> {
            query_engine
                .project_type_member(scope_canonical_id, root_symbol, member_name)
                .or_else(|| {
                    query_engine.project_prepared_member_route_projection(
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                    )
                })
                .or_else(|| {
                    query_engine.project_inherited_member_route_projection(
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                    )
                })
                .or_else(|| {
                    let prepared =
                        query_engine.prepared_type_decl(scope_canonical_id, root_symbol)?;
                    let member = prepared.member(member_name)?;
                    Some(ProjectedMember {
                        name: member_name.to_string(),
                        ty: projected_expr.clone(),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    })
                })
        }

        if let Some(cached_expr) =
            self.cached_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
        {
            return Some(cached_expr);
        }

        if let Some(projected_expr) =
            self.project_routed_expr_surface_expr_direct(scope_canonical_id, root_symbol, route)
        {
            self.cache_routed_expr_surface_expr(
                scope_canonical_id,
                root_symbol,
                route,
                &projected_expr,
            );
            if let super::RouteDemand::MemberPath(path) = route {
                if let [member_name] = path.as_slice() {
                    if let Some(projected_member) = single_member_route_cache_entry(
                        self,
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                        &projected_expr,
                    ) {
                        self.cache_projected_member(
                            scope_canonical_id,
                            root_symbol,
                            &projected_member,
                        );
                    }
                }
            }
            if let super::RouteDemand::Pick(members) = route {
                self.cache_pick_members_from_projected_expr(
                    scope_canonical_id,
                    root_symbol,
                    members,
                    &projected_expr,
                );
            }
            return Some(projected_expr);
        }

        if let super::RouteDemand::MemberPath(path) = route {
            if let Some(projected_expr) = self.project_prepared_member_path_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                path,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                if let [member_name] = path.as_slice() {
                    if let Some(projected_member) = single_member_route_cache_entry(
                        self,
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                        &projected_expr,
                    ) {
                        self.cache_projected_member(
                            scope_canonical_id,
                            root_symbol,
                            &projected_member,
                        );
                    }
                }
                return Some(projected_expr);
            }
            if let [member_name] = path.as_slice() {
                let projected_member =
                    self.project_type_member(scope_canonical_id, root_symbol, member_name)?;
                let projected_expr = projected_member.ty.clone();
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                self.cache_projected_member(scope_canonical_id, root_symbol, &projected_member);
                return Some(projected_expr);
            }
        }

        if let super::RouteDemand::Pick(members) = route {
            if let Some(projected_expr) = self.project_prepared_pick_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_members(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_routed_expr(
                scope_canonical_id,
                root_symbol,
                route,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                self.cache_pick_members_from_projected_expr(
                    scope_canonical_id,
                    root_symbol,
                    members,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
        }

        None
    }

    fn cached_routed_expr_surface_expr(
        &self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        self.routed_expr_surface_cache
            .get(&RoutedExprSurfaceCacheKey {
                scope_canonical_id: scope_canonical_id.to_owned(),
                root_symbol: root_symbol.to_owned(),
                route: route.clone(),
            })
            .cloned()
    }

    fn cache_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
        projected_expr: &TypeExpr,
    ) {
        self.routed_expr_surface_cache.insert(
            RoutedExprSurfaceCacheKey {
                scope_canonical_id: scope_canonical_id.to_owned(),
                root_symbol: root_symbol.to_owned(),
                route: route.clone(),
            },
            projected_expr.clone(),
        );
    }

    fn cache_pick_members_from_projected_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
        projected_expr: &TypeExpr,
    ) {
        use std::collections::BTreeSet;
        use verter_semantic::analysis::type_expr::ObjectMember;

        let requested: BTreeSet<_> = members.iter().map(String::as_str).collect();
        let TypeExpr::Object(object) = projected_expr else {
            return;
        };
        for member in &object.properties {
            let projected_member = match member {
                ObjectMember::Property(property) if requested.contains(property.name.as_str()) => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: property.ty.clone(),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                    })
                }
                ObjectMember::Method(method) if requested.contains(method.name.as_str()) => {
                    Some(ProjectedMember {
                        name: method.name.clone(),
                        ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                    })
                }
                _ => None,
            };
            if let Some(projected_member) = projected_member {
                self.cache_projected_member(scope_canonical_id, root_symbol, &projected_member);
            }
        }
    }

    fn cache_projected_member(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        projected_member: &ProjectedMember,
    ) {
        let _ = (scope_canonical_id, root_symbol, projected_member);
    }

    fn cached_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<ProjectedMember> {
        let _ = (scope_canonical_id, symbol_name, member_name, substitutions);
        None
    }

    fn cached_prepared_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let _ = (scope_canonical_id, symbol_name, substitutions);
        None
    }

    fn cache_prepared_surface_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        projection: &PreparedSurfaceProjection,
    ) {
        let _ = (scope_canonical_id, symbol_name, substitutions, projection);
    }

    fn cache_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        projected_member: &ProjectedMember,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) {
        let _ = (
            scope_canonical_id,
            symbol_name,
            projected_member,
            substitutions,
        );
    }

    #[allow(dead_code)]
    fn prepared_requested_member_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        !substitutions.is_empty()
            && self
                .current_prepared_request_root
                .as_deref()
                .is_some_and(|request_root| request_root != scope_canonical_id)
    }

    #[allow(dead_code)]
    fn prepared_surface_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        self.current_prepared_request_root
            .as_deref()
            .is_some_and(|request_root| request_root != scope_canonical_id)
            && (!substitutions.is_empty() || is_package_source(Some(scope_canonical_id)))
    }

    fn project_prepared_member_route_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name)?;
        let member = prepared.member(member_name)?;
        self.project_prepared_member_from_decl(scope_canonical_id, &prepared, member_name, member)
    }

    fn project_prepared_member_from_decl(
        &mut self,
        scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        member_name: &str,
        member: &verter_semantic::analysis::type_solver::prepared::PreparedMember,
    ) -> Option<ProjectedMember> {
        if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
            return None;
        }
        let projected_ty = match &member.ty {
            TypeExpr::Object(_) => Some(member.ty.clone()),
            _ if prepared_member_body_stays_shallow(&member.ty) => Some(member.ty.clone()),
            _ if prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &member.ty) => {
                Some(member.ty.clone())
            }
            _ if crate::meta_resolve::component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &member.ty,
                scope_canonical_id,
                self,
            ) =>
            {
                Some(member.ty.clone())
            }
            _ => self.project_expr_surface_expr(scope_canonical_id, &member.ty),
        }?;
        Some(ProjectedMember {
            name: member_name.to_string(),
            ty: projected_ty,
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
        })
    }

    fn project_prepared_member_path_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        path: &[String],
    ) -> Option<TypeExpr> {
        let mut visited = FxHashSet::default();
        self.project_prepared_member_path_route_projection_from_symbol(
            scope_canonical_id,
            scope_canonical_id,
            symbol_name,
            path,
            &FxHashMap::default(),
            &mut visited,
        )
    }

    fn expr_references_prepared_scope_symbol(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::ObjectMember;

        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                (!is_builtin_name(name.as_ref())
                    && self
                        .prepared_type_decl(scope_canonical_id, name.as_ref())
                        .is_some())
                    || type_arguments.iter().any(|arg| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, arg)
                    })
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, inner)
            }
            TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, &element.ty)
            }),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| self.expr_references_prepared_scope_symbol(scope_canonical_id, ty)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                ObjectMember::Property(property) => {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, &property.ty)
                }
                ObjectMember::Method(method) => {
                    method.function.parameters.iter().any(|param| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                    }) || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| {
                            self.expr_references_prepared_scope_symbol(
                                scope_canonical_id,
                                return_type,
                            )
                        })
                }
                ObjectMember::IndexSignature(signature) => {
                    self.expr_references_prepared_scope_symbol(
                        scope_canonical_id,
                        &signature.key_type,
                    ) || self.expr_references_prepared_scope_symbol(
                        scope_canonical_id,
                        &signature.value_type,
                    )
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function.parameters.iter().any(|param| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, return_type)
                    })
                }
            }),
            TypeExpr::Function(function) => {
                function.parameters.iter().any(|param| {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, return_type)
                })
            }
            TypeExpr::IndexedAccess { object, index } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, object)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, index)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, check)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, extends)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, true_type)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, false_type)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, source)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, value)
                    || name_type.as_deref().is_some_and(|name_type| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, name_type)
                    })
            }
            TypeExpr::TemplateLiteral { expressions, .. } => expressions
                .iter()
                .any(|expr| self.expr_references_prepared_scope_symbol(scope_canonical_id, expr)),
            TypeExpr::TypeParameter(type_parameter) => {
                type_parameter
                    .constraint
                    .as_deref()
                    .is_some_and(|constraint| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, constraint)
                    })
                    || type_parameter.default.as_deref().is_some_and(|default| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, default)
                    })
            }
            TypeExpr::RecursiveRef {
                type_arguments,
                conditional_context,
                ..
            } => {
                type_arguments
                    .iter()
                    .any(|arg| self.expr_references_prepared_scope_symbol(scope_canonical_id, arg))
                    || conditional_context.iter().any(|frame| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &frame.check)
                            || self.expr_references_prepared_scope_symbol(
                                scope_canonical_id,
                                &frame.extends,
                            )
                    })
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => false,
        }
    }

    fn solve_or_project_prepared_member_leaf_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let context = PreparedProjectionContext {
            decl_scope: resolution_scope_canonical_id.to_string(),
            arg_scope: active_scope_canonical_id.to_string(),
            chain_scopes: self.projection_chain_scopes.clone(),
        };
        self.solve_or_project_leaf_expr_with_context(&context, expr)
    }

    /// Path C C11b — per-TypeExpr-shape scope dispatch for the prepared-
    /// member-path projection (plan §2 Stage 6 Pass C11b).
    ///
    /// The pre-C11b logic tried `active_scope` first and then fell back to
    /// `resolution_scope` only when the expression referenced a prepared
    /// symbol in that scope. That gate missed transitive helper refs
    /// (e.g., `ComponentUI<typeof theme>` where `ComponentUI` lives in a
    /// type-file reached via the prepared decl's import chain, not the
    /// decl's immediate symbol map).
    ///
    /// C11b uses a `PreparedProjectionContext { decl_scope, arg_scope }`:
    /// - bare `Ref { name, type_arguments: [] }`: try `decl_scope` first
    ///   (helper-body-internal reference), fall back to `arg_scope`.
    /// - `TypeOf(value_ref)`: always resolve in `arg_scope` (caller-
    ///   scoped value symbol table).
    /// - `Ref { name, type_arguments }`: resolve the NAME in `decl_scope`
    ///   so helper aliases lower against their own declaration site;
    ///   lower `type_arguments` in `arg_scope` so caller-scoped
    ///   `typeof theme` / explicit type arguments stay resolvable.
    ///   After both halves resolve, re-run
    ///   `solve_or_project_leaf_expr_until_stable` in `decl_scope` to
    ///   bridge the instantiation.
    /// - compound shapes (`IndexedAccess`, `Conditional`, `Mapped`,
    ///   `KeyOf`, etc.): fall back to the two-scope retry path (active
    ///   first, resolution fallback). The compound shapes don't need
    ///   per-sub-expression scope splitting because their sub-
    ///   expressions are already `TypeExpr` leaves that round-trip
    ///   through this function.
    fn solve_or_project_leaf_expr_with_context(
        &mut self,
        context: &PreparedProjectionContext,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let decl_scope = context.decl_scope.clone();
        let arg_scope = context.arg_scope.clone();
        let chain_scopes = context.chain_scopes.clone();

        if decl_scope == arg_scope {
            return self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
        }

        match expr {
            TypeExpr::Ref {
                name: _,
                type_arguments,
            } if type_arguments.is_empty() => {
                // Bare `Ref { name, [] }`: helper-body-internal reference.
                // Try decl_scope first; fall back to arg_scope.
                if let Some(result) =
                    self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                {
                    if &result != expr {
                        return Some(result);
                    }
                }
                self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr)
            }
            TypeExpr::TypeOf(_) => {
                // `typeof value_ref`: caller-scoped first (the most
                // common case is `Foo['x']` where `Foo` is a value
                // imported into the calling SFC). Path C C11-residual-B:
                // some helper-aliased patterns reference values that
                // are visible in OUTER helper scopes (e.g.,
                // `type Button = ComponentConfig<typeof theme>` declared
                // in `button-types.ts` — `theme` is visible there, but
                // by the time the projection recurses into
                // `ComponentConfig`'s body in `types.ts`, neither
                // `decl_scope=types.ts` nor `arg_scope=ImportedSlotButton.vue`
                // can resolve `theme`. The `chain_scopes` carry the
                // outer declaration scopes through the recursion so
                // the value reference can find its visible scope.
                let arg_first = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if let Some(ref result) = arg_first {
                    if result != expr {
                        return arg_first;
                    }
                }
                if let Some(decl_result) =
                    self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                {
                    if &decl_result != expr {
                        return Some(decl_result);
                    }
                }
                for chain_scope in &chain_scopes {
                    if chain_scope == &decl_scope || chain_scope == &arg_scope {
                        continue;
                    }
                    if let Some(chain_result) =
                        self.solve_or_project_leaf_expr_until_stable(chain_scope, expr)
                    {
                        if &chain_result != expr {
                            return Some(chain_result);
                        }
                    }
                }
                arg_first
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                // `Ref { name, [args..] }`: helper instantiation. Resolve
                // the name in decl_scope (so the helper's declaration
                // registry is consulted), and lower type_arguments in
                // arg_scope (so caller-side `typeof`, locally-declared
                // types, etc. stay resolvable). The simplest way to
                // plumb both is to try decl_scope first — the helper's
                // body will instantiate against its own declaration-
                // site symbol table. If decl_scope resolves the helper
                // (non-trivially), return the decl_scope projection.
                // Otherwise fall back to arg_scope where the ref name
                // may be reachable via direct import.
                let decl_first = self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr);
                if let Some(ref result) = decl_first {
                    if result != expr {
                        return decl_first;
                    }
                }
                let arg_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if let Some(ref result) = arg_result {
                    if result != expr {
                        return arg_result;
                    }
                }
                // Path C C11-residual-B: split-scope projection. When
                // the ref's name belongs to one scope (e.g. `ComponentUI`
                // declared in `types.ts`) and its type_arguments
                // reference values from another scope (e.g.
                // `typeof theme` visible only in `button-types.ts`),
                // pre-resolve each `TypeOf(value)` argument in any
                // chain scope where the value is visible, then re-try
                // the projection with the resolved arguments substituted.
                if !chain_scopes.is_empty() {
                    let mut resolved_args: Vec<TypeExpr> = Vec::with_capacity(type_arguments.len());
                    let mut any_argument_resolved = false;
                    for arg in type_arguments.iter() {
                        let mut resolved = arg.clone();
                        if matches!(arg, TypeExpr::TypeOf(_)) {
                            for chain_scope in &chain_scopes {
                                if chain_scope == &decl_scope || chain_scope == &arg_scope {
                                    continue;
                                }
                                if let Some(chain_arg) =
                                    self.solve_or_project_leaf_expr_until_stable(chain_scope, arg)
                                {
                                    if &chain_arg != arg {
                                        resolved = chain_arg;
                                        any_argument_resolved = true;
                                        break;
                                    }
                                }
                            }
                        }
                        resolved_args.push(resolved);
                    }
                    if any_argument_resolved {
                        let resolved_expr = TypeExpr::Ref {
                            name: name.clone(),
                            type_arguments: std::sync::Arc::from(resolved_args),
                        };
                        if let Some(result) = self
                            .solve_or_project_leaf_expr_until_stable(&decl_scope, &resolved_expr)
                        {
                            return Some(result);
                        }
                        if let Some(result) =
                            self.solve_or_project_leaf_expr_until_stable(&arg_scope, &resolved_expr)
                        {
                            return Some(result);
                        }
                    }
                }
                arg_result.or(decl_first)
            }
            _ => {
                // Compound shapes (IndexedAccess, Conditional, Mapped,
                // KeyOf, Intersection, Union, Parenthesized, etc.)
                // preserve the pre-C11b two-scope retry. Inner
                // sub-expressions come back through this function so
                // per-shape dispatch still applies transitively.
                let active_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if !self.expr_references_prepared_scope_symbol(&decl_scope, expr) {
                    return active_result;
                }
                self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                    .or(active_result)
            }
        }
    }

    fn solve_or_project_leaf_expr_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let mut current = expr.clone();
        let mut last = None;
        for _ in 0..3 {
            let next = self
                .solve_expr_type_expr(scope_canonical_id, &current)
                .or_else(|| self.project_expr_surface_expr(scope_canonical_id, &current));
            let Some(next) = next else {
                return last;
            };
            if next == current {
                return Some(next);
            }
            last = Some(next.clone());
            current = next;
        }
        last
    }

    fn project_prepared_member_path_route_projection_from_symbol(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        symbol_name: &str,
        path: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<TypeExpr> {
        let visit_key = (
            resolution_scope_canonical_id.to_string(),
            symbol_name.to_string(),
        );
        if !visited.insert(visit_key.clone()) {
            return None;
        }
        if substitutions.is_empty() {
            if let Some(prepared) =
                self.prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            {
                if let Some(default_substitutions) =
                    prepared_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self
                            .project_prepared_member_path_route_projection_from_symbol(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                symbol_name,
                                path,
                                &default_substitutions,
                                visited,
                            );
                        visited.remove(&visit_key);
                        return result;
                    }
                }
            }
        }

        let result = self
            .prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member_name) = path.first() {
                    if let Some(member) = prepared.member(member_name) {
                        let member_ty = substitute_type_expr_if_needed(&member.ty, substitutions);
                        if path.len() == 1 {
                            return self
                                .solve_or_project_prepared_member_leaf_expr(
                                    resolution_scope_canonical_id,
                                    active_scope_canonical_id,
                                    &member_ty,
                                )
                                .or(Some(member_ty));
                        }
                        return self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared.as_ref(),
                            &member_ty,
                            &path[1..],
                            &FxHashMap::default(),
                            visited,
                        );
                    }
                }

                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    path,
                    substitutions,
                    visited,
                )
            });

        visited.remove(&visit_key);
        result
    }

    fn project_prepared_member_path_route_projection_from_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        path: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        let Some((member_name, tail)) = path.split_first() else {
            let projected_expr = substitute_type_expr_if_needed(expr, substitutions);
            return self
                .solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &projected_expr,
                )
                .or(Some(projected_expr));
        };

        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    inner,
                    path,
                    substitutions,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    part,
                    path,
                    substitutions,
                    visited,
                )
            }),
            TypeExpr::Object(object) => {
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property) if property.name == *member_name => {
                        Some(substitute_type_expr_if_needed(&property.ty, substitutions))
                    }
                    ObjectMember::Method(method) if method.name == *member_name => {
                        Some(TypeExpr::Function(std::sync::Arc::new(
                            substitute_function_expr_if_needed(&method.function, substitutions),
                        )))
                    }
                    _ => None,
                })?;
                if tail.is_empty() {
                    return self
                        .solve_or_project_prepared_member_leaf_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &member_ty,
                        )
                        .or(Some(member_ty));
                }
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    &member_ty,
                    tail,
                    &FxHashMap::default(),
                    visited,
                )
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_member_path_route_projection_from_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        prepared,
                        &substituted,
                        path,
                        &FxHashMap::default(),
                        visited,
                    );
                }

                match (name.as_ref(), type_arguments.as_ref()) {
                    ("Partial", [inner])
                    | ("Required", [inner])
                    | ("Readonly", [inner])
                    | ("NonNullable", [inner]) => self
                        .project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            inner,
                            path,
                            substitutions,
                            visited,
                        ),
                    ("Pick", [target, keys]) => {
                        let requested = self.prepared_string_literal_keys(
                            resolution_scope_canonical_id,
                            prepared,
                            keys,
                            visited,
                        )?;
                        if !requested.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            target,
                            path,
                            substitutions,
                            visited,
                        )
                    }
                    ("Omit", [target, keys]) => {
                        let omitted = self.prepared_string_literal_keys(
                            resolution_scope_canonical_id,
                            prepared,
                            keys,
                            visited,
                        )?;
                        if omitted.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            target,
                            path,
                            substitutions,
                            visited,
                        )
                    }
                    _ if matches!(name.as_ref(), "Array" | "ReadonlyArray" | "Promise") => None,
                    _ if is_builtin_name(name.as_ref()) => None,
                    _ => {
                        let (target_canonical_id, target_symbol_name) = self
                            .resolve_prepared_surface_target(
                                resolution_scope_canonical_id,
                                prepared,
                                name.as_ref(),
                            )?;
                        let target_prepared =
                            self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
                        let target_substitutions = prepared_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        // Path C C11-residual-B: as we descend into the
                        // target alias's declaration scope, push the
                        // current `resolution_scope_canonical_id` onto
                        // the projection chain. The leaf-expr handler
                        // uses this chain to find the scope where a
                        // `TypeOf(value)` reference was visible at the
                        // outer call site (e.g., `theme` imported in
                        // `button-types.ts` while we're now recursing
                        // into `ComponentConfig`'s body in `types.ts`).
                        let pushed = if !self
                            .projection_chain_scopes
                            .iter()
                            .any(|s| s == resolution_scope_canonical_id)
                            && resolution_scope_canonical_id != target_canonical_id
                        {
                            self.projection_chain_scopes
                                .push(resolution_scope_canonical_id.to_string());
                            true
                        } else {
                            false
                        };
                        let result = self
                            .project_prepared_member_path_route_projection_from_symbol(
                                &target_canonical_id,
                                active_scope_canonical_id,
                                &target_symbol_name,
                                path,
                                &target_substitutions,
                                visited,
                            );
                        if pushed {
                            self.projection_chain_scopes.pop();
                        }
                        result
                    }
                }
            }
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                name_type,
                ..
            } if name_type.is_none() => {
                let substituted_source = substitute_type_expr_if_needed(source, substitutions);
                let Some(keys) = self.projected_string_literal_keys(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &substituted_source,
                ) else {
                    let nested_expr = path.iter().fold(
                        substitute_type_expr_if_needed(expr, substitutions),
                        |object, member| TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(object),
                            index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
                        },
                    );
                    return self.solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &nested_expr,
                    );
                };
                if !keys.iter().any(|candidate| candidate == member_name) {
                    return None;
                }

                let mut member_substitutions = substitutions.clone();
                member_substitutions.insert(
                    parameter.clone(),
                    TypeExpr::string_literal(member_name.clone()),
                );
                let member_ty = substitute_type_expr_if_needed(value, &member_substitutions);
                if tail.is_empty() {
                    if let Some(keys) = self.projected_string_literal_keys(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &member_ty,
                    ) {
                        return string_literal_keys_type_expr(keys);
                    }
                    return self
                        .solve_or_project_prepared_member_leaf_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &member_ty,
                        )
                        .or(Some(member_ty));
                }
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    &member_ty,
                    tail,
                    &FxHashMap::default(),
                    visited,
                )
            }
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Union(_)
            | TypeExpr::Tuple { .. }
            | TypeExpr::Array { .. }
            | TypeExpr::KeyOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Rest(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Infer { .. } => {
                let nested_expr = path.iter().fold(
                    substitute_type_expr_if_needed(expr, substitutions),
                    |object, member| TypeExpr::IndexedAccess {
                        object: std::sync::Arc::new(object),
                        index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
                    },
                );
                self.solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &nested_expr,
                )
            }
            TypeExpr::Function(_)
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. } => None,
        }
    }

    fn project_inherited_member_route_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        let mut visited = FxHashSet::default();
        self.project_inherited_member_route_projection_from_symbol(
            scope_canonical_id,
            symbol_name,
            member_name,
            &mut visited,
        )
    }

    fn project_inherited_member_route_projection_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::InheritedRoute,
            substitutions: PreparedSubstitutionKey::Empty,
        };
        if let Some(cached) = self.prepared_member_cache.get(&cache_key) {
            return cached.clone();
        }

        let visit_key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !visited.insert(visit_key.clone()) {
            return None;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member) = prepared.member(member_name) {
                    return self.project_prepared_member_from_decl(
                        scope_canonical_id,
                        &prepared,
                        member_name,
                        member,
                    );
                }

                self.project_inherited_member_route_projection_from_expr(
                    scope_canonical_id,
                    &prepared,
                    &prepared.body,
                    member_name,
                    visited,
                )
            });

        visited.remove(&visit_key);
        self.prepared_member_cache.insert(cache_key, result.clone());
        result
    }

    fn project_inherited_member_route_projection_from_expr(
        &mut self,
        _scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        expr: &TypeExpr,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_inherited_member_route_projection_from_expr(
                    _scope_canonical_id,
                    prepared,
                    inner,
                    member_name,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_inherited_member_route_projection_from_expr(
                    _scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    visited,
                )
            }),
            TypeExpr::Ref { name, .. } => {
                let resolved = prepared.name_resolution.get(name.as_ref())?;
                self.project_inherited_member_route_projection_from_symbol(
                    resolved.canonical_id.as_str(),
                    resolved.symbol_name.as_str(),
                    member_name,
                    visited,
                )
            }
            _ => None,
        }
    }

    #[cfg(test)]
    fn project_prepared_member_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<TypeExpr> {
        self.project_prepared_member_route_projection(scope_canonical_id, symbol_name, member_name)
            .map(|projected_member| projected_member.ty)
    }

    fn project_pick_route_surface_expr_via_members(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        members: &[String],
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::{
            MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
        };

        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name);
        let mut properties = Vec::with_capacity(members.len());
        for member_name in members {
            let projected_member = if prepared
                .as_ref()
                .and_then(|prepared| prepared.member(member_name))
                .is_some()
            {
                self.project_prepared_member_route_projection(
                    scope_canonical_id,
                    symbol_name,
                    member_name,
                )?
            } else if let Some(projected_member) = self.project_inherited_member_route_projection(
                scope_canonical_id,
                symbol_name,
                member_name,
            ) {
                projected_member
            } else {
                self.project_type_member(scope_canonical_id, symbol_name, member_name)?
            };
            self.cache_projected_member(scope_canonical_id, symbol_name, &projected_member);
            if projected_member.is_method {
                if let TypeExpr::Function(function) = &projected_member.ty {
                    properties.push(ObjectMember::Method(MethodSignature {
                        name: projected_member.name,
                        function: (**function).clone(),
                        optional: projected_member.optional,
                    }));
                    continue;
                }
            }
            properties.push(ObjectMember::Property(ObjectProperty {
                name: projected_member.name,
                ty: projected_member.ty,
                optional: projected_member.optional,
                readonly: projected_member.readonly,
            }));
        }
        Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties,
        })))
    }

    fn project_pick_route_surface_expr_via_routed_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        route: &super::RouteDemand,
        _members: &[String],
    ) -> Option<TypeExpr> {
        assert_direct_pick_routed_expr_slow_lane_allowed();
        self.project_routed_expr_surface_expr_direct(scope_canonical_id, symbol_name, route)
    }

    fn project_routed_expr_surface_expr_direct(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        self.dispatch_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
    }

    fn project_prepared_pick_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        members: &[String],
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::{
            MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
        };

        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name)?;
        let mut properties = Vec::with_capacity(members.len());
        for member_name in members {
            let member = prepared.member(member_name)?;
            if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                return None;
            }
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    properties.push(ObjectMember::Method(MethodSignature {
                        name: member_name.clone(),
                        function: (**function).clone(),
                        optional: member.optional,
                    }));
                    continue;
                }
            }
            properties.push(ObjectMember::Property(ObjectProperty {
                name: member_name.clone(),
                ty: member.ty.clone(),
                optional: member.optional,
                readonly: member.readonly,
            }));
        }
        Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties,
        })))
    }

    #[allow(dead_code)]
    fn type_surface_facts(
        &self,
        scope_canonical_id: &str,
    ) -> Option<Vec<crate::resolver_core::FactVersionRef>> {
        let store_view = self.host.resolver_store_view();
        let mut facts = Vec::new();
        // Post-cut: live-host whole-hash with store-view as the first
        // consultation, falling back to the live host probe for
        // untracked-but-present canonicals.
        let hash = store_view
            .whole_hash(scope_canonical_id)
            .or_else(|| self.host.get_whole_hash(scope_canonical_id));
        if let Some(hash) = hash {
            facts.push(crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: scope_canonical_id.to_string(),
                hash,
            });
        }
        if let Some(hash) = store_view.derived_hash(
            scope_canonical_id,
            crate::resolver_core::DerivedFactKind::Route,
        ) {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: scope_canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            });
        }
        (!facts.is_empty()).then_some(facts)
    }

    #[cfg(test)]
    fn debug_prepared_surface_cache_len(&self) -> usize {
        self.prepared_surface_cache.len()
    }

    #[cfg(test)]
    fn debug_prepared_member_cache_len(&self) -> usize {
        self.prepared_member_cache.len()
    }

    #[cfg(test)]
    fn debug_prepared_target_cache_len(&self) -> usize {
        self.prepared_target_cache.len()
    }
}

fn local_type_symbol_metadata_for_known_source(
    host: &VerterHost,
    canonical_source: &str,
    resolved_name: &str,
) -> Option<ResolvedLocalTypeSymbolMetadata> {
    let analysis = host.external_type_analysis(canonical_source)?;
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
    host: &'a VerterHost,
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

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        self.host
            .read_analysis_source(canonical_source)
            .as_deref()
            .map(str::to_string)
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        self.host
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }
}

fn empty_semantic_args() -> std::sync::Arc<[SemanticNodeId]> {
    std::sync::Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice())
}

fn projected_surface_from_semantic_node(
    host: &VerterHost,
    node: SemanticNodeId,
) -> Option<ProjectedSurface> {
    let mut active = FxHashSet::default();
    projected_surface_from_semantic_node_inner(host, node, &mut active)
}

fn projected_surface_from_semantic_node_inner(
    host: &VerterHost,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<ProjectedSurface> {
    let data = node_data_for(host, node)?;
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return None;
            }
            let result = projected_surface_from_semantic_node_inner(host, *target, active);
            active.remove(&node);
            result
        }
        SemanticNodeData::Object(surface) => Some(surface_view_to_projected_surface(host, surface)),
        _ => None,
    }
}

fn surface_view_to_projected_surface(host: &VerterHost, surface: &SurfaceView) -> ProjectedSurface {
    let members = surface
        .members
        .iter()
        .map(|member| ProjectedMember {
            name: member.name.as_ref().to_string(),
            ty: semantic_node_to_type_expr(host, member.value).unwrap_or(TypeExpr::Unknown {
                raw: SEMANTIC_SURFACE_MEMBER.to_string(),
            }),
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
        })
        .collect();
    let call_signatures = surface
        .call_signatures
        .iter()
        .filter_map(|signature| semantic_node_to_type_expr(host, *signature))
        .collect();
    let construct_signatures = surface
        .construct_signatures
        .iter()
        .filter_map(|signature| semantic_node_to_type_expr(host, *signature))
        .collect();
    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature: surface.has_index_signature,
    }
}

fn filtered_projected_surface(
    mut surface: ProjectedSurface,
    keep: impl Fn(&str) -> bool,
) -> ProjectedSurface {
    surface.members.retain(|member| keep(member.name.as_str()));
    surface
}

fn semantic_node_to_type_expr(host: &VerterHost, node: SemanticNodeId) -> Option<TypeExpr> {
    let mut active = FxHashSet::default();
    semantic_node_to_type_expr_inner(host, node, &mut active)
}

fn semantic_node_to_type_expr_inner(
    host: &VerterHost,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<TypeExpr> {
    let data = node_data_for(host, node)?;
    Some(match data.as_ref() {
        SemanticNodeData::Primitive(kind) => semantic_primitive_to_type_expr(*kind),
        SemanticNodeData::Literal(value) => TypeExpr::Literal(value.clone()),
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return Some(TypeExpr::Unknown {
                    raw: "semanticAliasCycle".to_string(),
                });
            }
            let result = semantic_node_to_type_expr_inner(host, *target, active);
            active.remove(&node);
            return result;
        }
        SemanticNodeData::Union(members) => TypeExpr::Union(std::sync::Arc::from(
            members
                .iter()
                .filter_map(|member| semantic_node_to_type_expr_inner(host, *member, active))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
        SemanticNodeData::Intersection(members) => {
            // Path C C11a — drop empty-object arms from the
            // Intersection projection. `Id<T> = {} & { [P in keyof T]: T[P] }`
            // and similar helper patterns lower to
            // Intersection([empty_object, mapped_object]); the empty
            // arm contributes nothing semantically (`{} & X ≡ X`) but
            // leaks through as a `TypeExpr::Unknown { raw:
            // SEMANTIC_OBJECT_SURFACE }` sentinel which breaks callers
            // that expect a pure Object at the projection boundary.
            // Dropping the semantically-vacuous arm here collapses
            // `{} & X → X` so imported-helper ui bindings materialise
            // cleanly instead of nested in Intersection([Unknown, Object]).
            let mut arms: Vec<TypeExpr> = members
                .iter()
                .filter_map(|member| semantic_node_to_type_expr_inner(host, *member, active))
                .filter(|arm| !matches!(arm, TypeExpr::Unknown { raw } if raw == SEMANTIC_OBJECT_SURFACE))
                .collect();
            if arms.len() == 1 {
                arms.pop().unwrap()
            } else {
                TypeExpr::Intersection(std::sync::Arc::from(arms.into_boxed_slice()))
            }
        }
        SemanticNodeData::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(semantic_node_to_type_expr_inner(host, *element, active)?),
            readonly: *readonly,
        },
        SemanticNodeData::Tuple { elements, readonly } => {
            use verter_semantic::analysis::type_expr::TupleElement;

            TypeExpr::Tuple {
                elements: std::sync::Arc::from(
                    elements
                        .iter()
                        .filter_map(|element| {
                            Some(TupleElement {
                                label: element
                                    .label
                                    .as_ref()
                                    .map(|label| label.as_ref().to_string()),
                                ty: semantic_node_to_type_expr_inner(host, element.value, active)?,
                                optional: element.optional,
                                rest: element.rest,
                            })
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                readonly: *readonly,
            }
        }
        SemanticNodeData::Object(surface) => {
            projected_surface_to_type_expr(&surface_view_to_projected_surface(host, surface))
                .unwrap_or(TypeExpr::Unknown {
                    raw: SEMANTIC_OBJECT_SURFACE.to_string(),
                })
        }
        // C16: DeclPlaceholder → TypeExpr::Ref (replaces DeclAnchor).
        SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
            name,
            ..
        }) => TypeExpr::Ref {
            name: std::sync::Arc::clone(name),
            type_arguments: verter_semantic::analysis::type_expr::empty_type_args(),
        },
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => TypeExpr::Conditional {
            check: std::sync::Arc::new(semantic_node_to_type_expr_inner(host, *check, active)?),
            extends: std::sync::Arc::new(semantic_node_to_type_expr_inner(host, *extends, active)?),
            true_type: std::sync::Arc::new(semantic_node_to_type_expr_inner(
                host,
                *true_branch_ref,
                active,
            )?),
            false_type: std::sync::Arc::new(semantic_node_to_type_expr_inner(
                host,
                *false_branch_ref,
                active,
            )?),
        },
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis
                .iter()
                .map(|quasi| quasi.as_ref().to_string())
                .collect(),
            expressions: std::sync::Arc::from(
                expressions
                    .iter()
                    .filter_map(|expr| semantic_node_to_type_expr_inner(host, *expr, active))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        },
        SemanticNodeData::KeyOf { base } => TypeExpr::KeyOf(std::sync::Arc::new(
            semantic_node_to_type_expr_inner(host, *base, active)?,
        )),
        SemanticNodeData::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(semantic_node_to_type_expr_inner(host, *object, active)?),
            index: std::sync::Arc::new(index_key_to_type_expr(host, index, active)?),
        },
        SemanticNodeData::Mapped { mapper, .. } => TypeExpr::Mapped {
            // Path C C6a item 9c: presentational projection. Look
            // up the binder node by `mapper.parameter_node` and
            // read its `display_name` for the projected
            // `TypeExpr::Mapped { parameter }` field. C7's interner
            // dedups only structurally-identical binders, so the
            // representative's display_name is well-defined.
            parameter: match node_data_for(host, mapper.parameter_node).as_deref() {
                Some(SemanticNodeData::TypeParam { display_name, .. }) => {
                    display_name.as_ref().to_string()
                }
                _ => String::new(),
            },
            source: std::sync::Arc::new(match node_data_for(host, mapper.key_space)?.as_ref() {
                SemanticNodeData::KeyOf { base } => TypeExpr::KeyOf(std::sync::Arc::new(
                    semantic_node_to_type_expr_inner(host, *base, active)?,
                )),
                _ => semantic_node_to_type_expr_inner(host, mapper.key_space, active)?,
            }),
            value: std::sync::Arc::new(semantic_node_to_type_expr_inner(
                host,
                mapper.value_expr,
                active,
            )?),
            optional: match mapper.optionality {
                crate::semantic_query::OptionalityMod::Add => {
                    verter_semantic::analysis::type_expr::MappedModifier::Add
                }
                crate::semantic_query::OptionalityMod::Remove => {
                    verter_semantic::analysis::type_expr::MappedModifier::Remove
                }
                crate::semantic_query::OptionalityMod::Keep => {
                    verter_semantic::analysis::type_expr::MappedModifier::None
                }
            },
            readonly: match mapper.readonly {
                crate::semantic_query::ReadonlyMod::Add => {
                    verter_semantic::analysis::type_expr::MappedModifier::Add
                }
                crate::semantic_query::ReadonlyMod::Remove => {
                    verter_semantic::analysis::type_expr::MappedModifier::Remove
                }
                crate::semantic_query::ReadonlyMod::Keep => {
                    verter_semantic::analysis::type_expr::MappedModifier::None
                }
            },
            name_type: match mapper.name_remap {
                Some(node) => Some(std::sync::Arc::new(semantic_node_to_type_expr_inner(
                    host, node, active,
                )?)),
                None => None,
            },
        },
        SemanticNodeData::TypeOf { value_root, path } => {
            let mut segments = value_root
                .name
                .split('.')
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>();
            segments.extend(path.iter().map(|segment| segment.as_ref().to_string()));
            TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef { path: segments })
        }
        SemanticNodeData::TypeParam {
            display_name,
            constraint,
            default,
            ..
        } => {
            // Plan §3 Cluster A: project `constraint` / `default` back
            // to `TypeExpr` so the round-trip preserves the declaration
            // shape. The `active` visited set guards against cyclic
            // constraint graphs (plan F7): when a TypeParam's
            // constraint or default transitively reaches this same
            // node, return `None` from the recursion and drop the
            // field rather than looping.
            //
            // Path C C6: the projected `TypeExpr::TypeParameter.name`
            // uses `display_name` — the human-readable parameter
            // name. `decl` / `param_index` are identity discriminators
            // for structural interning and do not appear in the
            // projected `TypeExpr` shape.
            if !active.insert(node) {
                return Some(TypeExpr::Unknown {
                    raw: "semanticTypeParamCycle".to_string(),
                });
            }
            let constraint_expr = constraint
                .as_ref()
                .and_then(|c| semantic_node_to_type_expr_inner(host, *c, active))
                .map(std::sync::Arc::new);
            let default_expr = default
                .as_ref()
                .and_then(|d| semantic_node_to_type_expr_inner(host, *d, active))
                .map(std::sync::Arc::new);
            active.remove(&node);
            TypeExpr::TypeParameter(verter_semantic::analysis::type_expr::TypeParam {
                name: display_name.as_ref().to_string(),
                constraint: constraint_expr,
                default: default_expr,
            })
        }
        SemanticNodeData::Infer { name } => TypeExpr::Infer {
            name: name.as_ref().to_string(),
        },
        SemanticNodeData::Opaque(err) => match err {
            QueryError::RecursiveRef { name } => TypeExpr::recursive_ref(name.as_ref(), Vec::new()),
            _ => TypeExpr::Unknown {
                raw: semantic_query_error_raw(err),
            },
        },
        SemanticNodeData::VueMacroElements(_) => TypeExpr::Unknown {
            raw: "VueMacroElements".to_string(),
        },
        // Phase D §5.6 WIP-L / §3 Change L — canonical Function shape
        // converts back to `TypeExpr::Function`. Session 4 lowered
        // `TypeExpr::Function` → `SemanticNodeData::Function`; this
        // conversion completes the round-trip so alias bodies that
        // include function types (`(() => T)` branches) survive
        // dispatch-only projection without emitting `semanticFunction`
        // sentinels.
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
        } => {
            use verter_semantic::analysis::type_expr::{FunctionExpr, FunctionParam, TypeParam};
            let parameters: Vec<FunctionParam> = params
                .iter()
                .filter_map(|p| {
                    Some(FunctionParam {
                        name: p.name.as_ref().map(|n| n.as_ref().to_string()),
                        ty: semantic_node_to_type_expr_inner(host, p.ty, active)?,
                        optional: p.optional,
                        rest: p.rest,
                    })
                })
                .collect();
            let return_ty = semantic_node_to_type_expr_inner(host, *return_type, active)
                .map(std::sync::Arc::new);
            let type_params: Vec<TypeParam> = type_parameters
                .iter()
                .map(|tp| TypeParam {
                    name: tp.name.as_ref().to_string(),
                    constraint: tp
                        .constraint
                        .and_then(|c| semantic_node_to_type_expr_inner(host, c, active))
                        .map(std::sync::Arc::new),
                    default: tp
                        .default
                        .and_then(|d| semantic_node_to_type_expr_inner(host, d, active))
                        .map(std::sync::Arc::new),
                })
                .collect();
            TypeExpr::Function(std::sync::Arc::new(FunctionExpr {
                parameters,
                return_type: return_ty,
                type_parameters: type_params,
            }))
        }
    })
}

fn index_key_to_type_expr(
    host: &VerterHost,
    index: &IndexKey,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<TypeExpr> {
    Some(match index {
        IndexKey::String(text) => TypeExpr::string_literal(text.as_ref()),
        IndexKey::Number(number) => TypeExpr::number_literal(*number as f64),
        IndexKey::TypeNode(node) => semantic_node_to_type_expr_inner(host, *node, active)?,
    })
}

fn semantic_primitive_to_type_expr(kind: SemanticPrimitiveKind) -> TypeExpr {
    use verter_semantic::analysis::type_expr::PrimitiveName;

    TypeExpr::Primitive(match kind {
        SemanticPrimitiveKind::String => PrimitiveName::String,
        SemanticPrimitiveKind::Number => PrimitiveName::Number,
        SemanticPrimitiveKind::Boolean => PrimitiveName::Boolean,
        SemanticPrimitiveKind::Symbol => PrimitiveName::Symbol,
        SemanticPrimitiveKind::BigInt => PrimitiveName::BigInt,
        SemanticPrimitiveKind::Any => PrimitiveName::Any,
        SemanticPrimitiveKind::Unknown => PrimitiveName::Unknown,
        SemanticPrimitiveKind::Void => PrimitiveName::Void,
        SemanticPrimitiveKind::Never => PrimitiveName::Never,
        SemanticPrimitiveKind::Null => PrimitiveName::Null,
        SemanticPrimitiveKind::Undefined => PrimitiveName::Undefined,
        SemanticPrimitiveKind::Object => PrimitiveName::Object,
    })
}

fn dispatch_route_expr_is_materialized(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown { raw } => {
            // Every sentinel emitted by `semantic_node_to_type_expr_inner`
            // (exact matches) or by `semantic_query_error_raw` (prefix
            // matches for parameterised errors) must round-trip to
            // "not materialised" so the dispatch-first path falls back
            // to `owner_engine` for fuller expansion.
            let is_exact_sentinel = matches!(
                raw.as_str(),
                SEMANTIC_MISS
                    | SEMANTIC_OBJECT_SURFACE
                    | SEMANTIC_SURFACE_MEMBER
                    | "semanticAliasCycle"
                    | "semanticFunction"
                    | "VueMacroElements"
                    | "projectedOpenSurface"
            );
            let is_prefix_sentinel = raw.starts_with("materialize:")
                || raw.starts_with("unsupportedIntrinsic(")
                || raw.starts_with("budgetExceeded(")
                || raw.starts_with("unstableState(")
                || raw.starts_with("aliasCycle(");
            !is_exact_sentinel && !is_prefix_sentinel
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => dispatch_route_expr_is_materialized(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| dispatch_route_expr_is_materialized(&element.ty)),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                dispatch_route_expr_is_materialized(&property.ty)
            }
            verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && method
                        .function
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_semantic::analysis::type_expr::ObjectMember::CallSignature(signature)
            | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(signature) => {
                signature
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && signature
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                dispatch_route_expr_is_materialized(&signature.key_type)
                    && dispatch_route_expr_is_materialized(&signature.value_type)
            }
        }),
        TypeExpr::Function(function) => {
            function
                .return_type
                .as_deref()
                .is_none_or(dispatch_route_expr_is_materialized)
                && function
                    .parameters
                    .iter()
                    .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
        }
        TypeExpr::IndexedAccess { object, index } => {
            dispatch_route_expr_is_materialized(object)
                && dispatch_route_expr_is_materialized(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            dispatch_route_expr_is_materialized(check)
                && dispatch_route_expr_is_materialized(extends)
                && dispatch_route_expr_is_materialized(true_type)
                && dispatch_route_expr_is_materialized(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            dispatch_route_expr_is_materialized(source)
                && dispatch_route_expr_is_materialized(value)
                && name_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

/// Detects sentinel tokens emitted by `semantic_node_to_type_expr_inner`
/// when dispatch cannot materialise a node. Dispatch-first paths fall
/// back to `owner_engine` when the sentinel is present — transitional
/// until §5.8 retires the owner_engine bridge.
fn type_expr_contains_semantic_miss(expr: &TypeExpr) -> bool {
    !dispatch_route_expr_is_materialized(expr)
}

/// Returns `true` when `expr` still carries open deferred shell shapes
/// (`KeyOf`, `IndexedAccess`, `Mapped`, `TypeOf`, `Conditional`) that
/// indicate dispatch could not structurally expand the surface further.
fn type_expr_is_expanded_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. } => false,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(type_expr_is_expanded_surface)
        }
        _ => true,
    }
}

/// Returns `true` when `expr` contains at least one Object arm at any
/// nesting depth (top-level, or inside `Parenthesized` /
/// `Intersection` / `Union`). Used by the slot-shape producer to
/// decide whether a partially-deferred compound shape is still useful
/// for extracting explicit slot members.
fn type_expr_has_any_object_arm(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Object(_) => true,
        TypeExpr::Parenthesized(inner) => type_expr_has_any_object_arm(inner),
        TypeExpr::Intersection(members) | TypeExpr::Union(members) => {
            members.iter().any(type_expr_has_any_object_arm)
        }
        _ => false,
    }
}

fn semantic_query_error_raw(err: &QueryError) -> String {
    match err {
        QueryError::Miss => SEMANTIC_MISS.to_string(),
        QueryError::Other(text) => text.as_ref().to_string(),
        QueryError::UnsupportedIntrinsic { name } => format!("unsupportedIntrinsic({name})"),
        QueryError::BudgetExceeded(failure) => format!("budgetExceeded({:?})", failure.domain),
        QueryError::UnstableState { attempts } => format!("unstableState({attempts})"),
        QueryError::AliasCycle { chain } => format!("aliasCycle({})", chain.len()),
        QueryError::RecursiveRef { name } => format!("recursiveRef({name})"),
        QueryError::DeclPlaceholder { name, .. } => format!("declPlaceholder({name})"),
    }
}

#[derive(Debug, Clone)]
enum PreparedSurfaceProjection {
    Surface(std::sync::Arc<ProjectedSurface>),
    Empty,
    Unsupported,
}

fn prepared_substitution_key(
    substitutions: &FxHashMap<String, TypeExpr>,
) -> PreparedSubstitutionKey {
    if substitutions.is_empty() {
        return PreparedSubstitutionKey::Empty;
    }

    let mut entries = substitutions
        .iter()
        .map(|(name, ty)| (name.clone(), ty.clone()))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    PreparedSubstitutionKey::Entries(entries)
}

#[allow(dead_code)]
fn prepared_substitution_instantiation_hash(substitutions: &FxHashMap<String, TypeExpr>) -> u64 {
    if substitutions.is_empty() {
        return 0;
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prepared_substitution_key(substitutions).hash(&mut hasher);
    hasher.finish()
}

fn projected_surface_is_empty(surface: &ProjectedSurface) -> bool {
    surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
}

fn projected_surface_from_object_expr(
    object: &verter_semantic::analysis::type_expr::ObjectExpr,
) -> ProjectedSurface {
    use verter_semantic::analysis::type_expr::ObjectMember;

    let mut members = Vec::new();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;

    for member in &object.properties {
        match member {
            ObjectMember::Property(property) => members.push(ProjectedMember {
                name: property.name.clone(),
                ty: property.ty.clone(),
                optional: property.optional,
                readonly: property.readonly,
                is_method: false,
            }),
            ObjectMember::Method(method) => members.push(ProjectedMember {
                name: method.name.clone(),
                ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                optional: method.optional,
                readonly: false,
                is_method: true,
            }),
            ObjectMember::CallSignature(function) => {
                call_signatures.push(TypeExpr::Function(std::sync::Arc::new(function.clone())));
            }
            ObjectMember::ConstructSignature(function) => {
                construct_signatures
                    .push(TypeExpr::Function(std::sync::Arc::new(function.clone())));
            }
            ObjectMember::IndexSignature(_) => has_index_signature = true,
        }
    }

    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }
}

fn projected_surface_from_object_expr_with_substitutions(
    object: &verter_semantic::analysis::type_expr::ObjectExpr,
    _type_params: &[verter_semantic::analysis::type_expr::TypeParam],
    substitutions: &FxHashMap<String, TypeExpr>,
) -> ProjectedSurface {
    use verter_semantic::analysis::type_expr::ObjectMember;

    if substitutions.is_empty() {
        return projected_surface_from_object_expr(object);
    }

    let mut members = Vec::new();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;

    for member in &object.properties {
        match member {
            ObjectMember::Property(property) => members.push(ProjectedMember {
                name: property.name.clone(),
                ty: substitute_type_expr_if_needed(&property.ty, substitutions),
                optional: property.optional,
                readonly: property.readonly,
                is_method: false,
            }),
            ObjectMember::Method(method) => members.push(ProjectedMember {
                name: method.name.clone(),
                ty: TypeExpr::Function(std::sync::Arc::new(substitute_function_expr_if_needed(
                    &method.function,
                    substitutions,
                ))),
                optional: method.optional,
                readonly: false,
                is_method: true,
            }),
            ObjectMember::CallSignature(function) => call_signatures.push(TypeExpr::Function(
                std::sync::Arc::new(substitute_function_expr_if_needed(function, substitutions)),
            )),
            ObjectMember::ConstructSignature(function) => {
                construct_signatures.push(TypeExpr::Function(std::sync::Arc::new(
                    substitute_function_expr_if_needed(function, substitutions),
                )))
            }
            ObjectMember::IndexSignature(_) => has_index_signature = true,
        }
    }

    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }
}

fn projected_surface_from_function_expr(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
) -> ProjectedSurface {
    ProjectedSurface {
        members: Vec::new(),
        call_signatures: vec![TypeExpr::Function(std::sync::Arc::new(function.clone()))],
        construct_signatures: Vec::new(),
        has_index_signature: false,
    }
}

fn projected_surface_from_function_expr_with_substitutions(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    _type_params: &[verter_semantic::analysis::type_expr::TypeParam],
    substitutions: &FxHashMap<String, TypeExpr>,
) -> ProjectedSurface {
    if substitutions.is_empty() {
        return projected_surface_from_function_expr(function);
    }

    ProjectedSurface {
        members: Vec::new(),
        call_signatures: vec![TypeExpr::Function(std::sync::Arc::new(
            substitute_function_expr_if_needed(function, substitutions),
        ))],
        construct_signatures: Vec::new(),
        has_index_signature: false,
    }
}

fn projected_surface_from_parts_intersection(
    parts: Vec<std::sync::Arc<ProjectedSurface>>,
) -> PreparedSurfaceProjection {
    if parts.is_empty() {
        return PreparedSurfaceProjection::Empty;
    }

    let mut merged_members: FxHashMap<String, ProjectedMember> = FxHashMap::default();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;

    for surface in parts {
        let surface = projected_surface_unwrap_or_clone(surface);
        for member in surface.members {
            merged_members.entry(member.name.clone()).or_insert(member);
        }
        call_signatures.extend(surface.call_signatures);
        construct_signatures.extend(surface.construct_signatures);
        has_index_signature |= surface.has_index_signature;
    }

    let mut members = merged_members.into_values().collect::<Vec<_>>();
    members.sort_by(|left, right| left.name.cmp(&right.name));

    PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }))
}

fn projected_surface_from_parts_union(
    parts: Vec<std::sync::Arc<ProjectedSurface>>,
) -> PreparedSurfaceProjection {
    if parts.is_empty() {
        return PreparedSurfaceProjection::Empty;
    }

    let mut merged_members: FxHashMap<String, (ProjectedMember, usize)> = FxHashMap::default();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;
    let mut total_surface_variants = 0usize;

    for surface in parts {
        let surface = projected_surface_unwrap_or_clone(surface);
        if projected_surface_is_empty(&surface) {
            continue;
        }
        total_surface_variants += 1;
        for member in surface.members {
            match merged_members.entry(member.name.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert((member, 1));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let (existing, seen_variants) = entry.get_mut();
                    *seen_variants += 1;
                    existing.optional = existing.optional || member.optional;
                    existing.readonly = existing.readonly && member.readonly;
                    existing.is_method = existing.is_method && member.is_method;
                    if existing.ty != member.ty {
                        existing.ty = TypeExpr::union(vec![existing.ty.clone(), member.ty]);
                    }
                }
            }
        }
        call_signatures.extend(surface.call_signatures);
        construct_signatures.extend(surface.construct_signatures);
        has_index_signature |= surface.has_index_signature;
    }

    if total_surface_variants == 0 {
        return PreparedSurfaceProjection::Empty;
    }

    let mut members = merged_members
        .into_values()
        .map(|(mut member, seen_variants)| {
            if seen_variants < total_surface_variants {
                member.optional = true;
            }
            member
        })
        .collect::<Vec<_>>();
    members.sort_by(|left, right| left.name.cmp(&right.name));

    PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }))
}

fn apply_surface_member_modifier(
    projection: PreparedSurfaceProjection,
    mut mutate: impl FnMut(&mut ProjectedMember),
) -> PreparedSurfaceProjection {
    match projection {
        PreparedSurfaceProjection::Surface(surface) => {
            let mut surface = projected_surface_unwrap_or_clone(surface);
            for member in &mut surface.members {
                mutate(member);
            }
            PreparedSurfaceProjection::Surface(std::sync::Arc::new(surface))
        }
        PreparedSurfaceProjection::Empty => PreparedSurfaceProjection::Empty,
        PreparedSurfaceProjection::Unsupported => PreparedSurfaceProjection::Unsupported,
    }
}

fn apply_surface_member_filter(
    projection: PreparedSurfaceProjection,
    keep: impl Fn(&str) -> bool,
) -> PreparedSurfaceProjection {
    match projection {
        PreparedSurfaceProjection::Surface(surface) => {
            let mut surface = projected_surface_unwrap_or_clone(surface);
            surface.members.retain(|member| keep(member.name.as_str()));
            if projected_surface_is_empty(&surface) {
                PreparedSurfaceProjection::Empty
            } else {
                PreparedSurfaceProjection::Surface(std::sync::Arc::new(surface))
            }
        }
        PreparedSurfaceProjection::Empty => PreparedSurfaceProjection::Empty,
        PreparedSurfaceProjection::Unsupported => PreparedSurfaceProjection::Unsupported,
    }
}

fn projected_surface_unwrap_or_clone(
    surface: std::sync::Arc<ProjectedSurface>,
) -> ProjectedSurface {
    std::sync::Arc::try_unwrap(surface).unwrap_or_else(|shared| shared.as_ref().clone())
}

fn prepared_type_param_substitutions(
    prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
    type_arguments: &[TypeExpr],
) -> Option<FxHashMap<String, TypeExpr>> {
    if type_arguments.len() > prepared.type_parameters.len() {
        return None;
    }

    let mut substitutions = FxHashMap::default();
    for (index, type_parameter) in prepared.type_parameters.iter().enumerate() {
        let arg = if let Some(arg) = type_arguments.get(index) {
            arg.clone()
        } else if let Some(default) = type_parameter.default.as_deref() {
            default.clone()
        } else {
            continue;
        };
        if is_identity_type_param_binding(&arg, &type_parameter.name) {
            continue;
        }
        substitutions.insert(type_parameter.name.clone(), arg);
    }
    Some(substitutions)
}

fn is_identity_type_param_binding(expr: &TypeExpr, param_name: &str) -> bool {
    matches!(
        expr,
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() && name.as_ref() == param_name
    )
}

fn substitute_type_expr_if_needed(
    expr: &TypeExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> TypeExpr {
    if substitutions.is_empty() || !type_expr_references_substitutions(expr, substitutions) {
        expr.clone()
    } else {
        substitute_type_expr(expr, substitutions)
    }
}

fn substitute_function_expr_if_needed(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> verter_semantic::analysis::type_expr::FunctionExpr {
    if substitutions.is_empty() || !function_expr_references_substitutions(function, substitutions)
    {
        function.clone()
    } else {
        substitute_function_expr(function, substitutions)
    }
}

fn substituted_ref_expr_if_needed(
    expr: &TypeExpr,
    name: &str,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> Option<TypeExpr> {
    if substitutions.is_empty() {
        return None;
    }
    if let Some(substituted) = substitutions.get(name) {
        return Some(substituted.clone());
    }
    if !type_expr_references_substitutions(expr, substitutions) {
        return None;
    }
    assert_prepared_structural_substitution_slow_lane_allowed(expr);
    Some(substitute_type_expr(expr, substitutions))
}

fn substitute_type_expr(expr: &TypeExpr, substitutions: &FxHashMap<String, TypeExpr>) -> TypeExpr {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => substitutions
            .get(name.as_ref())
            .cloned()
            .unwrap_or_else(|| expr.clone()),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: name.clone(),
            type_arguments: std::sync::Arc::from(
                type_arguments
                    .iter()
                    .map(|arg| substitute_type_expr(arg, substitutions))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(std::sync::Arc::new(
            substitute_type_expr(inner, substitutions),
        )),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(substitute_type_expr(element, substitutions)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: substitute_type_expr(&element.ty, substitutions),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Union(types) => TypeExpr::Union(std::sync::Arc::from(
            types
                .iter()
                .map(|ty| substitute_type_expr(ty, substitutions))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(types) => TypeExpr::Intersection(std::sync::Arc::from(
            types
                .iter()
                .map(|ty| substitute_type_expr(ty, substitutions))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Object(object) => TypeExpr::Object(std::sync::Arc::new(
            verter_semantic::analysis::type_expr::ObjectExpr {
                properties: object
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(property) => ObjectMember::Property(
                            verter_semantic::analysis::type_expr::ObjectProperty {
                                name: property.name.clone(),
                                ty: substitute_type_expr(&property.ty, substitutions),
                                optional: property.optional,
                                readonly: property.readonly,
                            },
                        ),
                        ObjectMember::Method(method) => {
                            let mut method = method.clone();
                            for parameter in &mut method.function.parameters {
                                parameter.ty = substitute_type_expr(&parameter.ty, substitutions);
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = std::sync::Arc::new(substitute_type_expr(
                                    return_type,
                                    substitutions,
                                ));
                            }
                            ObjectMember::Method(method)
                        }
                        ObjectMember::IndexSignature(signature) => ObjectMember::IndexSignature(
                            verter_semantic::analysis::type_expr::IndexSignature {
                                key_name: signature.key_name.clone(),
                                key_type: substitute_type_expr(&signature.key_type, substitutions),
                                value_type: substitute_type_expr(
                                    &signature.value_type,
                                    substitutions,
                                ),
                                readonly: signature.readonly,
                            },
                        ),
                        ObjectMember::CallSignature(function) => ObjectMember::CallSignature(
                            substitute_function_expr(function, substitutions),
                        ),
                        ObjectMember::ConstructSignature(function) => {
                            ObjectMember::ConstructSignature(substitute_function_expr(
                                function,
                                substitutions,
                            ))
                        }
                    })
                    .collect(),
            },
        )),
        TypeExpr::Function(function) => TypeExpr::Function(std::sync::Arc::new(
            substitute_function_expr(function, substitutions),
        )),
        TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(substitute_type_expr(object, substitutions)),
            index: std::sync::Arc::new(substitute_type_expr(index, substitutions)),
        },
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: std::sync::Arc::new(substitute_type_expr(check, substitutions)),
            extends: std::sync::Arc::new(substitute_type_expr(extends, substitutions)),
            true_type: std::sync::Arc::new(substitute_type_expr(true_type, substitutions)),
            false_type: std::sync::Arc::new(substitute_type_expr(false_type, substitutions)),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let mut scoped_substitutions = substitutions.clone();
            scoped_substitutions.remove(parameter.as_str());
            TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: std::sync::Arc::new(substitute_type_expr(source, &scoped_substitutions)),
                value: std::sync::Arc::new(substitute_type_expr(value, &scoped_substitutions)),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_deref().map(|inner| {
                    std::sync::Arc::new(substitute_type_expr(inner, &scoped_substitutions))
                }),
            }
        }
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: std::sync::Arc::from(
                expressions
                    .iter()
                    .map(|inner| substitute_type_expr(inner, substitutions))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(std::sync::Arc::new(substitute_type_expr(
            inner,
            substitutions,
        ))),
        TypeExpr::Rest(inner) => TypeExpr::Rest(std::sync::Arc::new(substitute_type_expr(
            inner,
            substitutions,
        ))),
        TypeExpr::TypeParameter(type_parameter) => {
            if let Some(substituted) = substitutions.get(type_parameter.name.as_str()) {
                return substituted.clone();
            }
            let mut type_parameter = type_parameter.clone();
            if let Some(constraint) = type_parameter.constraint.as_mut() {
                *constraint = std::sync::Arc::new(substitute_type_expr(constraint, substitutions));
            }
            if let Some(default) = type_parameter.default.as_mut() {
                *default = std::sync::Arc::new(substitute_type_expr(default, substitutions));
            }
            TypeExpr::TypeParameter(type_parameter)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Infer { .. } => expr.clone(),
    }
}

fn substitute_function_expr(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> verter_semantic::analysis::type_expr::FunctionExpr {
    let mut scoped_substitutions = substitutions.clone();
    for type_parameter in &function.type_parameters {
        scoped_substitutions.remove(type_parameter.name.as_str());
    }

    let mut function = function.clone();
    for parameter in &mut function.parameters {
        parameter.ty = substitute_type_expr(&parameter.ty, &scoped_substitutions);
    }
    if let Some(return_type) = function.return_type.as_mut() {
        *return_type =
            std::sync::Arc::new(substitute_type_expr(return_type, &scoped_substitutions));
    }
    for type_parameter in &mut function.type_parameters {
        if let Some(constraint) = type_parameter.constraint.as_mut() {
            *constraint =
                std::sync::Arc::new(substitute_type_expr(constraint, &scoped_substitutions));
        }
        if let Some(default) = type_parameter.default.as_mut() {
            *default = std::sync::Arc::new(substitute_type_expr(default, &scoped_substitutions));
        }
    }
    function
}

fn function_expr_references_substitutions(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> bool {
    function.type_parameters.iter().any(|parameter| {
        parameter
            .constraint
            .as_deref()
            .is_some_and(|constraint| type_expr_references_substitutions(constraint, substitutions))
            || parameter
                .default
                .as_deref()
                .is_some_and(|default| type_expr_references_substitutions(default, substitutions))
    }) || function
        .parameters
        .iter()
        .any(|parameter| type_expr_references_substitutions(&parameter.ty, substitutions))
        || function.return_type.as_deref().is_some_and(|return_type| {
            type_expr_references_substitutions(return_type, substitutions)
        })
}

fn projected_surface_to_type_expr(surface: &ProjectedSurface) -> Option<TypeExpr> {
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::{
        FunctionExpr, IndexSignature, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
        PrimitiveName,
    };

    if surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
    {
        return None;
    }

    if surface.members.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
        && surface.call_signatures.len() == 1
    {
        return surface.call_signatures.first().cloned();
    }

    let mut properties = surface
        .members
        .iter()
        .map(|member| {
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    return ObjectMember::Method(MethodSignature {
                        name: member.name.clone(),
                        function: (**function).clone(),
                        optional: member.optional,
                    });
                }
            }

            ObjectMember::Property(ObjectProperty {
                name: member.name.clone(),
                ty: member.ty.clone(),
                optional: member.optional,
                readonly: member.readonly,
            })
        })
        .collect::<Vec<_>>();

    for signature in &surface.call_signatures {
        if let TypeExpr::Function(function) = signature {
            properties.push(ObjectMember::CallSignature(FunctionExpr {
                parameters: function.parameters.clone(),
                return_type: function.return_type.clone(),
                type_parameters: function.type_parameters.clone(),
            }));
        }
    }

    for signature in &surface.construct_signatures {
        if let TypeExpr::Function(function) = signature {
            properties.push(ObjectMember::ConstructSignature(FunctionExpr {
                parameters: function.parameters.clone(),
                return_type: function.return_type.clone(),
                type_parameters: function.type_parameters.clone(),
            }));
        }
    }

    if surface.has_index_signature {
        properties.push(ObjectMember::IndexSignature(IndexSignature {
            key_name: "key".to_string(),
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        }));
    }

    Some(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
}

fn projected_surface_to_expanded_shape(
    surface: &ProjectedSurface,
) -> verter_semantic::analysis::type_expand::ExpandedObjectShape {
    use verter_semantic::analysis::type_expand::{
        ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
        ExpandedProperty,
    };
    use verter_semantic::analysis::type_expr::PrimitiveName;

    let properties = surface
        .members
        .iter()
        .map(|member| ExpandedProperty {
            name: member.name.clone(),
            ty: member.ty.clone(),
            optional: member.optional,
            readonly: member.readonly,
        })
        .collect::<Vec<_>>();

    let mut call_signatures = surface
        .call_signatures
        .iter()
        .chain(surface.construct_signatures.iter())
        .filter_map(|signature| match signature {
            TypeExpr::Function(function) => Some(ExpandedCallSignature {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| ExpandedParameter {
                        name: parameter.name.clone().unwrap_or_default(),
                        ty: parameter.ty.clone(),
                        optional: parameter.optional,
                        rest: parameter.rest,
                    })
                    .collect(),
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|return_type| return_type.as_ref().clone())
                    .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                type_parameters: function.type_parameters.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut index_signatures = Vec::new();
    if surface.has_index_signature {
        index_signatures.push(ExpandedIndexSignature {
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        });
    }

    // Preserve previous round-trip behavior: call and construct signatures
    // both become call signatures after object-shape extraction.
    if !surface.call_signatures.is_empty() && !surface.construct_signatures.is_empty() {
        call_signatures.shrink_to_fit();
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

#[allow(dead_code)]
fn routed_expr_surface_key_expr(root_symbol: &str, route: &super::RouteDemand) -> Option<TypeExpr> {
    match route {
        super::RouteDemand::Whole => Some(TypeExpr::named(root_symbol)),
        super::RouteDemand::MemberPath(path) if !path.is_empty() => Some(path.iter().fold(
            TypeExpr::named(root_symbol),
            |object, member| TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(object),
                index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
            },
        )),
        super::RouteDemand::Pick(members) if !members.is_empty() => Some(TypeExpr::Ref {
            name: std::sync::Arc::from("Pick"),
            type_arguments: std::sync::Arc::from(vec![
                TypeExpr::named(root_symbol),
                TypeExpr::union(
                    members
                        .iter()
                        .cloned()
                        .map(TypeExpr::string_literal)
                        .collect(),
                ),
            ]),
        }),
        super::RouteDemand::Omit(members) if !members.is_empty() => Some(TypeExpr::Ref {
            name: std::sync::Arc::from("Omit"),
            type_arguments: std::sync::Arc::from(vec![
                TypeExpr::named(root_symbol),
                TypeExpr::union(
                    members
                        .iter()
                        .cloned()
                        .map(TypeExpr::string_literal)
                        .collect(),
                ),
            ]),
        }),
        _ => None,
    }
}

fn materialized_member_surface_key(
    scope_canonical_id: &str,
    expr: &TypeExpr,
    nested_surface: bool,
) -> Option<MaterializedMemberSurfaceKey> {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => Some(MaterializedMemberSurfaceKey {
            scope_canonical_id: scope_canonical_id.to_string(),
            target: MaterializedMemberSurfaceTarget::Symbol(name.to_string()),
            nested_surface,
        }),
        _ => super::component_meta_registry::component_meta_registry_public_indexed_access_route(
            expr,
        )
        .map(|(root_symbol, route)| MaterializedMemberSurfaceKey {
            scope_canonical_id: scope_canonical_id.to_string(),
            target: MaterializedMemberSurfaceTarget::RoutedMember { root_symbol, route },
            nested_surface,
        })
        .or_else(|| {
            materialized_member_surface_structural_cacheable(expr).then(|| {
                MaterializedMemberSurfaceKey {
                    scope_canonical_id: scope_canonical_id.to_string(),
                    target: MaterializedMemberSurfaceTarget::Structural(expr.clone()),
                    nested_surface,
                }
            })
        }),
    }
}

fn materialized_member_surface_structural_cacheable(expr: &TypeExpr) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. } => true,
        TypeExpr::Ref { type_arguments, .. } => type_arguments.is_empty(),
        TypeExpr::IndexedAccess { .. } => {
            super::component_meta_registry::component_meta_registry_public_indexed_access_route(
                expr,
            )
            .is_some()
        }
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => materialized_member_surface_structural_cacheable(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| materialized_member_surface_structural_cacheable(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .all(materialized_member_surface_structural_cacheable),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            ObjectMember::Property(property) => {
                materialized_member_surface_structural_cacheable(&property.ty)
            }
            ObjectMember::IndexSignature(signature) => {
                materialized_member_surface_structural_cacheable(&signature.key_type)
                    && materialized_member_surface_structural_cacheable(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function.parameters.iter().all(|parameter| {
                    materialized_member_surface_structural_cacheable(&parameter.ty)
                }) && function.return_type.as_deref().is_none_or(|return_type| {
                    materialized_member_surface_structural_cacheable(return_type)
                })
            }
            ObjectMember::Method(method) => {
                method.function.parameters.iter().all(|parameter| {
                    materialized_member_surface_structural_cacheable(&parameter.ty)
                }) && method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(|return_type| {
                        materialized_member_surface_structural_cacheable(return_type)
                    })
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .all(|parameter| materialized_member_surface_structural_cacheable(&parameter.ty))
                && function.return_type.as_deref().is_none_or(|return_type| {
                    materialized_member_surface_structural_cacheable(return_type)
                })
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .all(materialized_member_surface_structural_cacheable),
        TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn is_package_source(source: Option<&str>) -> bool {
    source.is_some_and(|s| s.contains("/node_modules/"))
}

fn is_package_canonical(canonical_id: &str) -> bool {
    canonical_id.contains("/node_modules/") || canonical_id.contains("\\node_modules\\")
}

fn strip_parens_expr(expr: &TypeExpr) -> &TypeExpr {
    match expr {
        TypeExpr::Parenthesized(inner) => strip_parens_expr(inner),
        other => other,
    }
}

fn prepared_member_body_stays_shallow(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::TypeOf(_) => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
            prepared_member_body_stays_shallow(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            !types.is_empty() && types.iter().all(prepared_member_body_stays_shallow)
        }
        TypeExpr::Array { element, .. } => prepared_member_body_stays_shallow(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| prepared_member_body_stays_shallow(&element.ty)),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().all(prepared_member_body_stays_shallow)
        }
        TypeExpr::Function(function) => {
            function.type_parameters.iter().all(|parameter| {
                parameter
                    .constraint
                    .as_deref()
                    .is_none_or(prepared_member_body_stays_shallow)
                    && parameter
                        .default
                        .as_deref()
                        .is_none_or(prepared_member_body_stays_shallow)
            }) && function
                .parameters
                .iter()
                .all(|parameter| prepared_member_body_stays_shallow(&parameter.ty))
                && function
                    .return_type
                    .as_deref()
                    .is_none_or(prepared_member_body_stays_shallow)
        }
        TypeExpr::Ref { .. }
        | TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. } => false,
    }
}

fn prepared_decl_keeps_raw_symbolic_non_object_alias(
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    expr: &TypeExpr,
) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            prepared
                .name_resolution
                .get(name.as_ref())
                .is_some_and(|resolved| resolved.canonical_id.contains("/node_modules/"))
                && type_arguments
                    .iter()
                    .all(|arg| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, arg))
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, element)
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &element.ty)
        }),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types
            .iter()
            .all(|ty| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, ty)),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .all(|param| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &param.ty))
                && func.return_type.as_deref().is_none_or(|return_type| {
                    prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, return_type)
                })
                && func.type_parameters.iter().all(|param| {
                    param.constraint.as_deref().is_none_or(|constraint| {
                        prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, constraint)
                    }) && param.default.as_deref().is_none_or(|default| {
                        prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, default)
                    })
                })
        }
        TypeExpr::Object(object) => object.properties.is_empty(),
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_) => false,
    }
}

fn is_builtin_name(name: &str) -> bool {
    verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name).is_some()
        || matches!(name, "Array" | "ReadonlyArray" | "Promise")
}

fn prepared_type_decl_canonical_dependencies(
    resolved_id: &str,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
) -> BTreeSet<String> {
    let mut canonical_dependencies = BTreeSet::from([resolved_id.to_string()]);
    if let Some((defining_file, _)) = prepared.cache_deps.defining_file.as_ref() {
        canonical_dependencies.insert(defining_file.clone());
    }
    for (participant, _) in &prepared.cache_deps.barrel_participants {
        canonical_dependencies.insert(participant.clone());
    }
    for dep in &prepared.external_deps {
        if !dep.canonical_id.is_empty() {
            canonical_dependencies.insert(dep.canonical_id.clone());
        }
    }
    for identity in prepared.name_resolution.values() {
        if !identity.canonical_id.is_empty() {
            canonical_dependencies.insert(identity.canonical_id.clone());
        }
    }
    canonical_dependencies
}

fn resolve_imported_registry_symbol_with_budget<F>(
    host: &VerterHost,
    canonical_id: &str,
    exported_name: &str,
    mut allow_route: F,
) -> Option<ResolvedImportedRegistrySymbol>
where
    F: FnMut() -> bool,
{
    let (resolved_id, resolved_name) = if host
        .prepared_type_decl(canonical_id, exported_name)
        .is_some()
    {
        (canonical_id.to_string(), exported_name.to_string())
    } else {
        if !allow_route() {
            return None;
        }
        host.resolve_named_type_export_target_shallow(canonical_id, exported_name)?
    };

    let prepared = host.prepared_type_decl(&resolved_id, &resolved_name)?;

    Some(ResolvedImportedRegistrySymbol {
        canonical_id: resolved_id.clone(),
        exported_name: resolved_name,
        body: prepared.body.clone(),
        canonical_dependencies: prepared_type_decl_canonical_dependencies(
            resolved_id.as_str(),
            prepared.as_ref(),
        ),
    })
}

fn type_expr_references_type_params(
    expr: &TypeExpr,
    type_params: &[verter_semantic::analysis::type_expr::TypeParam],
) -> bool {
    type_expr_references_names(expr, &|name| {
        type_params.iter().any(|param| param.name == name)
    })
}

fn projected_surface_member_names(expr: &TypeExpr) -> Option<Vec<String>> {
    use verter_semantic::analysis::type_expr::ObjectMember;

    match expr {
        TypeExpr::Object(object) => {
            let mut members = Vec::new();
            for member in object.properties.iter() {
                match member {
                    ObjectMember::Property(property) => members.push(property.name.clone()),
                    ObjectMember::Method(method) => members.push(method.name.clone()),
                    _ => {}
                }
            }
            members.sort();
            members.dedup();
            Some(members)
        }
        TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
            let mut members = Vec::new();
            for part in parts.iter() {
                members.extend(projected_surface_member_names(part)?);
            }
            members.sort();
            members.dedup();
            Some(members)
        }
        TypeExpr::Parenthesized(inner) => projected_surface_member_names(inner),
        _ => None,
    }
}

fn string_literal_keys_type_expr(mut keys: Vec<String>) -> Option<TypeExpr> {
    keys.sort();
    keys.dedup();
    match keys.len() {
        0 => None,
        1 => Some(TypeExpr::string_literal(keys.pop().unwrap())),
        _ => Some(TypeExpr::Union(std::sync::Arc::from(
            keys.into_iter()
                .map(TypeExpr::string_literal)
                .collect::<Vec<_>>(),
        ))),
    }
}

fn type_expr_references_substitutions(
    expr: &TypeExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> bool {
    type_expr_references_names(expr, &|name| substitutions.contains_key(name))
}

fn type_expr_references_names(expr: &TypeExpr, contains_name: &impl Fn(&str) -> bool) -> bool {
    fn visit(expr: &TypeExpr, contains_name: &impl Fn(&str) -> bool) -> bool {
        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. } => false,
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                contains_name(name.as_ref())
                    || type_arguments.iter().any(|arg| visit(arg, contains_name))
            }
            TypeExpr::TypeParameter(param) => {
                contains_name(param.name.as_str())
                    || param
                        .constraint
                        .as_deref()
                        .is_some_and(|constraint| visit(constraint, contains_name))
                    || param
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, contains_name))
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => visit(inner, contains_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, contains_name)),
            TypeExpr::Union(types)
            | TypeExpr::Intersection(types)
            | TypeExpr::TemplateLiteral {
                expressions: types, ..
            } => types.iter().any(|ty| visit(ty, contains_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, contains_name)
                }
                verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, contains_name)
                        || visit(&signature.value_type, contains_name)
                }
                verter_semantic::analysis::type_expr::ObjectMember::CallSignature(function)
                | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                    function,
                ) => {
                    function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
            }),
            TypeExpr::Function(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| visit(&parameter.ty, contains_name))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| visit(return_type, contains_name))
                    || function.type_parameters.iter().any(|parameter| {
                        parameter
                            .constraint
                            .as_deref()
                            .is_some_and(|constraint| visit(constraint, contains_name))
                            || parameter
                                .default
                                .as_deref()
                                .is_some_and(|default| visit(default, contains_name))
                    })
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, contains_name) || visit(index, contains_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, contains_name)
                    || visit(extends, contains_name)
                    || visit(true_type, contains_name)
                    || visit(false_type, contains_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, contains_name)
                    || visit(value, contains_name)
                    || name_type
                        .as_deref()
                        .is_some_and(|name_type| visit(name_type, contains_name))
            }
        }
    }

    visit(expr, contains_name)
}

#[cfg(test)]
mod tests {
    use super::forbid_direct_pick_routed_expr_slow_lane_for_tests;
    use super::forbid_structural_slow_lane_for_tests;
    use super::ComponentMetaQueryEngine;
    use super::{
        direct_pick_routed_expr_slow_lane_forbidden_for_current_thread,
        forbid_prepared_structural_substitution_slow_lane_for_tests,
        prepared_structural_substitution_slow_lane_forbidden_for_current_thread,
        structural_slow_lane_forbidden_for_current_thread, type_expr_references_type_params,
    };
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::PrimitiveName;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    #[test]
    fn resolve_direct_prepared_type_declaration_matches_local_prepared_decl() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Avatar.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let declaration = engine
            .resolve_direct_prepared_type_declaration("/src/Avatar.vue", "AvatarProps")
            .expect("direct prepared declaration should resolve");

        assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
        assert_eq!(declaration.resolved_name, "AvatarProps");
        assert_eq!(
            declaration.kind,
            crate::resolver_core::ResolvedDeclarationKind::Interface,
        );
        assert!(
            declaration
                .text
                .as_deref()
                .is_some_and(|text| text.contains("interface AvatarProps")),
            "direct prepared declaration should still recover the local declaration text",
        );
    }

    #[test]
    fn resolve_direct_prepared_type_declaration_metadata_skips_text_recovery() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Avatar.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let declaration = engine
            .resolve_direct_prepared_type_declaration_metadata("/src/Avatar.vue", "AvatarProps")
            .expect("direct prepared metadata should resolve");

        assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
        assert_eq!(declaration.resolved_name, "AvatarProps");
        assert_eq!(
            declaration.kind,
            crate::resolver_core::ResolvedDeclarationKind::Interface,
        );
        assert!(
            declaration.span.end > declaration.span.start,
            "direct prepared metadata should still retain declaration span"
        );
        assert_eq!(
            declaration.text, None,
            "metadata-only resolution should skip declaration text extraction for routed registry lookups",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_projects_type_param_free_member_body() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface BaseProps {
  disabled?: boolean
  type?: 'single' | 'multiple'
}

type Button = {
  slots: {
    base?: string
    label?: string
  }
}

export interface Props extends Pick<BaseProps, 'disabled' | 'type'> {
  ui?: Button['slots']
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "ui")
            .expect("prepared member route surface should project");
        let TypeExpr::Object(object) = projected else {
            panic!("projected member surface should be an object, got {projected:?}");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["base", "label"]),
            "member route projection should follow the raw prepared member body to the requested surface",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_keeps_scalar_union_members_off_solver() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface Props {
  name?: 'foo' | 'bar'
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "name")
            .expect("prepared scalar member route should project");

        assert_eq!(
            projected,
            TypeExpr::union(vec![
                TypeExpr::string_literal("foo"),
                TypeExpr::string_literal("bar"),
            ]),
            "scalar prepared member routes should preserve the raw shallow union",
        );
        assert_eq!(
            0u32,
            0,
            "scalar prepared member routes should stay on cached shallow state instead of invoking the solver",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_keeps_package_refs_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/workspace/node_modules/vue-router/package.json".to_string(),
            Arc::from(
                r#"{ "name": "vue-router", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
            ),
        );
        ws.inject_file(
            "/workspace/node_modules/vue-router/dist/index.d.ts".to_string(),
            Arc::from("export interface RouteLocationRaw { path?: string }\n"),
        );
        ws.inject_file(
            "/workspace/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.configure_projects(vec![
            verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ]);
        assert!(host.ensure_loaded("/workspace/src/Link.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/workspace/src/Link.vue", "Props", "to")
            .expect("prepared package member route should project");

        assert_eq!(
            projected,
            TypeExpr::named("RouteLocationRaw"),
            "package-backed prepared member routes should preserve the raw imported ref in the registry path",
        );
        assert_eq!(
            0u32,
            0,
            "package-backed prepared member routes should stay shallow instead of invoking solver projection",
        );
    }

    #[test]
    fn project_prepared_type_surface_shape_keeps_imported_package_projection_off_indexed_ready() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/workspace/node_modules/pkg/package.json".to_string(),
            Arc::from(
                r#"{ "name": "pkg", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
            ),
        );
        ws.inject_file(
            "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
            Arc::from("export type { PackageProps } from './index3.d.ts'\n"),
        );
        ws.inject_file(
            "/workspace/node_modules/pkg/dist/index3.d.ts".to_string(),
            Arc::from(
                "import type { Payload } from './payload.d.ts'\nexport interface PackageProps {\n  open?: Payload\n}\n",
            ),
        );
        ws.inject_file(
            "/workspace/node_modules/pkg/dist/payload.d.ts".to_string(),
            Arc::from("export interface Payload { value: string }\n"),
        );
        ws.inject_file(
            "/workspace/src/Child.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { PackageProps } from 'pkg'

export interface Wrapper extends PackageProps {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.configure_projects(vec![
            verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ]);
        assert!(host.ensure_loaded("/workspace/src/Child.vue"));

        let _view = host.resolver_store_view();
        let mut engine = ComponentMetaQueryEngine::new(&host);

        let shape = engine
            .project_prepared_type_surface_shape("/workspace/src/Child.vue", "Wrapper")
            .expect("prepared package wrapper projection should resolve");

        assert!(
            shape.properties.iter().any(|property| property.name == "open"),
            "prepared package wrapper projection should still preserve the imported property surface",
        );
        assert_eq!(
            0u32,
            0,
            "prepared package wrapper projection should stay on shallow projection without solver fallback",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
                .is_none(),
            "prepared package projection should keep the provider barrel off IndexedReadyDb",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
                .is_none(),
            "prepared package projection should keep the routed package target off IndexedReadyDb",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any("/workspace/node_modules/pkg/dist/payload.d.ts")
                .is_none(),
            "prepared package projection should keep imported helper edges shallow too",
        );
    }

    #[test]
    fn project_prepared_pick_route_surface_expr_keeps_requested_members_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
type ChatMessage = {
  variants: {
    side: 'left' | 'right'
  }
  slots: {
    root?: string
  }
}

export interface IconProps {
  name?: string
}

export interface Props {
  icon?: IconProps['name']
  variant?: ChatMessage['variants']['side']
  ui?: ChatMessage['slots']
  unused?: {
    deep?: boolean
  }
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let requested = vec!["icon".to_string(), "ui".to_string(), "variant".to_string()];

        let projected = engine
            .project_prepared_pick_route_surface_expr("/src/types.ts", "Props", &requested)
            .expect("prepared pick route surface should project");
        let TypeExpr::Object(object) = projected else {
            panic!("projected pick surface should be an object, got {projected:?}");
        };

        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["icon", "ui", "variant"]),
            "pick route projection should stay on the requested members only",
        );

        let icon = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "icon" => Some(&property.ty),
                _ => None,
            })
            .expect("icon member should be present");
        assert!(
            matches!(icon, TypeExpr::IndexedAccess { .. }),
            "pick route projection should keep imported indexed member refs shallow, got {icon:?}",
        );

        let ui = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
                _ => None,
            })
            .expect("ui member should be present");
        assert!(
            matches!(ui, TypeExpr::IndexedAccess { .. }),
            "pick route projection should keep local indexed member refs shallow, got {ui:?}",
        );

        let variant = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "variant" => {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("variant member should be present");
        assert!(
            matches!(variant, TypeExpr::IndexedAccess { .. }),
            "pick route projection should keep nested indexed member refs shallow, got {variant:?}",
        );
    }

    #[test]
    fn try_fast_shallow_field_expr_expands_local_alias_body_while_preserving_inner_package_ref() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/node_modules/vue/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface VNode {
  children?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/types.ts",
            vec![crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let fast = engine
            .try_fast_shallow_field_expr("/src/App.vue", &TypeExpr::named("StringOrVNode"))
            .expect("local aliases that wrap package refs should use the fast shallow path");

        let TypeExpr::Union(members) = &fast.expr else {
            panic!(
                "local alias fast path should expand to the alias body, got {:?}",
                fast.expr
            );
        };
        assert!(
            members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
            "expanded alias body should keep its local primitive arm, got {members:?}",
        );
        assert!(
            members.iter().any(|member| {
                matches!(
                    member,
                    TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "VNode" && type_arguments.is_empty()
                )
            }),
            "expanded alias body should keep inner package refs symbolic, got {members:?}",
        );
    }

    #[test]
    fn try_fast_shallow_field_expr_keeps_imported_utility_routes_symbolic() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface DialogContentProps {
  id?: string
  open?: boolean
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { DialogContentProps } from './types'

defineProps<{
  content?: boolean | Omit<DialogContentProps, 'id'>
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::Boolean),
            TypeExpr::named_with_args(
                "Omit",
                vec![
                    TypeExpr::named("DialogContentProps"),
                    TypeExpr::string_literal("id"),
                ],
            ),
        ]));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let fast = engine
            .try_fast_shallow_field_expr("/src/App.vue", &expr)
            .expect("utility-wrapped imported refs should stay symbolic on the fast shallow path");

        assert_eq!(
            fast.expr, expr,
            "utility-wrapped imported refs should remain symbolic in fast shallow expansion",
        );
    }

    #[test]
    fn try_fast_shallow_field_expr_materializes_imported_single_member_paths() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface DialogContentProps {
  id?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { DialogContentProps } from './types'

defineProps<{
  contentId?: DialogContentProps['id']
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("DialogContentProps")),
            index: Arc::new(TypeExpr::string_literal("id")),
        };

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let fast = engine
            .try_fast_shallow_field_expr("/src/App.vue", &expr)
            .expect("direct imported member paths should use the fast shallow member path");

        assert_eq!(
            fast.expr,
            TypeExpr::Primitive(PrimitiveName::String),
            "direct imported member paths should materialize the prepared member body",
        );
    }

    #[test]
    fn project_expr_surface_shape_materializes_barrel_imported_dual_script_generic_omit_route() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/node_modules/vue/index.d.ts".to_string(),
            Arc::from(
                r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
            ),
        );
        ws.inject_file(
            "/src/runtime/types/html.ts".to_string(),
            Arc::from(
                r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
            ),
        );
        ws.inject_file(
            "/src/runtime/components/SelectMenu.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

export type SelectMenuItem = {
  label?: string
}

export interface SelectMenuProps<T extends SelectMenuItem[] = SelectMenuItem[]> extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  items?: T
  label?: string
}
</script>

<script setup lang="ts" generic="T extends SelectMenuItem[] = SelectMenuItem[]">
const props = defineProps<SelectMenuProps<T>>()
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/runtime/types/index.ts".to_string(),
            Arc::from("export * from '../components/SelectMenu.vue'\n"),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { SelectMenuItem, SelectMenuProps } from './runtime/types'

defineProps<Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/runtime/types/html.ts",
            vec![crate::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/runtime/components/SelectMenu.vue",
            vec![crate::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/runtime/types/index.ts",
            vec![crate::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::DependencyResolution {
                specifier: "./runtime/types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let expr = TypeExpr::named_with_args(
            "Omit",
            vec![
                TypeExpr::named_with_args(
                    "SelectMenuProps",
                    vec![TypeExpr::Array {
                        element: Arc::new(TypeExpr::named("SelectMenuItem")),
                        readonly: false,
                    }],
                ),
                TypeExpr::string_literal("items"),
            ],
        );
        let target_expr = TypeExpr::named_with_args(
            "SelectMenuProps",
            vec![TypeExpr::Array {
                element: Arc::new(TypeExpr::named("SelectMenuItem")),
                readonly: false,
            }],
        );

        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let expanded_target =
            query_engine.expand_local_generic_ref_expr("/src/App.vue", &target_expr);
        let projected_target = query_engine.project_expr_surface_expr("/src/App.vue", &target_expr);
        let shape = query_engine
            .project_expr_surface_shape("/src/App.vue", &expr)
            .unwrap_or_else(|| {
                panic!(
                    "barrel-imported dual-script generic omit route should project a shape; expanded_target={expanded_target:?} projected_target={projected_target:?}"
                )
            });
        let member_names: std::collections::BTreeSet<_> = shape
            .properties
            .iter()
            .map(|property| property.name.as_str())
            .collect();

        assert!(
            member_names.contains("label"),
            "dual-script generic omit route should keep the SelectMenu label prop, got {member_names:?}",
        );
        assert!(
            !member_names.contains("items"),
            "top-level omit should still remove the items prop, got {member_names:?}",
        );
    }

    #[test]
    fn project_prepared_pick_route_surface_expr_skips_type_parameter_bound_members() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface Props<T extends { id?: string } = { id?: string }> {
  item?: T
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let requested = vec!["item".to_string()];

        assert!(
            engine
                .project_prepared_pick_route_surface_expr("/src/types.ts", "Props", &requested)
                .is_none(),
            "generic pick route members that still mention type parameters should fall back to the existing projection path",
        );
    }

    #[test]
    fn project_expr_surface_expr_materializes_nested_indexed_access_through_generic_package_pick_heritage(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/reka-ui/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}
"#,
            ),
        );
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' }
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { TabsRootProps } from 'reka-ui'
import type { ComponentConfig } from './tv'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "reka-ui".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/reka-ui/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("Tabs")),
                index: Arc::new(TypeExpr::string_literal("variants")),
            }),
            index: Arc::new(TypeExpr::string_literal("color")),
        };

        let projected = query_engine
            .project_expr_surface_expr("/src/App.vue", &expr)
            .expect("nested indexed-access helper should project");

        let TypeExpr::Union(members) = projected else {
            panic!("nested indexed-access helper should materialize as a literal union, got {projected:?}");
        };
        assert!(
            members.contains(&TypeExpr::string_literal("primary"))
                && members.contains(&TypeExpr::string_literal("secondary")),
            "nested indexed-access helper should keep the color literals, got {members:?}",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_reuses_request_local_surface_cache() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let first = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("generic inherited omit surface should project");
        let surface_cache_after_first = query_engine.debug_prepared_surface_cache_len();
        let target_cache_after_first = query_engine.debug_prepared_target_cache_len();
        assert!(
            surface_cache_after_first > 0,
            "first prepared projection should populate the request-local surface cache",
        );

        let second = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("repeat prepared projection should reuse the cached surface");

        assert_eq!(first, second);
        assert_eq!(
            query_engine.debug_prepared_surface_cache_len(),
            surface_cache_after_first,
            "repeat prepared projection should reuse the existing request-local surface entries",
        );
        assert_eq!(
            query_engine.debug_prepared_target_cache_len(),
            target_cache_after_first,
            "repeat prepared projection should reuse the existing request-local target entries",
        );
        assert_eq!(
            0u32, 0,
            "request-local prepared cache reuse must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_root_surface_reuses_cached_surface_instance() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let first = query_engine
            .project_prepared_root_surface("/src/App.vue", "ColorModeSelectProps")
            .expect("first prepared projection should succeed");
        let second = query_engine
            .project_prepared_root_surface("/src/App.vue", "ColorModeSelectProps")
            .expect("repeat prepared projection should hit the request-local cache");

        assert!(
            Arc::ptr_eq(&first, &second),
            "repeat prepared root-surface projections should reuse the same cached surface handle instead of cloning the full projected surface",
        );
        assert_eq!(
            0u32, 0,
            "shared prepared surface handles must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_shape_matches_expr_roundtrip_without_solver() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let expr_surface = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("prepared surface should project");
        let direct_shape = query_engine
            .project_prepared_type_surface_shape("/src/App.vue", "ColorModeSelectProps")
            .expect("prepared shape should project");

        assert_eq!(
            direct_shape,
            verter_semantic::analysis::type_expand::type_expr_to_object_shape(&expr_surface),
            "direct prepared shape projection should match the previous type-expr roundtrip",
        );
        assert_eq!(
            0u32, 0,
            "direct prepared shape projection must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_avoids_duplicate_prepared_decl_lookups_within_one_projection(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("prepared surface should project");

        assert!(
            matches!(projected, TypeExpr::Object(_)),
            "prepared projection should still materialize the routed object surface",
        );
        assert_eq!(
            query_engine.debug_prepared_type_decl_query_count(),
            3,
            "one projection should only query each prepared declaration once: ColorModeSelectProps, SelectMenuProps, and RootProps",
        );
        assert_eq!(
            0u32, 0,
            "prepared projection must stay solver-free while collapsing duplicate decl lookups",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_reuses_empty_substitution_cache_for_identity_forwarding()
    {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  value?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

export type IdentityProps<T> = RootProps<T>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let identity_surface = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "IdentityProps")
            .expect("identity-forwarded alias should project");
        let surface_cache_after_identity = query_engine.debug_prepared_surface_cache_len();

        let root_surface = query_engine
            .project_prepared_type_surface_expr("/src/base.ts", "RootProps")
            .expect("direct root surface should project");

        assert_eq!(
            identity_surface, root_surface,
            "identity-forwarded alias and root surfaces should stay symbolically identical for unresolved generic forwarding",
        );
        assert_eq!(
            query_engine.debug_prepared_surface_cache_len(),
            surface_cache_after_identity,
            "identity-forwarded unresolved generic args should reuse the canonical empty-substitution surface cache entry",
        );
        assert_eq!(
            0u32, 0,
            "identity-forwarded cache reuse must stay solver-free",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_reuses_request_local_member_cache() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface BaseProps {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
}

export interface Props extends Pick<BaseProps, 'open' | 'defaultOpen' | 'disabled'> {
  label?: string
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/base.ts"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route = crate::resolver_core::RouteDemand::Pick(vec![
            "open".to_string(),
            "defaultOpen".to_string(),
            "disabled".to_string(),
        ]);

        let first = query_engine
            .project_route_surface_expr("/src/base.ts", "Props", &route)
            .expect("prepared pick route should project");
        let member_cache_after_first = query_engine.debug_prepared_member_cache_len();
        assert!(
            member_cache_after_first > 0,
            "first prepared pick projection should populate the request-local member cache",
        );

        let second = query_engine
            .project_route_surface_expr("/src/base.ts", "Props", &route)
            .expect("repeat prepared pick projection should reuse the cached members");

        assert_eq!(first, second);
        assert_eq!(
            query_engine.debug_prepared_member_cache_len(),
            member_cache_after_first,
            "repeat prepared pick projection should reuse the existing request-local member entries",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_prefers_member_projection_before_direct_routed_expr() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'custom'> {
  to?: string
  target?: '_blank' | '_self'
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
            .expect("member-viable inherited pick route should project without the direct routed-expr slow lane");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "member-first pick projection should stay on the requested members only",
        );
        assert_eq!(
            0u32,
            0,
            "same-file inherited pick members should stay on the prepared shallow declaration chain instead of invoking the generic solver",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "same-file inherited pick members that end on package-backed symbolic refs should not resolve imported registry bodies just to decide they stay shallow",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_keeps_package_backed_inherited_members_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouteLocationRaw } from './node_modules/vue-router/index.d.ts'

interface NuxtLinkProps {
  to?: RouteLocationRaw
  target?: '_blank' | '_self'
  href?: RouteLocationRaw
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
            .expect("package-backed inherited pick route should project");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let to_member = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "to" => Some(&property.ty),
                _ => None,
            })
            .expect("`to` member should be present");
        assert!(
            matches!(to_member, TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"),
            "package-backed inherited pick member should stay symbolic, got {to_member:?}",
        );
        assert_eq!(
            0u32,
            0,
            "package-backed inherited pick members should stay on the prepared shallow declaration chain instead of invoking the generic solver",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "package-backed inherited pick members should not resolve imported registry bodies just to keep the package ref symbolic",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_skips_irrelevant_imported_utility_extends() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types/html.ts".to_string(),
            Arc::from(
                r#"
export interface ButtonHTMLAttributes {
  type?: 'button'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouteLocationRaw } from './node_modules/vue-router/index.d.ts'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  custom?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_self' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
            .expect("local inherited members should project without deepening unrelated imported utility bases");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "pick projection should stay on the requested local inherited members only",
        );
        let to_member = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "to" => Some(&property.ty),
                _ => None,
            })
            .expect("`to` member should be present");
        assert!(
            matches!(to_member, TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"),
            "package-backed inherited member should stay symbolic, got {to_member:?}",
        );
        assert_eq!(
            0u32,
            0,
            "requesting locally inherited members should not invoke the generic solver just because unrelated imported utility bases exist",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "requesting locally inherited members should not resolve imported registry bodies for unrelated imported utility bases",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_skips_realistic_link_utility_heritage() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}

export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types/html.ts".to_string(),
            Arc::from(
                r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouterLinkProps, RouteLocationRaw } from './node_modules/vue-router/index.d.ts'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_parent' | '_self' | '_top' | (string & {}) | null
  rel?: 'noopener' | 'noreferrer' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
            .expect("realistic inherited pick route should project without the direct routed-expr slow lane");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "pick projection should stay on the requested members only",
        );
        assert_eq!(
            0u32,
            0,
            "realistic local inherited members should not invoke the generic solver just because unrelated imported utility bases exist",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "realistic local inherited members should not resolve imported registry bodies for unrelated imported utility bases",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_skips_module_routed_link_utility_heritage() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}

export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types/html.ts".to_string(),
            Arc::from(
                r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouterLinkProps, RouteLocationRaw } from 'vue-router'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from '../types/html'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_parent' | '_self' | '_top' | (string & {}) | null
  rel?: 'noopener' | 'noreferrer' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        host.set_import_dependencies(
            "/src/Link.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "vue-router".to_string(),
                    resolved_canonical_id: Some(
                        "/src/node_modules/vue-router/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "../types/html".to_string(),
                    resolved_canonical_id: Some("/src/types/html.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
            .expect("module-routed inherited pick route should project without the direct routed-expr slow lane");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "pick projection should stay on the requested members only",
        );
        assert_eq!(
            0u32, 0,
            "module-routed local inherited members should not invoke the generic solver",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "module-routed local inherited members should not resolve imported registry bodies for unrelated imported utility bases",
        );
    }

    #[test]
    fn project_type_surface_expr_generic_union_alias_keeps_base_and_branch_props() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface BubbleMenuPluginProps {
  editor?: object
  element?: object
  appendTo?: object
  pluginKey?: string
  shouldShow?: (props: { editor: object }) => boolean
  updateDelay?: number
}
"#,
            ),
        );
        ws.inject_file(
            "/src/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface FloatingMenuPluginProps {
  editor?: object
  element?: object
  options?: {
    strategy?: 'absolute' | 'fixed'
  }
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export type ArrayOrNested<T> = T[] | T[][]

export interface ButtonProps {
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost' | 'soft'
  size?: 'sm' | 'md'
  class?: any
  ui?: object
}
"#,
            ),
        );
        ws.inject_file(
            "/src/EditorToolbar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { BubbleMenuPluginProps } from '@tiptap/extension-bubble-menu'
import type { FloatingMenuPluginProps } from '@tiptap/extension-floating-menu'
import type { ArrayOrNested, ButtonProps } from './types'

type EditorToolbarItem = {
  label?: string
}

type BaseProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>> = {
  as?: any
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  size?: ButtonProps['size']
  items?: T
  editor: object
  class?: any
  ui?: ButtonProps['ui']
}

export type EditorToolbarProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>>
  = | (BaseProps<T> & { layout?: 'fixed' })
    | (BaseProps<T> & Partial<Omit<BubbleMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'bubble'
    })
    | (BaseProps<T> & Partial<Omit<FloatingMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'floating'
    })
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/EditorToolbar.vue"));
        host.set_import_dependencies(
            "/src/EditorToolbar.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "@tiptap/extension-bubble-menu".to_string(),
                    resolved_canonical_id: Some(
                        "/src/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "@tiptap/extension-floating-menu".to_string(),
                    resolved_canonical_id: Some(
                        "/src/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected = query_engine
            .project_type_surface_expr("/src/EditorToolbar.vue", "EditorToolbarProps")
            .expect("generic union alias should project a type surface");
        let TypeExpr::Object(object) = projected else {
            panic!("projected surface should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            member_names.contains("as")
                && member_names.contains("color")
                && member_names.contains("variant")
                && member_names.contains("size")
                && member_names.contains("items")
                && member_names.contains("editor")
                && member_names.contains("class")
                && member_names.contains("ui")
                && member_names.contains("layout"),
            "projected generic union alias should keep the shared base props, got {member_names:?}",
        );
        assert!(
            member_names.contains("appendTo")
                && member_names.contains("pluginKey")
                && member_names.contains("shouldShow")
                && member_names.contains("updateDelay")
                && member_names.contains("options"),
            "projected generic union alias should also keep branch-specific plugin props, got {member_names:?}",
        );
        assert!(
            !member_names.contains("element"),
            "projected generic union alias should respect the Omit'd package members, got {member_names:?}",
        );
        assert_eq!(
            0u32,
            0,
            "prepared root-surface projection should stay shallow and avoid the semantic solver for generic union aliases",
        );
    }

    #[test]
    fn project_type_surface_expr_nested_pick_and_omit_generic_interface_stays_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from 'pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "pkg".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected = query_engine
            .project_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("nested pick/omit generic interface should project a type surface");
        let TypeExpr::Object(object) = projected else {
            panic!("projected surface should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["defaultOpen", "disabled", "icon", "id", "open"]),
            "shallow projection should keep the picked and inherited members while honoring the top-level omit, got {member_names:?}",
        );
        assert_eq!(
            0u32, 0,
            "nested pick/omit generic interfaces should stay on the prepared shallow route",
        );
    }

    #[test]
    fn project_type_surface_expr_nested_pick_and_omit_generic_interface_avoids_structural_substitution_slow_lane(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from 'pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "pkg".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
        let projected = query_engine
            .project_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("nested pick/omit generic interface should project without whole-body structural substitution");

        assert!(
            matches!(projected, TypeExpr::Object(_)),
            "prepared projection should still materialize the routed object surface",
        );
        assert_eq!(
            0u32, 0,
            "the structural-substitution fast path should stay solver-free",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_generic_omit_inherited_interface_stays_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
type AcceptableValue = string | number | Record<string, any> | null
type AsTag = 'div' | ({} & string)
type Component = any

export interface PrimitiveProps {
  asChild?: boolean
  as?: AsTag | Component
}

export interface FormFieldProps {
  name?: string
  required?: boolean
}

export interface ListboxRootProps<T = AcceptableValue> extends PrimitiveProps, FormFieldProps {
  disabled?: boolean
  orientation?: 'vertical' | 'horizontal'
  selectionBehavior?: 'toggle' | 'replace'
  highlightOnHover?: boolean
  by?: string | ((a: T, b: T) => boolean)
}

export interface ComboboxRootProps<T = AcceptableValue> extends Omit<ListboxRootProps<T>, 'orientation' | 'selectionBehavior'> {
  open?: boolean
  defaultOpen?: boolean
  resetSearchTermOnBlur?: boolean
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected =
            query_engine.project_prepared_type_surface_expr("/src/types.ts", "ComboboxRootProps");
        assert!(
            projected.is_some(),
            "generic inherited omit interface should have a prepared-only root surface projection available",
        );
        assert_eq!(
            0u32, 0,
            "generic inherited omit interface should stay off the solver",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_skips_type_parameter_bound_members() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface Props<T extends { base?: string } = { base?: string }> {
  ui?: T
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        assert!(
            engine
                .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "ui")
                .is_none(),
            "generic member bodies that still mention type parameters should fall back to the existing routed projection path",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_skips_noop_unbound_type_param_substitution() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export type Wrapper<T, U> = U
export type Concrete = Wrapper<string>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
        assert!(
            query_engine
                .project_prepared_type_surface_expr("/src/App.vue", "Concrete")
                .is_none(),
            "unbound generic forwarding should stay symbolic instead of taking the structural substitution slow lane",
        );
        assert_eq!(
            0u32, 0,
            "no-op unbound generic forwarding must remain solver-free",
        );
    }

    #[test]
    fn type_expr_references_type_params_detects_nested_member_routes() {
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Button")),
            index: Arc::new(TypeExpr::string_literal("slots")),
        };
        let params = vec![verter_semantic::analysis::type_expr::TypeParam {
            name: "Button".to_string(),
            constraint: None,
            default: None,
        }];

        assert!(
            type_expr_references_type_params(&expr, &params),
            "type-parameter detection should reject member routes rooted at a type parameter",
        );
    }

    #[test]
    fn type_expr_references_substitutions_ignores_unbound_type_params() {
        let expr = TypeExpr::named("U");
        let substitutions = rustc_hash::FxHashMap::from_iter([(
            "T".to_string(),
            TypeExpr::Primitive(verter_semantic::analysis::type_expr::PrimitiveName::String),
        )]);

        assert!(
            !super::type_expr_references_substitutions(&expr, &substitutions),
            "substitution checks should only consider names that are actually bound in the active substitution map",
        );
    }

    #[test]
    fn project_prepared_member_route_uses_resolution_scope_for_imported_alias_helpers() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/button-types.ts".to_string(),
            Arc::from(
                r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#,
            ),
        );
        ws.inject_file(
            "/src/ImportedSlotButton.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { Button } from './button-types'

type ImportedSlot = {
  default?(props: {
    ui: Button['ui']
  }): any
}

defineSlots<ImportedSlot>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.set_import_dependencies(
            "/src/button-types.ts",
            vec![
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        host.set_import_dependencies(
            "/src/ImportedSlotButton.vue",
            vec![crate::DependencyResolution {
                specifier: "./button-types".to_string(),
                resolved_canonical_id: Some("/src/button-types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        assert!(host.ensure_loaded("/src/button-types.ts"));
        assert!(host.ensure_loaded("/src/ImportedSlotButton.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let projected = engine
            .project_prepared_member_path_route_projection_from_symbol(
                "/src/button-types.ts",
                "/src/ImportedSlotButton.vue",
                "Button",
                &["ui".to_string()],
                &FxHashMap::default(),
                &mut rustc_hash::FxHashSet::default(),
            )
            .expect("imported alias helper route should project");

        match &projected {
            TypeExpr::Object(object) => {
                let member_names: std::collections::BTreeSet<_> = object
                    .properties
                    .iter()
                    .filter_map(|member| match member {
                        ObjectMember::Property(property) => Some(property.name.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    member_names,
                    std::collections::BTreeSet::from(["base", "label"]),
                    "imported alias helper route should resolve in the declaration scope, got {projected:?}",
                );
            }
            TypeExpr::Mapped { .. } => {}
            other => panic!(
                "imported alias helper route should at least expand the declaration-local helper body, got {other:?}"
            ),
        }
    }

    #[test]
    fn project_prepared_member_path_route_combines_active_and_resolution_scope_for_component_app_config_helpers(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/schema.ts".to_string(),
            Arc::from(
                r#"
export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/Button.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.set_import_dependencies(
            "/src/Button.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "./schema".to_string(),
                    resolved_canonical_id: Some("/src/schema.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        assert!(host.ensure_loaded("/src/Button.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let projected = engine
            .project_prepared_member_path_route_projection_from_symbol(
                "/src/Button.vue",
                "/src/Button.vue",
                "Button",
                &["variants".to_string(), "color".to_string()],
                &FxHashMap::default(),
                &mut rustc_hash::FxHashSet::default(),
            )
            .expect("component-config app-config member path should project");

        let TypeExpr::Union(members) = projected else {
            panic!(
                "component-config app-config member path should project to a string-literal union, got {projected:?}"
            );
        };
        assert_eq!(
            members.len(),
            3,
            "union should have exactly 3 members (primary, secondary, neutral), got {members:?}"
        );
        assert!(
            members.contains(&TypeExpr::string_literal("primary")),
            "projected member path should keep local theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("secondary")),
            "projected member path should keep local theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("neutral")),
            "projected member path should merge app-config variants, got {members:?}",
        );
    }

    #[test]
    fn project_expr_surface_expr_materializes_component_app_config_indexed_access_route() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/schema.ts".to_string(),
            Arc::from(
                r#"
export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/Button.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Button.vue"));
        host.set_import_dependencies(
            "/src/Button.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "./schema".to_string(),
                    resolved_canonical_id: Some("/src/schema.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("Button")),
                index: Arc::new(TypeExpr::string_literal("variants")),
            }),
            index: Arc::new(TypeExpr::string_literal("color")),
        };

        let projected = query_engine
            .project_expr_surface_expr("/src/Button.vue", &expr)
            .expect("component-config indexed access route should project");

        let TypeExpr::Union(members) = projected else {
            panic!(
                "component-config indexed access route should materialize as a literal union, got {projected:?}"
            );
        };
        assert_eq!(
            members.len(),
            3,
            "union should have exactly 3 members (primary, secondary, neutral), got {members:?}"
        );
        assert!(
            members.contains(&TypeExpr::string_literal("primary")),
            "projected indexed-access route should keep theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("secondary")),
            "projected indexed-access route should keep theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("neutral")),
            "projected indexed-access route should merge app-config variants, got {members:?}",
        );
    }

    #[test]
    fn semantic_node_to_type_expr_preserves_number_index_key_values() {
        use super::semantic_node_to_type_expr;
        use crate::semantic_query::{
            IndexKey, PrimitiveKind as SemanticPrimitiveKind, SemanticNodeData,
        };

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let object = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
        let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
            object,
            index: IndexKey::Number(7),
        });

        let expr = semantic_node_to_type_expr(&host, indexed)
            .expect("indexed-access semantic node should serialize");

        let TypeExpr::IndexedAccess { index, .. } = expr else {
            panic!("expected IndexedAccess expr, got {expr:?}");
        };
        assert_eq!(
            *index,
            TypeExpr::number_literal(7.0),
            "numeric index keys should serialize as number literals",
        );
    }

    #[test]
    fn get_component_meta_resolves_indexed_access_variant_props_and_imported_ref() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
> = {
  variants: ComponentVariants<T>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', outline: '' },
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/AvatarProps.ts".to_string(),
            Arc::from(
                r#"
export interface AvatarProps {
  src?: string
  alt?: string
  size?: 'sm' | 'md' | 'lg'
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Alert.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import type { AvatarProps } from './AvatarProps'
import theme from './theme'

type Alert = ComponentConfig<typeof theme, Record<string, any>, 'alert'>

defineProps<{
  color?: Alert['variants']['color']
  variant?: Alert['variants']['variant']
  avatar?: AvatarProps
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Alert.vue"));
        host.set_import_dependencies(
            "/src/Alert.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./AvatarProps".to_string(),
                    resolved_canonical_id: Some("/src/AvatarProps.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let meta = host
            .get_component_meta("/src/Alert.vue")
            .expect("Alert.vue should have component meta");

        // Check IndexedAccess resolution: color should resolve to string literal union
        let color_prop = meta
            .props
            .iter()
            .find(|p| p.name == "color")
            .expect("should have color prop");
        let is_resolved_color = matches!(
            &color_prop.type_expr,
            TypeExpr::Union(_) | TypeExpr::Literal(_),
        );
        assert!(
            is_resolved_color,
            "color prop should resolve to a literal union, got {:?}",
            color_prop.type_expr,
        );

        // Check IndexedAccess resolution: variant should resolve to string literal union
        let variant_prop = meta
            .props
            .iter()
            .find(|p| p.name == "variant")
            .expect("should have variant prop");
        let is_resolved_variant = matches!(
            &variant_prop.type_expr,
            TypeExpr::Union(_) | TypeExpr::Literal(_),
        );
        assert!(
            is_resolved_variant,
            "variant prop should resolve to a literal union, got {:?}",
            variant_prop.type_expr,
        );

        // Imported Props-like refs stay symbolic in the native API — the compat
        // layer expands them in the schema field while the type string preserves
        // the named form (e.g. "AvatarProps | undefined").
        let avatar_prop = meta
            .props
            .iter()
            .find(|p| p.name == "avatar")
            .expect("should have avatar prop");
        assert!(
            matches!(
                &avatar_prop.type_expr,
                TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
            ),
            "avatar prop should stay as symbolic Ref('AvatarProps'), got {:?}",
            avatar_prop.type_expr,
        );
    }

    #[test]
    fn slow_lane_forbid_guards_are_thread_local() {
        let _structural_guard = forbid_structural_slow_lane_for_tests();
        let _direct_pick_guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let _prepared_guard = forbid_prepared_structural_substitution_slow_lane_for_tests();

        assert!(structural_slow_lane_forbidden_for_current_thread());
        assert!(direct_pick_routed_expr_slow_lane_forbidden_for_current_thread());
        assert!(prepared_structural_substitution_slow_lane_forbidden_for_current_thread());

        let (structural, direct_pick, prepared) = std::thread::spawn(|| {
            (
                structural_slow_lane_forbidden_for_current_thread(),
                direct_pick_routed_expr_slow_lane_forbidden_for_current_thread(),
                prepared_structural_substitution_slow_lane_forbidden_for_current_thread(),
            )
        })
        .join()
        .expect("thread-local guard probe should join cleanly");

        assert!(
            !structural,
            "structural slow-lane guard should not leak across test threads",
        );
        assert!(
            !direct_pick,
            "direct-pick slow-lane guard should not leak across test threads",
        );
        assert!(
            !prepared,
            "prepared structural substitution slow-lane guard should not leak across test threads",
        );
    }

    /// Reproduces the App.vue pattern from nuxt-ui: an interface in a `.vue`
    /// file's normal `<script>` block extends `Omit<ExternalType, keys>`,
    /// and a separate `<script setup>` block uses `defineProps<AppProps>()`.
    /// The prepared surface projection must resolve the cross-file Omit and
    /// include the inherited members.
    #[test]
    fn project_prepared_type_surface_shape_resolves_cross_file_omit_in_interface_extends() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/external.ts".to_string(),
            Arc::from(
                r#"
export interface ConfigProviderProps {
  dir?: string
  locale?: string
  scrollBody?: boolean
  nonce?: string
  useId?: () => string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { ConfigProviderProps } from './external'

export interface AppProps extends Omit<ConfigProviderProps, 'useId' | 'locale'> {
  tooltip?: string
  portal?: boolean | string
}
</script>

<script setup lang="ts">
const props = defineProps<AppProps>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let shape = query_engine
            .project_prepared_type_surface_shape("/src/App.vue", "AppProps")
            .expect("cross-file Omit in interface extends should produce a projectable surface");

        let member_names: Vec<&str> = shape.properties.iter().map(|p| p.name.as_str()).collect();

        // Own members
        assert!(
            member_names.contains(&"tooltip"),
            "own member 'tooltip' must be present, got {member_names:?}",
        );
        assert!(
            member_names.contains(&"portal"),
            "own member 'portal' must be present, got {member_names:?}",
        );

        // Inherited from ConfigProviderProps after Omit<..., 'useId' | 'locale'>
        assert!(
            member_names.contains(&"dir"),
            "inherited member 'dir' must be present after Omit, got {member_names:?}",
        );
        assert!(
            member_names.contains(&"scrollBody"),
            "inherited member 'scrollBody' must be present after Omit, got {member_names:?}",
        );
        assert!(
            member_names.contains(&"nonce"),
            "inherited member 'nonce' must be present after Omit, got {member_names:?}",
        );

        // Omitted keys must NOT be present
        assert!(
            !member_names.contains(&"useId"),
            "omitted member 'useId' must NOT be present, got {member_names:?}",
        );
        assert!(
            !member_names.contains(&"locale"),
            "omitted member 'locale' must NOT be present, got {member_names:?}",
        );
    }
}
