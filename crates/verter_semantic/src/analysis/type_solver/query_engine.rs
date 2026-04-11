//! `TypeQueryEngine` — request-scoped solver with op-key caching.
//!
//! Replaces per-call `QueryArena` with a single shared arena that persists
//! across all solves in one request. Op-key caching types (`OpKey`, `SubjectKey`)
//! are owned here so request-scoped memoization stays inside the engine.
//!
//! One engine is created per component-meta request (or per one-shot call).
//! It owns the arena and solver caches for the entire request lifetime.

use rustc_hash::FxHashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::arena::{Node, NodeId, QueryArena, SolverCaches};
use super::audit::{AuditSink, NoopAudit, RecordingAudit};
use super::host::{BareRefOrigin, ResolvedRootIdentity, TypeSolverHost, UtilitySource};
use super::lower::lower_type_expr;
use super::project;
use super::result::SolverResult;
use super::solve::{project_to_type_expr, resolve_node, SolveLimits, SolveState};
use super::substitution::SubstitutionEnv;
use crate::analysis::type_expr::TypeExpr;

// ---------------------------------------------------------------------------
// SubjectKey / SubjectId — canonical subject normalization (scaffolding)
// ---------------------------------------------------------------------------

/// Interned handle into `TypeQueryEngine.subjects`. Cheap to copy and hash.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SubjectId(u32);

/// Modifier pair for mapped type optional/readonly changes.
#[allow(dead_code)]
#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
pub struct SurfaceModifiers {
    pub optional: Option<bool>,
    pub readonly: Option<bool>,
}

/// How the mapped type filters its source keyspace.
#[allow(dead_code)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum KeyFilterKind {
    All,
    Include(Vec<String>),
    Exclude(Vec<String>),
    Opaque { filter_hash: u64 },
}

/// Case transform kinds for key remapping.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum CaseTransformKind {
    Capitalize,
    Uncapitalize,
    Uppercase,
    Lowercase,
}

/// How the mapped type remaps key names.
#[allow(dead_code)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum KeyRemapKind {
    Identity,
    Prefix(String),
    Suffix(String),
    CaseTransform(CaseTransformKind),
    Opaque { remap_hash: u64 },
}

/// How the mapped type transforms member values.
#[allow(dead_code)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ValueRuleKind {
    PassThrough,
    Transform { transform_hash: u64 },
}

/// Canonical semantic identity of a type after substitution and normalization.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum SubjectKey {
    /// Canonical declaration subject after applying effective args.
    Decl {
        canonical_id: String,
        symbol_name: String,
        args_hash: u64,
        conditional_ctx_hash: u64,
    },
    /// Pure overlays that reuse the base subject instead of walking again.
    Overlay {
        base: Box<SubjectKey>,
        modifiers: SurfaceModifiers,
    },
    /// Structural mapped transform over a base subject.
    MappedTransform {
        base: Box<SubjectKey>,
        key_filter: KeyFilterKind,
        key_remap: KeyRemapKind,
        value_rule: ValueRuleKind,
        modifiers: SurfaceModifiers,
    },
    /// Inline symbolic subject when there is no declaration identity.
    Symbolic {
        node_hash: u64,
        conditional_ctx_hash: u64,
    },
}

// ---------------------------------------------------------------------------
// OpKey / OpResult — operation-level caching (scaffolding)
// ---------------------------------------------------------------------------

/// Semantic identity for a type operation.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum OpKey {
    #[allow(dead_code)]
    Instantiate {
        canonical_id: String,
        symbol_name: String,
        args_hash: u64,
    },
    ProjectMember {
        subject: SubjectId,
        member: String,
    },
    ProjectKeyspace {
        subject: SubjectId,
    },
    ProjectSurface {
        subject: SubjectId,
    },
    #[allow(dead_code)]
    IndexedAccess {
        object: SubjectId,
        index_hash: u64,
    },
    #[allow(dead_code)]
    Conditional {
        check_hash: u64,
        extends_hash: u64,
        true_hash: u64,
        false_hash: u64,
        distributive: bool,
    },
    #[allow(dead_code)]
    StructuralTransform {
        subject: SubjectId,
        transform_hash: u64,
    },
    #[allow(dead_code)]
    TypeOf {
        canonical_id: String,
        path: Vec<String>,
    },
    TopLevel {
        expr_hash: u64,
        scope_canonical_id: String,
    },
}

/// Cached result of an operation.
struct OpResult {
    result: SolverResult<TypeExpr>,
    visited_decls: Vec<ResolvedRootIdentity>,
}

// ---------------------------------------------------------------------------
// TypeQueryEngine
// ---------------------------------------------------------------------------

/// Request-scoped type solver engine with op-key caching.
///
/// One engine is created per component-meta request. It owns the arena
/// and solver caches. All solves within one request share these resources.
///
/// `TypeQueryEngine` is `&mut self` throughout — not `Send` or `Sync`.
pub struct TypeQueryEngine<'a, A: AuditSink = NoopAudit> {
    host: &'a dyn TypeSolverHost,
    op_cache: FxHashMap<OpKey, OpResult>,
    shallow_field_expr_cache: FxHashMap<TypeExpr, bool>,
    shallow_imported_bare_ref_cache: FxHashMap<String, bool>,
    shallow_transitive_ref_cache: FxHashMap<String, bool>,
    subjects: FxHashMap<SubjectKey, SubjectId>,
    subject_keys: Vec<SubjectKey>,
    next_subject_id: u32,
    /// Shared arena for the entire request.
    arena: QueryArena,
    /// Shared request-scoped instantiation cache.
    instantiation_cache: FxHashMap<super::recursion::RecursionKey, NodeId>,
    /// Shared request-scoped projection cache.
    projection_cache: FxHashMap<super::solve::ProjectionCacheKey, NodeId>,
    /// Shared request-scoped caches (relation, instantiation, keyspace, member).
    caches: SolverCaches,
    /// Trace accumulator: all external decls visited across all solves.
    visited_decls: Vec<ResolvedRootIdentity>,
    /// Request-scoped aggregate of steps spent in uncached solves.
    total_steps: u64,
    /// Number of uncached solves performed by this engine.
    solve_count: u32,
    /// Accumulated hot-path trace counters across all solves.
    pub trace_summary: SolverTraceSummary,
    #[allow(dead_code)]
    audit: A,
}

/// Accumulated solver hot-path counters for tracing/profiling.
#[derive(Debug, Default, Clone)]
pub struct SolverTraceSummary {
    pub resolve_ref_count: u32,
    pub resolve_ref_host_lookups: u32,
    pub resolve_indexed_access_count: u32,
    pub instantiation_cache_hits: u32,
    pub instantiation_cache_misses: u32,
    pub resolve_union_count: u32,
    pub resolve_intersection_count: u32,
    pub resolve_object_count: u32,
    pub resolve_conditional_count: u32,
    pub resolve_mapped_count: u32,
    pub projection_cache_hits: u32,
    pub arena_high_water: u32,
}

impl<'a> TypeQueryEngine<'a, NoopAudit> {
    /// Create a new engine with no audit (default runtime path).
    pub fn new(host: &'a dyn TypeSolverHost) -> Self {
        Self::with_audit(host, NoopAudit)
    }
}

impl<'a> TypeQueryEngine<'a, RecordingAudit> {
    /// Create a new engine with recording audit (test/trace path).
    pub fn new_with_recording(host: &'a dyn TypeSolverHost) -> Self {
        Self::with_audit(host, RecordingAudit::default())
    }
}

impl<'a, A: AuditSink> TypeQueryEngine<'a, A> {
    /// Create a new engine with a specific audit sink.
    pub fn with_audit(host: &'a dyn TypeSolverHost, audit: A) -> Self {
        Self {
            host,
            op_cache: FxHashMap::default(),
            shallow_field_expr_cache: FxHashMap::default(),
            shallow_imported_bare_ref_cache: FxHashMap::default(),
            shallow_transitive_ref_cache: FxHashMap::default(),
            subjects: FxHashMap::default(),
            subject_keys: Vec::new(),
            next_subject_id: 0,
            arena: QueryArena::new(),
            instantiation_cache: FxHashMap::default(),
            projection_cache: FxHashMap::default(),
            caches: SolverCaches::default(),
            visited_decls: Vec::new(),
            total_steps: 0,
            solve_count: 0,
            trace_summary: SolverTraceSummary::default(),
            audit,
        }
    }

    /// Solve a TypeExpr. Returns the resolved type and solver metadata.
    pub fn solve(&mut self, expr: &TypeExpr) -> SolverResult<TypeExpr> {
        self.solve_with_trace(expr).0
    }

    pub fn should_preserve_imported_bare_ref(&mut self, expr: &TypeExpr) -> bool {
        fn is_package_canonical(canonical_id: &str) -> bool {
            canonical_id.contains("/node_modules/") || canonical_id.contains("\\node_modules\\")
        }

        fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
            match expr {
                TypeExpr::Parenthesized(inner) => strip_parens(inner),
                other => other,
            }
        }

        let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens(expr)
        else {
            return false;
        };
        if !type_arguments.is_empty() {
            return false;
        }
        if let Some(cached) = self.shallow_imported_bare_ref_cache.get(name.as_ref()) {
            return *cached;
        }

        let preserve = if self.host.bare_ref_origin(name.as_ref()) != BareRefOrigin::Imported {
            false
        } else {
            let Some(root_identity) = self.host.root_identity("", name.as_ref()) else {
                self.shallow_imported_bare_ref_cache
                    .insert(name.to_string(), false);
                return false;
            };
            if is_package_canonical(&root_identity.canonical_id) {
                self.shallow_imported_bare_ref_cache
                    .insert(name.to_string(), true);
                return true;
            }
            let Some(prepared) = self.host.resolve_prepared_type_decl(&root_identity) else {
                self.shallow_imported_bare_ref_cache
                    .insert(name.to_string(), false);
                return false;
            };

            matches!(
                prepared.projection_class,
                super::prepared::PreparedProjectionClass::DirectMembers
            ) || matches!(
                prepared.kind,
                crate::analysis::type_eval::TypeDeclKind::Class
            )
        };

        self.shallow_imported_bare_ref_cache
            .insert(name.to_string(), preserve);
        preserve
    }

    fn should_preserve_imported_utility_route(&mut self, expr: &TypeExpr) -> bool {
        fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
            match expr {
                TypeExpr::Parenthesized(inner) => strip_parens(inner),
                other => other,
            }
        }

        fn imported_value_route_arg<A: super::audit::AuditSink>(
            engine: &TypeQueryEngine<'_, A>,
            expr: &TypeExpr,
        ) -> bool {
            match strip_parens(expr) {
                TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef { path }) => {
                    path.first().is_some_and(|root| {
                        engine.host.bare_ref_origin(root) == BareRefOrigin::Imported
                    })
                }
                TypeExpr::Parenthesized(inner) => imported_value_route_arg(engine, inner),
                _ => false,
            }
        }

        match strip_parens(expr) {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if matches!(
                self.host.utility_source(name.as_ref()),
                UtilitySource::Builtin
            ) && !type_arguments.is_empty() =>
            {
                type_arguments.iter().any(|argument| {
                    self.should_preserve_imported_bare_ref(argument)
                        || imported_value_route_arg(self, argument)
                        || self.should_preserve_package_member_path(argument)
                        || self.should_preserve_imported_utility_route(argument)
                })
            }
            _ => false,
        }
    }

    fn should_preserve_imported_member_path(&mut self, expr: &TypeExpr) -> bool {
        fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
            match expr {
                TypeExpr::Parenthesized(inner) => strip_parens(inner),
                other => other,
            }
        }

        fn root_import_name(expr: &TypeExpr) -> Option<&str> {
            match strip_parens(expr) {
                TypeExpr::IndexedAccess { object, .. } => root_import_name(object),
                TypeExpr::Ref {
                    name,
                    type_arguments: _,
                } => Some(name.as_ref()),
                _ => None,
            }
        }

        let TypeExpr::IndexedAccess { object, .. } = strip_parens(expr) else {
            return false;
        };
        let Some(name) = root_import_name(object) else {
            return false;
        };
        if self.host.bare_ref_origin(name) != BareRefOrigin::Imported {
            return false;
        }
        self.host.root_identity("", name).is_some()
    }

    fn should_preserve_package_member_path(&mut self, expr: &TypeExpr) -> bool {
        fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
            match expr {
                TypeExpr::Parenthesized(inner) => strip_parens(inner),
                other => other,
            }
        }

        fn root_import_name(expr: &TypeExpr) -> Option<&str> {
            match strip_parens(expr) {
                TypeExpr::IndexedAccess { object, .. } => root_import_name(object),
                TypeExpr::Ref {
                    name,
                    type_arguments: _,
                } => Some(name.as_ref()),
                _ => None,
            }
        }

        let Some(name) = root_import_name(expr) else {
            return false;
        };
        if self.host.bare_ref_origin(name) != BareRefOrigin::Imported {
            return false;
        }
        let Some(root_identity) = self.host.root_identity("", name) else {
            return false;
        };
        root_identity.canonical_id.contains("/node_modules/")
            || root_identity.canonical_id.contains("\\node_modules\\")
    }

    fn should_preserve_transitive_ref(
        &mut self,
        name: &str,
        active_exprs: &mut rustc_hash::FxHashSet<TypeExpr>,
        active_refs: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        let Some(root_identity) = self.host.root_identity("", name) else {
            return false;
        };
        let cache_key = format!(
            "{}::{}",
            root_identity.canonical_id, root_identity.symbol_name
        );
        if let Some(cached) = self.shallow_transitive_ref_cache.get(&cache_key) {
            return *cached;
        }
        if root_identity.canonical_id.contains("/node_modules/")
            || root_identity.canonical_id.contains("\\node_modules\\")
        {
            self.shallow_transitive_ref_cache
                .insert(cache_key.clone(), true);
            return true;
        }
        if !active_refs.insert(cache_key.clone()) {
            return false;
        }

        let preserve = self
            .host
            .resolve_prepared_type_decl(&root_identity)
            .is_some_and(|prepared| {
                if matches!(prepared.body, TypeExpr::TypeParameter(_)) {
                    true
                } else {
                    Self::should_preserve_shallow_field_expr_inner(
                        self,
                        &prepared.body,
                        active_exprs,
                        active_refs,
                    )
                }
            });

        active_refs.remove(&cache_key);
        self.shallow_transitive_ref_cache
            .insert(cache_key, preserve);
        preserve
    }

    pub fn try_fast_shallow_field_expr(
        &mut self,
        expr: &TypeExpr,
    ) -> Option<SolverResult<TypeExpr>> {
        fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
            match expr {
                TypeExpr::Parenthesized(inner) => strip_parens(inner),
                other => other,
            }
        }

        fn single_member_import_root(expr: &TypeExpr) -> Option<(&str, &str)> {
            let TypeExpr::IndexedAccess { object, index } = strip_parens(expr) else {
                return None;
            };
            let TypeExpr::Ref {
                name,
                type_arguments,
            } = strip_parens(object)
            else {
                return None;
            };
            if !type_arguments.is_empty() {
                return None;
            }
            let TypeExpr::Literal(crate::analysis::type_expr::LiteralValue::String(member_name)) =
                strip_parens(index)
            else {
                return None;
            };
            Some((name.as_ref(), member_name.as_str()))
        }

        fn fast_symbolic_imported_generic_route<A: super::audit::AuditSink>(
            engine: &TypeQueryEngine<'_, A>,
            expr: &TypeExpr,
            active_locals: &mut rustc_hash::FxHashSet<String>,
        ) -> bool {
            match strip_parens(expr) {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => match engine.host.bare_ref_origin(name.as_ref()) {
                    BareRefOrigin::Imported => !type_arguments.is_empty(),
                    BareRefOrigin::Local if type_arguments.is_empty() => {
                        if !active_locals.insert(name.to_string()) {
                            return false;
                        }
                        let preserve = engine
                            .host
                            .root_identity("", name.as_ref())
                            .and_then(|root_identity| {
                                engine.host.resolve_prepared_type_decl(&root_identity)
                            })
                            .is_some_and(|prepared| {
                                fast_symbolic_imported_generic_route(
                                    engine,
                                    &prepared.body,
                                    active_locals,
                                )
                            });
                        active_locals.remove(name.as_ref());
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
                | TypeExpr::Parenthesized(object) => {
                    fast_symbolic_imported_generic_route(engine, object, active_locals)
                }
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    fast_symbolic_imported_generic_route(engine, &element.ty, active_locals)
                }),
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        fast_symbolic_imported_generic_route(engine, member, active_locals)
                    })
                }
                _ => false,
            }
        }

        fn contains_direct_imported_utility_route<A: super::audit::AuditSink>(
            engine: &TypeQueryEngine<'_, A>,
            expr: &TypeExpr,
        ) -> bool {
            fn strip_parens(expr: &TypeExpr) -> &TypeExpr {
                match expr {
                    TypeExpr::Parenthesized(inner) => strip_parens(inner),
                    other => other,
                }
            }

            fn imported_value_route_arg<A: super::audit::AuditSink>(
                engine: &TypeQueryEngine<'_, A>,
                expr: &TypeExpr,
            ) -> bool {
                match strip_parens(expr) {
                    TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef { path }) => {
                        path.first().is_some_and(|root| {
                            engine.host.bare_ref_origin(root) == BareRefOrigin::Imported
                        })
                    }
                    TypeExpr::Parenthesized(inner) => imported_value_route_arg(engine, inner),
                    _ => false,
                }
            }

            fn imported_route_arg<A: super::audit::AuditSink>(
                engine: &TypeQueryEngine<'_, A>,
                expr: &TypeExpr,
            ) -> bool {
                match strip_parens(expr) {
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        (type_arguments.is_empty()
                            && engine.host.bare_ref_origin(name.as_ref())
                                == BareRefOrigin::Imported)
                            || imported_value_route_arg(engine, expr)
                            || contains_direct_imported_utility_route(engine, expr)
                    }
                    TypeExpr::IndexedAccess { object, .. } => imported_route_arg(engine, object),
                    TypeExpr::TypeOf(_) => imported_value_route_arg(engine, expr),
                    TypeExpr::Parenthesized(inner) => imported_route_arg(engine, inner),
                    _ => contains_direct_imported_utility_route(engine, expr),
                }
            }

            match strip_parens(expr) {
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => members
                    .iter()
                    .any(|member| contains_direct_imported_utility_route(engine, member)),
                TypeExpr::Array { element, .. }
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => {
                    contains_direct_imported_utility_route(engine, element)
                }
                TypeExpr::Tuple { elements, .. } => elements
                    .iter()
                    .any(|element| contains_direct_imported_utility_route(engine, &element.ty)),
                TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                    crate::analysis::type_expr::ObjectMember::Property(property) => {
                        contains_direct_imported_utility_route(engine, &property.ty)
                    }
                    crate::analysis::type_expr::ObjectMember::Method(method) => {
                        method.function.parameters.iter().any(|parameter| {
                            contains_direct_imported_utility_route(engine, &parameter.ty)
                        }) || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| {
                                contains_direct_imported_utility_route(engine, return_type)
                            })
                    }
                    crate::analysis::type_expr::ObjectMember::CallSignature(function)
                    | crate::analysis::type_expr::ObjectMember::ConstructSignature(function) => {
                        function.parameters.iter().any(|parameter| {
                            contains_direct_imported_utility_route(engine, &parameter.ty)
                        }) || function.return_type.as_deref().is_some_and(|return_type| {
                            contains_direct_imported_utility_route(engine, return_type)
                        })
                    }
                    crate::analysis::type_expr::ObjectMember::IndexSignature(index) => {
                        contains_direct_imported_utility_route(engine, &index.key_type)
                            || contains_direct_imported_utility_route(engine, &index.value_type)
                    }
                }),
                TypeExpr::Function(function) => {
                    function.parameters.iter().any(|parameter| {
                        contains_direct_imported_utility_route(engine, &parameter.ty)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        contains_direct_imported_utility_route(engine, return_type)
                    })
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if !type_arguments.is_empty()
                    && matches!(
                        engine.host.utility_source(name.as_ref()),
                        UtilitySource::Builtin
                    ) =>
                {
                    type_arguments
                        .iter()
                        .any(|argument| imported_route_arg(engine, argument))
                }
                _ => false,
            }
        }

        if contains_direct_imported_utility_route(self, expr) {
            return Some(SolverResult::exact_symbolic(expr.clone()));
        }

        if let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens(expr)
        {
            if !type_arguments.is_empty()
                && self.host.bare_ref_origin(name.as_ref()) == BareRefOrigin::Imported
            {
                let _ = self.host.root_identity("", name.as_ref())?;
                return Some(SolverResult::exact_symbolic(expr.clone()));
            }
        }

        if let Some((root_name, member_name)) = single_member_import_root(expr) {
            if self.host.bare_ref_origin(root_name) == BareRefOrigin::Imported {
                let root_identity = self.host.root_identity("", root_name)?;
                if root_identity.canonical_id.contains("/node_modules/")
                    || root_identity.canonical_id.contains("\\node_modules\\")
                {
                    return Some(SolverResult::exact_symbolic(expr.clone()));
                }
                let prepared = self.host.resolve_prepared_type_decl(&root_identity)?;
                let member = prepared.member(member_name)?;
                if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                    return None;
                }
                return Some(SolverResult::exact_concrete(member.ty.clone()));
            }
        }

        let mut active_locals = rustc_hash::FxHashSet::default();
        fast_symbolic_imported_generic_route(self, expr, &mut active_locals)
            .then(|| SolverResult::exact_symbolic(expr.clone()))
    }

    fn should_preserve_shallow_field_expr_inner(
        engine: &mut TypeQueryEngine<'_, A>,
        expr: &TypeExpr,
        active_exprs: &mut rustc_hash::FxHashSet<TypeExpr>,
        active_refs: &mut rustc_hash::FxHashSet<String>,
    ) -> bool {
        if let Some(cached) = engine.shallow_field_expr_cache.get(expr) {
            return *cached;
        }
        if !active_exprs.insert(expr.clone()) {
            return false;
        }

        let preserve = if engine.should_preserve_imported_bare_ref(expr) {
            true
        } else if engine.should_preserve_imported_member_path(expr) {
            true
        } else if engine.should_preserve_imported_utility_route(expr) {
            true
        } else if engine.should_preserve_package_member_path(expr) {
            true
        } else {
            match expr {
                TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
                    members.iter().any(|member| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            member,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::Array { element, .. }
                | TypeExpr::KeyOf(element)
                | TypeExpr::Rest(element)
                | TypeExpr::Parenthesized(element) => {
                    Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        element,
                        active_exprs,
                        active_refs,
                    )
                }
                TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                    Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        &element.ty,
                        active_exprs,
                        active_refs,
                    )
                }),
                TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                    crate::analysis::type_expr::ObjectMember::Property(property) => {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            &property.ty,
                            active_exprs,
                            active_refs,
                        )
                    }
                    crate::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            &signature.key_type,
                            active_exprs,
                            active_refs,
                        ) || Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            &signature.value_type,
                            active_exprs,
                            active_refs,
                        )
                    }
                    crate::analysis::type_expr::ObjectMember::CallSignature(function)
                    | crate::analysis::type_expr::ObjectMember::ConstructSignature(function) => {
                        function.parameters.iter().any(|parameter| {
                            Self::should_preserve_shallow_field_expr_inner(
                                engine,
                                &parameter.ty,
                                active_exprs,
                                active_refs,
                            )
                        }) || function.return_type.as_deref().is_some_and(|return_type| {
                            Self::should_preserve_shallow_field_expr_inner(
                                engine,
                                return_type,
                                active_exprs,
                                active_refs,
                            )
                        })
                    }
                    crate::analysis::type_expr::ObjectMember::Method(method) => {
                        method.function.parameters.iter().any(|parameter| {
                            Self::should_preserve_shallow_field_expr_inner(
                                engine,
                                &parameter.ty,
                                active_exprs,
                                active_refs,
                            )
                        }) || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| {
                                Self::should_preserve_shallow_field_expr_inner(
                                    engine,
                                    return_type,
                                    active_exprs,
                                    active_refs,
                                )
                            })
                    }
                }),
                TypeExpr::Function(function) => {
                    function.parameters.iter().any(|parameter| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            &parameter.ty,
                            active_exprs,
                            active_refs,
                        )
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            return_type,
                            active_exprs,
                            active_refs,
                        )
                    }) || function.type_parameters.iter().any(|parameter| {
                        parameter.constraint.as_deref().is_some_and(|constraint| {
                            Self::should_preserve_shallow_field_expr_inner(
                                engine,
                                constraint,
                                active_exprs,
                                active_refs,
                            )
                        }) || parameter.default.as_deref().is_some_and(|default| {
                            Self::should_preserve_shallow_field_expr_inner(
                                engine,
                                default,
                                active_exprs,
                                active_refs,
                            )
                        })
                    })
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    (matches!(
                        engine.host.utility_source(name.as_ref()),
                        UtilitySource::Builtin
                    ) || !type_arguments.is_empty())
                        && type_arguments.iter().any(|argument| {
                            Self::should_preserve_shallow_field_expr_inner(
                                engine,
                                argument,
                                active_exprs,
                                active_refs,
                            )
                        })
                        || engine.should_preserve_transitive_ref(
                            name.as_ref(),
                            active_exprs,
                            active_refs,
                        )
                }
                TypeExpr::TypeParameter(parameter) => {
                    parameter.constraint.as_deref().is_some_and(|constraint| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            constraint,
                            active_exprs,
                            active_refs,
                        )
                    }) || parameter.default.as_deref().is_some_and(|default| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            default,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::IndexedAccess { object, index } => {
                    Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        object,
                        active_exprs,
                        active_refs,
                    ) || Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        index,
                        active_exprs,
                        active_refs,
                    )
                }
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => {
                    Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        check,
                        active_exprs,
                        active_refs,
                    ) || Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        extends,
                        active_exprs,
                        active_refs,
                    ) || Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        true_type,
                        active_exprs,
                        active_refs,
                    ) || Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        false_type,
                        active_exprs,
                        active_refs,
                    )
                }
                TypeExpr::Mapped {
                    source,
                    value,
                    name_type,
                    ..
                } => {
                    Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        source,
                        active_exprs,
                        active_refs,
                    ) || Self::should_preserve_shallow_field_expr_inner(
                        engine,
                        value,
                        active_exprs,
                        active_refs,
                    ) || name_type.as_deref().is_some_and(|name_type| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            name_type,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::TemplateLiteral { expressions, .. } => {
                    expressions.iter().any(|expression| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            expression,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::RecursiveRef { type_arguments, .. } => {
                    type_arguments.iter().any(|argument| {
                        Self::should_preserve_shallow_field_expr_inner(
                            engine,
                            argument,
                            active_exprs,
                            active_refs,
                        )
                    })
                }
                TypeExpr::Primitive(_)
                | TypeExpr::Literal(_)
                | TypeExpr::TypeOf(_)
                | TypeExpr::Infer { .. }
                | TypeExpr::Unknown { .. } => false,
            }
        };

        active_exprs.remove(expr);
        engine
            .shallow_field_expr_cache
            .insert(expr.clone(), preserve);
        preserve
    }

    pub fn should_preserve_shallow_field_expr(&mut self, expr: &TypeExpr) -> bool {
        let mut active_exprs = rustc_hash::FxHashSet::default();
        let mut active_refs = rustc_hash::FxHashSet::default();
        Self::should_preserve_shallow_field_expr_inner(
            self,
            expr,
            &mut active_exprs,
            &mut active_refs,
        )
    }

    /// Solve while keeping package-backed prepared refs symbolic.
    pub fn solve_preserving_package_refs(&mut self, expr: &TypeExpr) -> SolverResult<TypeExpr> {
        self.solve_with_trace_preserving_package_refs(expr).0
    }

    /// Solve and return trace (for Phase 1 macro expansion).
    pub fn solve_with_trace(
        &mut self,
        expr: &TypeExpr,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        self.solve_with_trace_internal(expr, false)
    }

    /// Solve and return trace while keeping package-backed prepared refs symbolic.
    pub fn solve_with_trace_preserving_package_refs(
        &mut self,
        expr: &TypeExpr,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        self.solve_with_trace_internal(expr, true)
    }

    fn solve_with_trace_internal(
        &mut self,
        expr: &TypeExpr,
        preserve_package_symbolic_refs: bool,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        let top_level_key = OpKey::TopLevel {
            expr_hash: hash_expr(expr),
            scope_canonical_id: if preserve_package_symbolic_refs {
                "__preserve_package_refs__".to_string()
            } else {
                String::new()
            },
        };
        if let Some(cached) = self.op_cache.get(&top_level_key) {
            self.audit.op_cache_hit("TopLevel");
            self.visited_decls
                .extend(cached.visited_decls.iter().cloned());
            return (cached.result.clone(), cached.visited_decls.clone());
        }
        self.audit.op_cache_miss("TopLevel");

        let mut state = if preserve_package_symbolic_refs {
            SolveState::with_caches(
                SolveLimits::default(),
                FxHashMap::default(),
                SolverCaches::default(),
            )
        } else {
            SolveState::with_caches(
                SolveLimits::default(),
                std::mem::take(&mut self.instantiation_cache),
                std::mem::take(&mut self.caches),
            )
        };
        state.preserve_package_symbolic_refs = preserve_package_symbolic_refs;
        state.projection_cache = if preserve_package_symbolic_refs {
            FxHashMap::default()
        } else {
            std::mem::take(&mut self.projection_cache)
        };
        let root = lower_type_expr(&mut self.arena, expr);
        let resolved = resolve_node(
            &mut self.arena,
            root,
            self.host,
            &mut state,
            &SubstitutionEnv::new(),
        );
        let result_expr = project_to_type_expr(&self.arena, resolved);

        let visited = std::mem::take(&mut state.visited_external_decls);
        self.visited_decls.extend(visited.iter().cloned());
        if !preserve_package_symbolic_refs {
            self.instantiation_cache = std::mem::take(&mut state.instantiation_cache);
            self.projection_cache = std::mem::take(&mut state.projection_cache);
            self.caches = std::mem::take(&mut state.relation_caches);
        }
        self.accumulate_trace_summary(&state);

        let result = SolverResult {
            value: result_expr,
            exactness: state.exactness,
            execution_status: state.execution_status,
            incomplete_reasons: state.incomplete_reasons,
            diagnostics: state.diagnostics,
            steps: state.steps,
        };
        self.total_steps += result.steps;
        self.solve_count += 1;

        self.op_cache.insert(
            top_level_key,
            OpResult {
                result: result.clone(),
                visited_decls: visited.clone(),
            },
        );

        (result, visited)
    }

    /// Solve using a different (scoped) host while sharing engine state.
    ///
    /// The `scope_canonical_id` partitions the op-cache key and the bare-name
    /// root_identity cache so results from one declaration scope do not alias
    /// with results from another scope in the same request-scoped engine.
    pub fn solve_scoped(
        &mut self,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        self.solve_scoped_internal(scoped_host, scope_canonical_id, expr, false)
    }

    pub fn solve_scoped_preserving_package_refs(
        &mut self,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        self.solve_scoped_internal(scoped_host, scope_canonical_id, expr, true)
    }

    fn solve_scoped_internal(
        &mut self,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        preserve_package_symbolic_refs: bool,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        let top_level_key = OpKey::TopLevel {
            expr_hash: hash_expr(expr),
            scope_canonical_id: if preserve_package_symbolic_refs {
                format!("{scope_canonical_id}::__preserve_package_refs__")
            } else {
                scope_canonical_id.to_string()
            },
        };
        if let Some(cached) = self.op_cache.get(&top_level_key) {
            self.audit.op_cache_hit("TopLevel_scoped");
            self.visited_decls
                .extend(cached.visited_decls.iter().cloned());
            return (cached.result.clone(), cached.visited_decls.clone());
        }
        self.audit.op_cache_miss("TopLevel_scoped");

        let mut state = if preserve_package_symbolic_refs {
            SolveState::with_caches_and_scope(
                SolveLimits::default(),
                FxHashMap::default(),
                SolverCaches::default(),
                scope_canonical_id.to_string(),
            )
        } else {
            SolveState::with_caches_and_scope(
                SolveLimits::default(),
                std::mem::take(&mut self.instantiation_cache),
                std::mem::take(&mut self.caches),
                scope_canonical_id.to_string(),
            )
        };
        state.preserve_package_symbolic_refs = preserve_package_symbolic_refs;
        state.projection_cache = if preserve_package_symbolic_refs {
            FxHashMap::default()
        } else {
            std::mem::take(&mut self.projection_cache)
        };
        let root = lower_type_expr(&mut self.arena, expr);
        let resolved = resolve_node(
            &mut self.arena,
            root,
            scoped_host,
            &mut state,
            &SubstitutionEnv::new(),
        );
        let result_expr = project_to_type_expr(&self.arena, resolved);

        let visited = std::mem::take(&mut state.visited_external_decls);
        self.visited_decls.extend(visited.iter().cloned());
        if !preserve_package_symbolic_refs {
            self.instantiation_cache = std::mem::take(&mut state.instantiation_cache);
            self.projection_cache = std::mem::take(&mut state.projection_cache);
            self.caches = std::mem::take(&mut state.relation_caches);
        }
        self.accumulate_trace_summary(&state);

        let result = SolverResult {
            value: result_expr,
            exactness: state.exactness,
            execution_status: state.execution_status,
            incomplete_reasons: state.incomplete_reasons,
            diagnostics: state.diagnostics,
            steps: state.steps,
        };
        self.total_steps += result.steps;
        self.solve_count += 1;

        self.op_cache.insert(
            top_level_key,
            OpResult {
                result: result.clone(),
                visited_decls: visited.clone(),
            },
        );

        (result, visited)
    }

    /// Get accumulated visited decls across all solves.
    pub fn visited_decls(&self) -> &[ResolvedRootIdentity] {
        &self.visited_decls
    }

    /// Number of memoized top-level entries in this request-scoped engine.
    pub fn cache_len(&self) -> usize {
        self.op_cache.len()
    }

    /// Total resolve steps spent in uncached solves for this request-scoped engine.
    pub fn total_steps(&self) -> u64 {
        self.total_steps
    }

    /// Number of uncached solves performed by this request-scoped engine.
    pub fn solve_count(&self) -> u32 {
        self.solve_count
    }

    /// Get the audit sink.
    pub fn audit(&self) -> &A {
        &self.audit
    }

    fn resolve_expr_node_scoped(
        &mut self,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> ResolvedNodeResult {
        let mut state = SolveState::with_caches_and_scope(
            SolveLimits::default(),
            std::mem::take(&mut self.instantiation_cache),
            std::mem::take(&mut self.caches),
            scope_canonical_id.to_string(),
        );
        state.projection_cache = std::mem::take(&mut self.projection_cache);
        let root = lower_type_expr(&mut self.arena, expr);
        let resolved = resolve_node(
            &mut self.arena,
            root,
            scoped_host,
            &mut state,
            &SubstitutionEnv::new(),
        );

        let visited = std::mem::take(&mut state.visited_external_decls);
        self.visited_decls.extend(visited);
        self.instantiation_cache = std::mem::take(&mut state.instantiation_cache);
        self.projection_cache = std::mem::take(&mut state.projection_cache);
        self.caches = std::mem::take(&mut state.relation_caches);
        self.accumulate_trace_summary(&state);
        self.total_steps += state.steps;
        self.solve_count += 1;

        ResolvedNodeResult {
            node: resolved,
            exactness: state.exactness,
        }
    }

    // -----------------------------------------------------------------------
    // Subject interning and projection operators
    // -----------------------------------------------------------------------

    /// Intern a subject key, returning a stable `SubjectId` for this request.
    /// Same key → same id within one engine lifetime.
    pub fn intern_subject(&mut self, key: SubjectKey) -> SubjectId {
        if let Some(&id) = self.subjects.get(&key) {
            return id;
        }
        let id = SubjectId(self.next_subject_id);
        self.next_subject_id += 1;
        self.subjects.insert(key.clone(), id);
        self.subject_keys.push(key);
        id
    }

    /// Retrieve the `SubjectKey` for a previously interned `SubjectId`.
    pub fn subject_key(&self, id: SubjectId) -> Option<&SubjectKey> {
        self.subject_keys.get(id.0 as usize)
    }

    /// Project a single member from a subject by name.
    ///
    /// For `SubjectKey::Decl`: resolves the declaration, then projects the named member.
    /// For `SubjectKey::MappedTransform`: binds the mapped parameter to the
    /// specific key and evaluates the value expression once.
    pub fn project_member(
        &mut self,
        subject: SubjectId,
        member_name: &str,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
    ) -> Option<ProjectedMember> {
        let op_key = OpKey::ProjectMember {
            subject,
            member: member_name.to_string(),
        };
        if let Some(cached) = self.op_cache.get(&op_key) {
            return extract_member_from_type_expr(&cached.result.value, member_name);
        }

        let key = self.subject_key(subject)?.clone();
        match &key {
            SubjectKey::Decl {
                canonical_id: _,
                symbol_name,
                ..
            } => {
                let visited_start = self.visited_decls.len();
                let type_ref = TypeExpr::named(symbol_name);
                let resolved =
                    self.resolve_expr_node_scoped(scoped_host, scope_canonical_id, &type_ref);
                if !resolved.exactness.is_exact() {
                    return None;
                }
                let projected_member = project::project_member(
                    &mut self.arena,
                    &mut self.caches,
                    resolved.node,
                    member_name,
                );
                if projected_member.exactness.is_exact() {
                    if let Some(ty) = projected_member.value {
                        let (optional, readonly, is_method) = match self.arena.get(resolved.node) {
                            Node::Object(object) => object
                                .properties
                                .iter()
                                .find(|property| property.name == member_name)
                                .map(|property| {
                                    (property.optional, property.readonly, property.is_method)
                                })
                                .unwrap_or((false, false, false)),
                            _ => (false, false, false),
                        };
                        let member = ProjectedMember {
                            name: member_name.to_string(),
                            ty: project_to_type_expr(&self.arena, ty),
                            optional,
                            readonly,
                            is_method,
                        };
                        let result_expr = projected_member_to_type_expr(&member);
                        let visited_decls = self.visited_decls[visited_start..].to_vec();
                        self.op_cache.insert(
                            op_key,
                            OpResult {
                                result: SolverResult::exact_concrete(result_expr),
                                visited_decls,
                            },
                        );
                        return Some(member);
                    }
                    return None;
                }
                let surface = project::project_surface(&self.arena, resolved.node);
                if !surface.exactness.is_exact() {
                    return None;
                }
                let member = surface
                    .value
                    .properties
                    .iter()
                    .find(|property| property.name == member_name)
                    .map(|member| ProjectedMember {
                        name: member.name.clone(),
                        ty: project_to_type_expr(&self.arena, member.ty),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    });
                if let Some(ref member) = member {
                    let result_expr = projected_member_to_type_expr(member);
                    let visited_decls = self.visited_decls[visited_start..].to_vec();
                    self.op_cache.insert(
                        op_key,
                        OpResult {
                            result: SolverResult::exact_concrete(result_expr),
                            visited_decls,
                        },
                    );
                }
                member
            }
            _ => None, // Other subject kinds not yet implemented
        }
    }

    /// Project the keyspace (set of member names) from a subject.
    pub fn project_keyspace(
        &mut self,
        subject: SubjectId,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
    ) -> Option<ProjectedKeyspace> {
        let op_key = OpKey::ProjectKeyspace { subject };
        if let Some(cached) = self.op_cache.get(&op_key) {
            return extract_keyspace_from_type_expr(&cached.result.value);
        }

        let key = self.subject_key(subject)?.clone();
        match &key {
            SubjectKey::Decl { symbol_name, .. } => {
                let visited_start = self.visited_decls.len();
                let type_ref = TypeExpr::named(symbol_name);
                let resolved =
                    self.resolve_expr_node_scoped(scoped_host, scope_canonical_id, &type_ref);
                if !resolved.exactness.is_exact() {
                    return None;
                }
                let projected =
                    project::project_keyspace(&self.arena, &mut self.caches, resolved.node);
                if !projected.exactness.is_exact() {
                    return None;
                }
                let keyspace = projected_keyspace_from_result(&projected.value);
                if let Some(result_expr) = projected_keyspace_to_type_expr(&keyspace) {
                    let visited_decls = self.visited_decls[visited_start..].to_vec();
                    self.op_cache.insert(
                        op_key,
                        OpResult {
                            result: SolverResult::exact_concrete(result_expr),
                            visited_decls,
                        },
                    );
                }
                Some(keyspace)
            }
            _ => None,
        }
    }

    /// Project the full surface (all members) from a subject.
    pub fn project_surface(
        &mut self,
        subject: SubjectId,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
    ) -> Option<ProjectedSurface> {
        let op_key = OpKey::ProjectSurface { subject };
        if let Some(cached) = self.op_cache.get(&op_key) {
            return extract_surface_from_type_expr(&cached.result.value);
        }

        let key = self.subject_key(subject)?.clone();
        match &key {
            SubjectKey::Decl { symbol_name, .. } => {
                let type_ref = TypeExpr::named(symbol_name);
                let visited_start = self.visited_decls.len();
                let projected =
                    self.project_expr_surface(scoped_host, scope_canonical_id, &type_ref);
                if let Some(ref surface) = projected {
                    if let Some(result_expr) = projected_surface_to_type_expr(surface) {
                        let visited_decls = self.visited_decls[visited_start..].to_vec();
                        self.op_cache.insert(
                            op_key,
                            OpResult {
                                result: SolverResult::exact_concrete(result_expr),
                                visited_decls,
                            },
                        );
                    }
                }
                projected
            }
            _ => None,
        }
    }

    pub fn project_expr_surface(
        &mut self,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<ProjectedSurface> {
        let resolved = self.resolve_expr_node_scoped(scoped_host, scope_canonical_id, expr);
        if !resolved.exactness.is_exact() {
            return None;
        }
        let projected = project::project_surface(&self.arena, resolved.node);
        if !projected.exactness.is_exact() {
            return None;
        }
        projected_surface_from_shape(&self.arena, &projected.value)
    }

    pub fn project_expr_surface_as_type_expr(
        &mut self,
        scoped_host: &dyn TypeSolverHost,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let surface = self.project_expr_surface(scoped_host, scope_canonical_id, expr)?;
        projected_surface_to_type_expr(&surface)
    }

    fn accumulate_trace_summary(&mut self, state: &super::solve::SolveState) {
        let s = &mut self.trace_summary;
        s.resolve_ref_count += state.resolve_ref_count;
        s.resolve_ref_host_lookups += state.resolve_ref_host_lookups;
        s.resolve_indexed_access_count += state.resolve_indexed_access_count;
        s.instantiation_cache_hits += state.instantiation_cache_hits;
        s.instantiation_cache_misses += state.instantiation_cache_misses;
        s.resolve_union_count += state.resolve_union_count;
        s.resolve_intersection_count += state.resolve_intersection_count;
        s.resolve_object_count += state.resolve_object_count;
        s.resolve_conditional_count += state.resolve_conditional_count;
        s.resolve_mapped_count += state.resolve_mapped_count;
        s.projection_cache_hits += state.projection_cache_hits;
        let arena_len = self.arena.len() as u32;
        if arena_len > s.arena_high_water {
            s.arena_high_water = arena_len;
        }
    }
}

fn hash_expr(expr: &TypeExpr) -> u64 {
    let mut hasher = DefaultHasher::new();
    expr.hash(&mut hasher);
    hasher.finish()
}

struct ResolvedNodeResult {
    node: NodeId,
    #[allow(dead_code)]
    exactness: super::result::SolverExactness,
}

// ---------------------------------------------------------------------------
// Projection result types
// ---------------------------------------------------------------------------

/// A single projected member from a type surface.
#[derive(Debug, Clone)]
pub struct ProjectedMember {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
}

/// The projected keyspace of a type surface — the set of known member names.
#[derive(Debug, Clone)]
pub struct ProjectedKeyspace {
    /// Concrete member names (from object properties, mapped finite keys).
    pub members: Vec<String>,
    /// Whether the keyspace also includes an open index signature.
    pub has_index_signature: bool,
}

/// The full projected surface of a type — all concrete members.
#[derive(Debug, Clone)]
pub struct ProjectedSurface {
    pub members: Vec<ProjectedMember>,
    /// Call signatures (for callable emits).
    pub call_signatures: Vec<TypeExpr>,
    /// Construct signatures.
    pub construct_signatures: Vec<TypeExpr>,
    /// Whether the surface includes an open index signature.
    pub has_index_signature: bool,
}

// ---------------------------------------------------------------------------
// Projection extraction helpers
// ---------------------------------------------------------------------------

/// Extract a named member from a solved TypeExpr (typically an Object).
fn extract_member_from_type_expr(expr: &TypeExpr, member_name: &str) -> Option<ProjectedMember> {
    use crate::analysis::type_expr::ObjectMember;
    match expr {
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) if prop.name == member_name => {
                        return Some(ProjectedMember {
                            name: prop.name.clone(),
                            ty: prop.ty.clone(),
                            optional: prop.optional,
                            readonly: prop.readonly,
                            is_method: false,
                        });
                    }
                    ObjectMember::Method(method) if method.name == member_name => {
                        return Some(ProjectedMember {
                            name: method.name.clone(),
                            ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                            optional: method.optional,
                            readonly: false,
                            is_method: true,
                        });
                    }
                    _ => {}
                }
            }
            None
        }
        TypeExpr::Intersection(members) => {
            for m in members.iter() {
                if let Some(found) = extract_member_from_type_expr(m, member_name) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract the keyspace (member name list) from a solved TypeExpr.
fn extract_keyspace_from_type_expr(expr: &TypeExpr) -> Option<ProjectedKeyspace> {
    use crate::analysis::type_expr::ObjectMember;
    match expr {
        TypeExpr::Object(obj) => {
            let mut members = Vec::new();
            let mut has_index = false;
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => members.push(prop.name.clone()),
                    ObjectMember::Method(method) => members.push(method.name.clone()),
                    ObjectMember::IndexSignature(_) => has_index = true,
                    _ => {}
                }
            }
            Some(ProjectedKeyspace {
                members,
                has_index_signature: has_index,
            })
        }
        TypeExpr::Intersection(parts) => {
            let mut all_members = Vec::new();
            let mut has_index = false;
            for part in parts.iter() {
                if let Some(ks) = extract_keyspace_from_type_expr(part) {
                    all_members.extend(ks.members);
                    has_index |= ks.has_index_signature;
                }
            }
            Some(ProjectedKeyspace {
                members: all_members,
                has_index_signature: has_index,
            })
        }
        _ => None,
    }
}

/// Extract the full surface from a solved TypeExpr.
fn extract_surface_from_type_expr(expr: &TypeExpr) -> Option<ProjectedSurface> {
    use crate::analysis::type_expr::ObjectMember;
    match expr {
        TypeExpr::Object(obj) => {
            let mut members = Vec::new();
            let mut call_sigs = Vec::new();
            let mut construct_sigs = Vec::new();
            let mut has_index = false;
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        members.push(ProjectedMember {
                            name: prop.name.clone(),
                            ty: prop.ty.clone(),
                            optional: prop.optional,
                            readonly: prop.readonly,
                            is_method: false,
                        });
                    }
                    ObjectMember::Method(method) => {
                        members.push(ProjectedMember {
                            name: method.name.clone(),
                            ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                            optional: method.optional,
                            readonly: false,
                            is_method: true,
                        });
                    }
                    ObjectMember::CallSignature(sig) => {
                        call_sigs.push(TypeExpr::Function(std::sync::Arc::new(sig.clone())));
                    }
                    ObjectMember::IndexSignature(_) => has_index = true,
                    ObjectMember::ConstructSignature(sig) => {
                        construct_sigs.push(TypeExpr::Function(std::sync::Arc::new(sig.clone())));
                    }
                }
            }
            Some(ProjectedSurface {
                members,
                call_signatures: call_sigs,
                construct_signatures: construct_sigs,
                has_index_signature: has_index,
            })
        }
        TypeExpr::Intersection(parts) => {
            let mut all_members = Vec::new();
            let mut all_sigs = Vec::new();
            let mut all_construct_sigs = Vec::new();
            let mut has_index = false;
            for part in parts.iter() {
                if let Some(surface) = extract_surface_from_type_expr(part) {
                    all_members.extend(surface.members);
                    all_sigs.extend(surface.call_signatures);
                    all_construct_sigs.extend(surface.construct_signatures);
                    has_index |= surface.has_index_signature;
                }
            }
            Some(ProjectedSurface {
                members: all_members,
                call_signatures: all_sigs,
                construct_signatures: all_construct_sigs,
                has_index_signature: has_index,
            })
        }
        _ => None,
    }
}

fn projected_keyspace_from_result(keyspace: &super::result::Keyspace) -> ProjectedKeyspace {
    match keyspace {
        super::result::Keyspace::Finite(members) => ProjectedKeyspace {
            members: members.clone(),
            has_index_signature: false,
        },
        super::result::Keyspace::Open => ProjectedKeyspace {
            members: Vec::new(),
            has_index_signature: true,
        },
        super::result::Keyspace::Empty => ProjectedKeyspace {
            members: Vec::new(),
            has_index_signature: false,
        },
    }
}

fn projected_member_to_type_expr(member: &ProjectedMember) -> TypeExpr {
    use crate::analysis::type_expr::{MethodSignature, ObjectExpr, ObjectMember, ObjectProperty};

    let property = if member.is_method {
        match &member.ty {
            TypeExpr::Function(function) => ObjectMember::Method(MethodSignature {
                name: member.name.clone(),
                function: (**function).clone(),
                optional: member.optional,
            }),
            _ => ObjectMember::Property(ObjectProperty {
                name: member.name.clone(),
                ty: member.ty.clone(),
                optional: member.optional,
                readonly: member.readonly,
            }),
        }
    } else {
        ObjectMember::Property(ObjectProperty {
            name: member.name.clone(),
            ty: member.ty.clone(),
            optional: member.optional,
            readonly: member.readonly,
        })
    };

    TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
        properties: vec![property],
    }))
}

fn projected_keyspace_to_type_expr(keyspace: &ProjectedKeyspace) -> Option<TypeExpr> {
    use crate::analysis::type_expr::{
        IndexSignature, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
    };

    if keyspace.members.is_empty() && !keyspace.has_index_signature {
        return None;
    }

    let mut properties = keyspace
        .members
        .iter()
        .map(|member| {
            ObjectMember::Property(ObjectProperty {
                name: member.clone(),
                ty: TypeExpr::Unknown {
                    raw: "projectedKeyspaceMember".to_string(),
                },
                optional: false,
                readonly: false,
            })
        })
        .collect::<Vec<_>>();

    if keyspace.has_index_signature {
        properties.push(ObjectMember::IndexSignature(IndexSignature {
            key_name: "key".to_string(),
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenKeyspace".to_string(),
            },
            readonly: false,
        }));
    }

    Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
        properties,
    })))
}

fn projected_surface_from_shape(
    arena: &QueryArena,
    shape: &project::SurfaceShape,
) -> Option<ProjectedSurface> {
    if shape.properties.is_empty()
        && shape.call_signatures.is_empty()
        && shape.construct_signatures.is_empty()
        && shape.index_signatures.is_empty()
    {
        return None;
    }

    let members = shape
        .properties
        .iter()
        .map(|prop| ProjectedMember {
            name: prop.name.clone(),
            ty: project_to_type_expr(arena, prop.ty),
            optional: prop.optional,
            readonly: prop.readonly,
            is_method: prop.is_method,
        })
        .collect();
    let call_signatures = shape
        .call_signatures
        .iter()
        .map(|sig| surface_signature_to_type_expr(arena, sig))
        .collect();
    let construct_signatures = shape
        .construct_signatures
        .iter()
        .map(|sig| surface_signature_to_type_expr(arena, sig))
        .collect();

    Some(ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature: !shape.index_signatures.is_empty() || shape.is_open,
    })
}

fn surface_signature_to_type_expr(
    arena: &QueryArena,
    sig: &project::SurfaceCallSignature,
) -> TypeExpr {
    TypeExpr::Function(std::sync::Arc::new(
        crate::analysis::type_expr::FunctionExpr {
            parameters: sig
                .parameters
                .iter()
                .map(|param| crate::analysis::type_expr::FunctionParam {
                    name: param.name.clone(),
                    ty: project_to_type_expr(arena, param.ty),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            return_type: Some(std::sync::Arc::new(project_to_type_expr(
                arena,
                sig.return_type,
            ))),
            type_parameters: Vec::new(),
        },
    ))
}

fn projected_surface_to_type_expr(surface: &ProjectedSurface) -> Option<TypeExpr> {
    use crate::analysis::type_expr::{
        FunctionExpr, IndexSignature, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
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
            key_type: TypeExpr::Primitive(crate::analysis::type_expr::PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        }));
    }

    Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
        properties,
    })))
}

fn type_expr_references_type_params(
    expr: &TypeExpr,
    type_params: &[crate::analysis::type_expr::TypeParam],
) -> bool {
    use rustc_hash::FxHashSet;

    fn visit(expr: &TypeExpr, type_param_names: &FxHashSet<&str>) -> bool {
        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. } => false,
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                type_param_names.contains(name.as_ref())
                    || type_arguments
                        .iter()
                        .any(|argument| visit(argument, type_param_names))
            }
            TypeExpr::TypeParameter(parameter) => {
                type_param_names.contains(parameter.name.as_str())
                    || parameter
                        .constraint
                        .as_deref()
                        .is_some_and(|constraint| visit(constraint, type_param_names))
                    || parameter
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, type_param_names))
            }
            TypeExpr::Union(types)
            | TypeExpr::Intersection(types)
            | TypeExpr::TemplateLiteral {
                expressions: types, ..
            } => types.iter().any(|ty| visit(ty, type_param_names)),
            TypeExpr::Array { element, .. }
            | TypeExpr::Parenthesized(element)
            | TypeExpr::KeyOf(element)
            | TypeExpr::Rest(element) => visit(element, type_param_names),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, type_param_names)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, type_param_names)
                }
                crate::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, type_param_names)
                        || visit(&signature.value_type, type_param_names)
                }
                crate::analysis::type_expr::ObjectMember::CallSignature(function)
                | crate::analysis::type_expr::ObjectMember::ConstructSignature(function) => {
                    function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, type_param_names))
                        || function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, type_param_names))
                }
                crate::analysis::type_expr::ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, type_param_names))
                        || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, type_param_names))
                }
            }),
            TypeExpr::Function(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| visit(&parameter.ty, type_param_names))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| visit(return_type, type_param_names))
                    || function.type_parameters.iter().any(|parameter| {
                        parameter
                            .constraint
                            .as_deref()
                            .is_some_and(|constraint| visit(constraint, type_param_names))
                            || parameter
                                .default
                                .as_deref()
                                .is_some_and(|default| visit(default, type_param_names))
                    })
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, type_param_names) || visit(index, type_param_names)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, type_param_names)
                    || visit(extends, type_param_names)
                    || visit(true_type, type_param_names)
                    || visit(false_type, type_param_names)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, type_param_names)
                    || visit(value, type_param_names)
                    || name_type
                        .as_deref()
                        .is_some_and(|name_type| visit(name_type, type_param_names))
            }
            TypeExpr::RecursiveRef { type_arguments, .. } => type_arguments
                .iter()
                .any(|argument| visit(argument, type_param_names)),
        }
    }

    let type_param_names: FxHashSet<&str> = type_params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    !type_param_names.is_empty() && visit(expr, &type_param_names)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::type_expr::PrimitiveName;
    use crate::analysis::type_solver::host::NoopSolverHost;
    use crate::analysis::type_solver::result::ExecutionStatus;
    use crate::analysis::type_solver::result::SolverExactness;
    use std::cell::Cell;

    #[test]
    fn engine_solves_primitive() {
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::new(&host);

        let expr = TypeExpr::Primitive(PrimitiveName::String);
        let result = engine.solve(&expr);
        assert!(matches!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::String)
        ));
        // Negative: must not be degraded
        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert!(result.incomplete_reasons.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn engine_subject_interning() {
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::<NoopAudit>::with_audit(&host, NoopAudit);

        let key1 = SubjectKey::Decl {
            canonical_id: "a.ts".into(),
            symbol_name: "Foo".into(),
            args_hash: 42,
            conditional_ctx_hash: 0,
        };
        let key2 = SubjectKey::Decl {
            canonical_id: "a.ts".into(),
            symbol_name: "Foo".into(),
            args_hash: 42,
            conditional_ctx_hash: 0,
        };
        let key3 = SubjectKey::Decl {
            canonical_id: "a.ts".into(),
            symbol_name: "Bar".into(),
            args_hash: 42,
            conditional_ctx_hash: 0,
        };

        let id1 = engine.intern_subject(key1);
        let id2 = engine.intern_subject(key2);
        let id3 = engine.intern_subject(key3);

        // Same key -> same id
        assert_eq!(id1, id2);
        // Different key -> different id
        assert_ne!(id1, id3);
        // Negative: ids are monotonic, no collisions
        assert_eq!(id1.0, 0);
        assert_eq!(id3.0, 1);
    }

    #[test]
    fn engine_with_recording_audit() {
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::new_with_recording(&host);

        let expr = TypeExpr::Primitive(PrimitiveName::String);
        let result = engine.solve(&expr);
        assert!(matches!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::String)
        ));
        assert_eq!(result.execution_status, ExecutionStatus::Completed);

        // Audit accessible and counters start at zero for a primitive solve
        let audit = engine.audit();
        assert_eq!(audit.conditional_deferrals, 0);
        assert_eq!(audit.indexed_access_open_skips, 0);
    }

    #[test]
    fn engine_solve_with_trace() {
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::new(&host);

        let expr = TypeExpr::Primitive(PrimitiveName::Number);
        let (result, trace) = engine.solve_with_trace(&expr);
        assert!(matches!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::Number)
        ));
        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert!(result.incomplete_reasons.is_empty());
        // Primitives don't visit external decls
        assert!(trace.is_empty());
        assert!(engine.visited_decls().is_empty());
    }

    #[test]
    fn engine_tracks_steps_only_for_uncached_solves() {
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::new(&host);

        let expr = TypeExpr::Primitive(PrimitiveName::String);
        let first = engine.solve(&expr);
        assert_eq!(engine.solve_count(), 1);
        assert_eq!(engine.total_steps(), first.steps);

        let second = engine.solve(&expr);
        assert_eq!(
            engine.solve_count(),
            1,
            "cached top-level solves should not increment solve_count",
        );
        assert_eq!(
            engine.total_steps(),
            first.steps,
            "cached top-level solves should not increment total_steps",
        );
        assert_eq!(second.steps, first.steps);
    }

    // -- Scoped solve tests --

    use crate::analysis::type_eval::TypeDeclKind;
    use crate::analysis::type_solver::host::{ResolvedRootIdentity, TypeSolverHost, UtilitySource};
    use crate::analysis::type_solver::prepared::{PreparedTypeDecl, PreparedValueDecl};
    use std::sync::Arc;

    /// Test host that resolves `root_identity("", name)` only for names in
    /// its `known_bare_names` set. Used to test scope-aware resolution.
    struct ScopedTestHost {
        known_bare_names: std::collections::HashSet<String>,
        decls: rustc_hash::FxHashMap<String, Arc<PreparedTypeDecl>>,
    }

    impl ScopedTestHost {
        fn with_names(names: &[&str]) -> Self {
            let mut known = std::collections::HashSet::new();
            let mut decls = rustc_hash::FxHashMap::default();
            for name in names {
                known.insert(name.to_string());
                let prepared = PreparedTypeDecl::new(
                    ResolvedRootIdentity::new("test_scope", *name),
                    TypeDeclKind::Alias,
                    TypeExpr::Primitive(PrimitiveName::String),
                );
                decls.insert(name.to_string(), Arc::new(prepared));
            }
            Self {
                known_bare_names: known,
                decls,
            }
        }

        fn with_decls(decls_input: &[(&str, TypeExpr)]) -> Self {
            let mut known = std::collections::HashSet::new();
            let mut decls = rustc_hash::FxHashMap::default();
            for (name, body) in decls_input {
                known.insert((*name).to_string());
                let prepared = PreparedTypeDecl::new(
                    ResolvedRootIdentity::new("test_scope", *name),
                    TypeDeclKind::Alias,
                    body.clone(),
                );
                decls.insert((*name).to_string(), Arc::new(prepared));
            }
            Self {
                known_bare_names: known,
                decls,
            }
        }
    }

    impl TypeSolverHost for ScopedTestHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.decls.get(&root_identity.symbol_name).cloned()
        }
        fn resolve_prepared_value_decl(
            &self,
            _: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            None
        }
        fn utility_source(&self, _: &str) -> UtilitySource {
            UtilitySource::Unknown
        }
        fn root_identity(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            if canonical_id.is_empty() && self.known_bare_names.contains(symbol_name) {
                Some(ResolvedRootIdentity::new("test_scope", symbol_name))
            } else {
                None
            }
        }
    }

    struct CountingPreserveHost {
        root_identity_calls: Cell<usize>,
        resolve_prepared_type_decl_calls: Cell<usize>,
        canonical_id: &'static str,
        prepared: Arc<PreparedTypeDecl>,
    }

    impl CountingPreserveHost {
        fn new() -> Self {
            Self::with_canonical("/pkg/index.d.ts")
        }

        fn new_package() -> Self {
            Self::with_canonical("/node_modules/editor-lib/index.d.ts")
        }

        fn with_canonical(canonical_id: &'static str) -> Self {
            let mut prepared = PreparedTypeDecl::new(
                ResolvedRootIdentity::new(canonical_id, "DialogContentProps"),
                TypeDeclKind::Interface,
                TypeExpr::Object(std::sync::Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "id".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: true,
                                readonly: false,
                            },
                        )],
                    },
                )),
            );
            prepared.build_member_index();
            prepared.classify_wrapper_shape();
            prepared.classify_projection();
            Self {
                root_identity_calls: Cell::new(0),
                resolve_prepared_type_decl_calls: Cell::new(0),
                canonical_id,
                prepared: Arc::new(prepared),
            }
        }
    }

    impl TypeSolverHost for CountingPreserveHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.resolve_prepared_type_decl_calls
                .set(self.resolve_prepared_type_decl_calls.get() + 1);
            (root_identity.canonical_id == self.canonical_id
                && root_identity.symbol_name == "DialogContentProps")
                .then(|| Arc::clone(&self.prepared))
        }

        fn resolve_prepared_value_decl(
            &self,
            _root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, name: &str) -> UtilitySource {
            match name {
                "Omit" | "Partial" | "ReturnType" => UtilitySource::Builtin,
                _ => UtilitySource::Unknown,
            }
        }

        fn bare_ref_origin(&self, name: &str) -> BareRefOrigin {
            match name {
                "DialogContentProps" | "useTemplateRef" => BareRefOrigin::Imported,
                _ => BareRefOrigin::Unknown,
            }
        }

        fn root_identity(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            assert!(
                canonical_id.is_empty(),
                "shallow preservation checks should resolve imported bare refs from the owner scope"
            );
            self.root_identity_calls
                .set(self.root_identity_calls.get() + 1);
            (symbol_name == "DialogContentProps")
                .then(|| ResolvedRootIdentity::new(self.canonical_id, symbol_name))
        }
    }

    struct TransitivePreserveHost {
        decls: rustc_hash::FxHashMap<String, Arc<PreparedTypeDecl>>,
    }

    impl TransitivePreserveHost {
        fn new() -> Self {
            let mut decls = rustc_hash::FxHashMap::default();

            let mut command_palette_group = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/src/CommandPalette.vue", "CommandPaletteGroup"),
                TypeDeclKind::Interface,
                TypeExpr::Object(std::sync::Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "items".to_string(),
                                ty: TypeExpr::Array {
                                    element: std::sync::Arc::new(TypeExpr::named("T")),
                                    readonly: false,
                                },
                                optional: true,
                                readonly: false,
                            },
                        )],
                    },
                )),
            );
            command_palette_group.type_parameters = vec![crate::analysis::type_expr::TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }];
            command_palette_group.build_member_index();
            command_palette_group.classify_wrapper_shape();
            command_palette_group.classify_projection();
            decls.insert(
                "CommandPaletteGroup".to_string(),
                Arc::new(command_palette_group),
            );

            let mut content_search_item = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/src/ContentSearch.vue", "ContentSearchItem"),
                TypeDeclKind::Interface,
                TypeExpr::Intersection(std::sync::Arc::from(vec![
                    TypeExpr::named_with_args(
                        "Omit",
                        vec![
                            TypeExpr::named("LinkProps"),
                            TypeExpr::string_literal("custom"),
                        ],
                    ),
                    TypeExpr::Object(std::sync::Arc::new(
                        crate::analysis::type_expr::ObjectExpr {
                            properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "badge".to_string(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: true,
                                    readonly: false,
                                },
                            )],
                        },
                    )),
                ])),
            );
            content_search_item.build_member_index();
            content_search_item.classify_wrapper_shape();
            content_search_item.classify_projection();
            decls.insert(
                "ContentSearchItem".to_string(),
                Arc::new(content_search_item),
            );

            let mut link_props = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/src/Link.vue", "LinkProps"),
                TypeDeclKind::Interface,
                TypeExpr::named_with_args(
                    "Omit",
                    vec![
                        TypeExpr::named("RouterLinkProps"),
                        TypeExpr::string_literal("to"),
                    ],
                ),
            );
            link_props.build_member_index();
            link_props.classify_wrapper_shape();
            link_props.classify_projection();
            decls.insert("LinkProps".to_string(), Arc::new(link_props));

            let mut router_link_props = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/node_modules/vue-router/index.d.ts", "RouterLinkProps"),
                TypeDeclKind::Interface,
                TypeExpr::Object(std::sync::Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "to".to_string(),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                            crate::analysis::type_expr::ObjectMember::Property(
                                crate::analysis::type_expr::ObjectProperty {
                                    name: "replace".to_string(),
                                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                        ],
                    },
                )),
            );
            router_link_props.build_member_index();
            router_link_props.classify_wrapper_shape();
            router_link_props.classify_projection();
            decls.insert("RouterLinkProps".to_string(), Arc::new(router_link_props));

            Self { decls }
        }
    }

    struct LocalTypeParameterPreserveHost {
        imported_root_identity_calls: Cell<u32>,
        imported_resolve_prepared_type_decl_calls: Cell<u32>,
        local_prepared: Arc<PreparedTypeDecl>,
        imported_prepared: Arc<PreparedTypeDecl>,
    }

    impl LocalTypeParameterPreserveHost {
        fn new() -> Self {
            let mut local_prepared = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/src/App.vue", "T"),
                TypeDeclKind::Alias,
                TypeExpr::type_parameter(crate::analysis::type_expr::TypeParam {
                    name: "T".to_string(),
                    constraint: Some(std::sync::Arc::new(TypeExpr::named("DialogContentProps"))),
                    default: None,
                }),
            );
            local_prepared.build_member_index();
            local_prepared.classify_wrapper_shape();
            local_prepared.classify_projection();

            let mut imported_prepared = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/src/types.ts", "DialogContentProps"),
                TypeDeclKind::Interface,
                TypeExpr::Object(std::sync::Arc::new(
                    crate::analysis::type_expr::ObjectExpr {
                        properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "id".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: true,
                                readonly: false,
                            },
                        )],
                    },
                )),
            );
            imported_prepared.build_member_index();
            imported_prepared.classify_wrapper_shape();
            imported_prepared.classify_projection();

            Self {
                imported_root_identity_calls: Cell::new(0),
                imported_resolve_prepared_type_decl_calls: Cell::new(0),
                local_prepared: Arc::new(local_prepared),
                imported_prepared: Arc::new(imported_prepared),
            }
        }
    }

    impl TypeSolverHost for LocalTypeParameterPreserveHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            match (
                root_identity.canonical_id.as_str(),
                root_identity.symbol_name.as_str(),
            ) {
                ("/src/App.vue", "T") => Some(Arc::clone(&self.local_prepared)),
                ("/src/types.ts", "DialogContentProps") => {
                    self.imported_resolve_prepared_type_decl_calls
                        .set(self.imported_resolve_prepared_type_decl_calls.get() + 1);
                    Some(Arc::clone(&self.imported_prepared))
                }
                _ => None,
            }
        }

        fn resolve_prepared_value_decl(
            &self,
            _root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, _name: &str) -> UtilitySource {
            UtilitySource::Unknown
        }

        fn bare_ref_origin(&self, name: &str) -> BareRefOrigin {
            match name {
                "T" => BareRefOrigin::Local,
                "DialogContentProps" => BareRefOrigin::Imported,
                _ => BareRefOrigin::Unknown,
            }
        }

        fn root_identity(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            match (canonical_id, symbol_name) {
                ("", "T") => Some(ResolvedRootIdentity::new("/src/App.vue", "T")),
                ("", "DialogContentProps") => {
                    self.imported_root_identity_calls
                        .set(self.imported_root_identity_calls.get() + 1);
                    Some(ResolvedRootIdentity::new(
                        "/src/types.ts",
                        "DialogContentProps",
                    ))
                }
                _ => None,
            }
        }
    }

    struct LocalAliasImportedGenericFastHost {
        local_root_identity_calls: Cell<u32>,
        imported_root_identity_calls: Cell<u32>,
        local_prepared_lookup_calls: Cell<u32>,
        imported_prepared_lookup_calls: Cell<u32>,
        local_prepared: Arc<PreparedTypeDecl>,
    }

    impl LocalAliasImportedGenericFastHost {
        fn new() -> Self {
            let mut local_prepared = PreparedTypeDecl::new(
                ResolvedRootIdentity::new("/src/App.vue", "DashboardSearch"),
                TypeDeclKind::Alias,
                TypeExpr::named_with_args(
                    "ComponentConfig",
                    vec![
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::string_literal("dashboardSearch"),
                    ],
                ),
            );
            local_prepared.build_member_index();
            local_prepared.classify_wrapper_shape();
            local_prepared.classify_projection();

            Self {
                local_root_identity_calls: Cell::new(0),
                imported_root_identity_calls: Cell::new(0),
                local_prepared_lookup_calls: Cell::new(0),
                imported_prepared_lookup_calls: Cell::new(0),
                local_prepared: Arc::new(local_prepared),
            }
        }
    }

    impl TypeSolverHost for LocalAliasImportedGenericFastHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            match (
                root_identity.canonical_id.as_str(),
                root_identity.symbol_name.as_str(),
            ) {
                ("/src/App.vue", "DashboardSearch") => {
                    self.local_prepared_lookup_calls
                        .set(self.local_prepared_lookup_calls.get() + 1);
                    Some(Arc::clone(&self.local_prepared))
                }
                ("/src/types/tv.ts", "ComponentConfig") => {
                    self.imported_prepared_lookup_calls
                        .set(self.imported_prepared_lookup_calls.get() + 1);
                    None
                }
                _ => None,
            }
        }

        fn resolve_prepared_value_decl(
            &self,
            _root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, _name: &str) -> UtilitySource {
            UtilitySource::Unknown
        }

        fn bare_ref_origin(&self, name: &str) -> BareRefOrigin {
            match name {
                "DashboardSearch" => BareRefOrigin::Local,
                "ComponentConfig" => BareRefOrigin::Imported,
                _ => BareRefOrigin::Unknown,
            }
        }

        fn root_identity(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            match (canonical_id, symbol_name) {
                ("", "DashboardSearch") => {
                    self.local_root_identity_calls
                        .set(self.local_root_identity_calls.get() + 1);
                    Some(ResolvedRootIdentity::new("/src/App.vue", "DashboardSearch"))
                }
                ("", "ComponentConfig") => {
                    self.imported_root_identity_calls
                        .set(self.imported_root_identity_calls.get() + 1);
                    Some(ResolvedRootIdentity::new(
                        "/src/types/tv.ts",
                        "ComponentConfig",
                    ))
                }
                _ => None,
            }
        }
    }

    impl TypeSolverHost for TransitivePreserveHost {
        fn resolve_prepared_type_decl(
            &self,
            root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedTypeDecl>> {
            self.decls.get(&root_identity.symbol_name).cloned()
        }

        fn resolve_prepared_value_decl(
            &self,
            _root_identity: &ResolvedRootIdentity,
        ) -> Option<Arc<PreparedValueDecl>> {
            None
        }

        fn utility_source(&self, name: &str) -> UtilitySource {
            match name {
                "Omit" => UtilitySource::Builtin,
                _ => UtilitySource::Unknown,
            }
        }

        fn bare_ref_origin(&self, name: &str) -> BareRefOrigin {
            match name {
                "CommandPaletteGroup" | "LinkProps" | "RouterLinkProps" => BareRefOrigin::Imported,
                "ContentSearchItem" => BareRefOrigin::Local,
                _ => BareRefOrigin::Unknown,
            }
        }

        fn root_identity(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<ResolvedRootIdentity> {
            if canonical_id.is_empty() {
                self.decls
                    .get(symbol_name)
                    .map(|decl| decl.root_identity.clone())
            } else {
                self.decls
                    .get(symbol_name)
                    .filter(|decl| decl.root_identity.canonical_id == canonical_id)
                    .map(|decl| decl.root_identity.clone())
            }
        }
    }

    #[test]
    fn solve_scoped_shares_engine_state() {
        // Two scoped solves in the same engine share caches/arena.
        let host = NoopSolverHost;
        let scoped_host = ScopedTestHost::with_names(&["MyType"]);
        let mut engine = TypeQueryEngine::new(&host);

        let expr = TypeExpr::named("MyType");
        let (r1, _) = engine.solve_scoped(&scoped_host, "scope_a", &expr);
        let steps1 = engine.total_steps();
        assert_eq!(engine.solve_count(), 1, "first scoped solve should count");

        // Same scope + same expr should hit op_cache
        let (r2, _) = engine.solve_scoped(&scoped_host, "scope_a", &expr);
        assert_eq!(
            engine.solve_count(),
            1,
            "second scoped solve with same scope should hit cache"
        );
        assert_eq!(
            engine.total_steps(),
            steps1,
            "cached scoped solve should not add steps"
        );
        assert_eq!(r1.value, r2.value, "cached result must match");
    }

    #[test]
    fn shallow_field_preservation_caches_imported_ref_probes() {
        let host = CountingPreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::Intersection(std::sync::Arc::from(vec![
            TypeExpr::named_with_args(
                "Omit",
                vec![
                    TypeExpr::named("DialogContentProps"),
                    TypeExpr::string_literal("as"),
                ],
            ),
            TypeExpr::named_with_args(
                "Partial",
                vec![TypeExpr::named_with_args(
                    "Omit",
                    vec![
                        TypeExpr::named("DialogContentProps"),
                        TypeExpr::string_literal("forceMount"),
                    ],
                )],
            ),
        ]));

        assert!(
            engine.should_preserve_shallow_field_expr(&expr),
            "utility-wrapped imported object refs should stay shallow-symbolic"
        );
        assert!(
            engine.should_preserve_shallow_field_expr(&expr),
            "repeating the same field expression should hit the request-local preserve cache"
        );
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "repeated preserve checks should reuse one imported-root proof"
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            1,
            "repeated preserve checks should reuse one prepared-decl lookup"
        );
    }

    #[test]
    fn shallow_field_preservation_skips_prepared_lookup_for_package_imports() {
        let host = CountingPreserveHost::new_package();
        let mut engine = TypeQueryEngine::new(&host);

        assert!(
            engine.should_preserve_imported_bare_ref(&TypeExpr::named("DialogContentProps")),
            "package-backed imported refs should stay symbolic in shallow field evaluation"
        );
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "package-backed preserve checks should still prove the direct import binding once"
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "package-backed preserve checks should not materialize the imported prepared decl just to keep it symbolic"
        );
    }

    #[test]
    fn shallow_field_preservation_keeps_package_member_paths_symbolic() {
        let host = CountingPreserveHost::new_package();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(TypeExpr::named_with_args(
                "DialogContentProps",
                vec![TypeExpr::Primitive(PrimitiveName::String)],
            )),
            index: std::sync::Arc::new(TypeExpr::string_literal("state")),
        };

        assert!(
            engine.should_preserve_shallow_field_expr(&expr),
            "package-backed indexed member paths should stay symbolic in shallow field evaluation"
        );
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "package-backed member path preservation should prove the import root once"
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "package-backed member path preservation should not materialize the imported prepared decl"
        );
    }

    #[test]
    fn shallow_field_preservation_keeps_direct_imported_member_paths_symbolic_without_prepared_lookup(
    ) {
        let host = CountingPreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::Intersection(std::sync::Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(TypeExpr::named("DialogContentProps")),
                index: std::sync::Arc::new(TypeExpr::string_literal("id")),
            },
        ]));

        assert!(
            engine.should_preserve_shallow_field_expr(&expr),
            "direct imported member paths inside symbolic wrappers should stay shallow"
        );
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "direct imported member path preservation should still prove the import root once"
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "direct imported member path preservation should not materialize the imported prepared decl just to keep it shallow",
        );
    }

    #[test]
    fn fast_shallow_field_expansion_materializes_imported_single_member_path() {
        let host = CountingPreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(TypeExpr::named("DialogContentProps")),
            index: std::sync::Arc::new(TypeExpr::string_literal("id")),
        };

        let result = engine
            .try_fast_shallow_field_expr(&expr)
            .expect("direct imported member paths should use the prepared-member fast path");

        assert_eq!(
            result.value,
            TypeExpr::Primitive(PrimitiveName::String),
            "fast shallow member expansion should reuse the direct prepared member body",
        );
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "fast imported member expansion should prove the import root once",
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            1,
            "fast imported member expansion should read one prepared declaration",
        );
    }

    #[test]
    fn fast_shallow_field_expansion_keeps_imported_utility_routes_symbolic_without_root_probes() {
        let host = CountingPreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::Union(std::sync::Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::Boolean),
            TypeExpr::named_with_args(
                "Omit",
                vec![
                    TypeExpr::named("DialogContentProps"),
                    TypeExpr::string_literal("id"),
                ],
            ),
        ]));

        let result = engine
            .try_fast_shallow_field_expr(&expr)
            .expect("utility-wrapped imported refs should use the symbolic shallow fast path");

        assert_eq!(result.value, expr);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(
            host.root_identity_calls.get(),
            0,
            "fast symbolic utility wrapping should not prove imported roots just to stay shallow",
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "fast symbolic utility wrapping should not materialize imported prepared declarations",
        );
    }

    #[test]
    fn fast_shallow_field_expansion_keeps_return_type_of_imported_value_symbolic() {
        let host = CountingPreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::named_with_args(
            "ReturnType",
            vec![TypeExpr::TypeOf(crate::analysis::type_expr::ValueRef {
                path: vec!["useTemplateRef".to_string()],
            })],
        );

        let result = engine.try_fast_shallow_field_expr(&expr).expect(
            "ReturnType<typeof importedValue> should stay symbolic on the shallow fast path",
        );

        assert_eq!(result.value, expr);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(
            host.root_identity_calls.get(),
            0,
            "utility routes over imported values should not probe imported roots just to stay shallow",
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "utility routes over imported values should not materialize imported declarations",
        );
    }

    #[test]
    fn fast_shallow_field_expansion_keeps_package_single_member_paths_symbolic_without_prepared_lookup(
    ) {
        let host = CountingPreserveHost::new_package();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(TypeExpr::named("DialogContentProps")),
            index: std::sync::Arc::new(TypeExpr::string_literal("id")),
        };

        let result = engine
            .try_fast_shallow_field_expr(&expr)
            .expect("package-backed single-member paths should use the symbolic shallow fast path");

        assert_eq!(
            result.value, expr,
            "package-backed single-member paths should stay symbolic in shallow field expansion"
        );
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "package-backed single-member fast path should still prove the imported root once",
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "package-backed single-member fast path should not materialize the imported prepared declaration",
        );
    }

    #[test]
    fn fast_shallow_field_expansion_keeps_package_generic_refs_symbolic_without_prepared_lookup() {
        let host = CountingPreserveHost::new_package();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::named_with_args(
            "DialogContentProps",
            vec![TypeExpr::Primitive(PrimitiveName::String)],
        );

        let result = engine.try_fast_shallow_field_expr(&expr).expect(
            "package-backed imported generic refs should use the symbolic shallow fast path",
        );

        assert_eq!(result.value, expr);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(
            host.root_identity_calls.get(),
            1,
            "package-backed imported generic refs should prove the imported root once",
        );
        assert_eq!(
            host.resolve_prepared_type_decl_calls.get(),
            0,
            "package-backed imported generic refs should not materialize the prepared declaration just to stay symbolic",
        );
    }

    #[test]
    fn fast_shallow_field_expansion_keeps_local_alias_member_paths_symbolic_when_body_routes_into_imported_generic(
    ) {
        let host = LocalAliasImportedGenericFastHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(TypeExpr::named("DashboardSearch")),
                index: std::sync::Arc::new(TypeExpr::string_literal("variants")),
            }),
            index: std::sync::Arc::new(TypeExpr::string_literal("size")),
        };

        let result = engine
            .try_fast_shallow_field_expr(&expr)
            .expect("local alias member paths that only route into imported generic helpers should stay symbolic immediately");

        assert_eq!(result.value, expr);
        assert_eq!(result.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(
            host.local_root_identity_calls.get(),
            1,
            "the fast path should prove the local alias once",
        );
        assert_eq!(
            host.local_prepared_lookup_calls.get(),
            1,
            "the fast path should inspect the local alias body once",
        );
        assert_eq!(
            host.imported_root_identity_calls.get(),
            0,
            "the fast path should not chase the imported generic helper just to keep the route symbolic",
        );
        assert_eq!(
            host.imported_prepared_lookup_calls.get(),
            0,
            "the fast path should not materialize the imported helper declaration",
        );
    }

    #[test]
    fn shallow_field_preservation_keeps_transitive_package_wrappers_symbolic() {
        let host = TransitivePreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::Array {
            element: std::sync::Arc::new(TypeExpr::named_with_args(
                "CommandPaletteGroup",
                vec![TypeExpr::named("ContentSearchItem")],
            )),
            readonly: false,
        };

        assert!(
            engine.should_preserve_shallow_field_expr(&expr),
            "local generic wrappers should stay shallow-symbolic when they flow into package-backed imported refs"
        );
    }

    #[test]
    fn shallow_field_preservation_keeps_local_type_params_symbolic_without_constraint_walk() {
        let host = LocalTypeParameterPreserveHost::new();
        let mut engine = TypeQueryEngine::new(&host);

        assert!(
            engine.should_preserve_shallow_field_expr(&TypeExpr::named("T")),
            "local generic parameters should stay symbolic in shallow field evaluation"
        );
        assert_eq!(
            host.imported_root_identity_calls.get(),
            0,
            "local generic parameters should not probe imported constraint roots just to stay symbolic"
        );
        assert_eq!(
            host.imported_resolve_prepared_type_decl_calls.get(),
            0,
            "local generic parameters should not materialize imported constraint declarations during shallow preservation"
        );
    }

    #[test]
    fn solve_scoped_different_scope_does_not_alias() {
        // Solving the same expr in two different scopes should not alias
        // through the op_cache (different scope_canonical_id in key).
        let host = NoopSolverHost;
        let scope_a = ScopedTestHost::with_names(&["Foo"]);
        let scope_b = ScopedTestHost::with_names(&[]); // Foo NOT known

        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::named("Foo");

        // Scope A resolves Foo
        let (ra, _) = engine.solve_scoped(&scope_a, "scope_a", &expr);
        assert!(
            matches!(ra.value, TypeExpr::Primitive(PrimitiveName::String)),
            "scope_a should resolve Foo to String"
        );

        // Scope B does NOT resolve Foo — must not reuse scope_a result
        let (rb, _) = engine.solve_scoped(&scope_b, "scope_b", &expr);
        assert!(
            !matches!(rb.value, TypeExpr::Primitive(PrimitiveName::String)),
            "scope_b must NOT inherit scope_a's resolution of Foo"
        );
    }

    #[test]
    fn solve_scoped_bare_name_miss_does_not_poison_other_scopes() {
        // A bare-name miss in scope A should not prevent scope B from
        // resolving the same name.
        let host = NoopSolverHost;
        let scope_missing = ScopedTestHost::with_names(&[]);
        let scope_has_it = ScopedTestHost::with_names(&["Promise"]);

        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::named("Promise");

        // First: miss in scope that doesn't have Promise
        let (r_miss, _) = engine.solve_scoped(&scope_missing, "scope_missing", &expr);
        assert!(
            !matches!(r_miss.value, TypeExpr::Primitive(PrimitiveName::String)),
            "scope without Promise should not resolve it"
        );

        // Second: scope that has Promise should still resolve it
        let (r_hit, _) = engine.solve_scoped(&scope_has_it, "scope_has_it", &expr);
        assert!(
            matches!(r_hit.value, TypeExpr::Primitive(PrimitiveName::String)),
            "scope with Promise must resolve it even after a miss in another scope"
        );
    }

    #[test]
    fn solve_scoped_explicit_canonical_deduplicates() {
        // Explicit canonical lookups (non-empty canonical_id) should deduplicate
        // safely across scoped queries in one shared engine.
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::new(&host);

        let expr = TypeExpr::Primitive(PrimitiveName::Number);

        let (r1, _) = engine.solve_scoped(&host, "scope_a", &expr);
        let (r2, _) = engine.solve_scoped(&host, "scope_b", &expr);

        // Same primitive expression should produce same result regardless of scope
        assert!(matches!(
            r1.value,
            TypeExpr::Primitive(PrimitiveName::Number)
        ),);
        assert!(matches!(
            r2.value,
            TypeExpr::Primitive(PrimitiveName::Number)
        ),);
    }

    #[test]
    fn engine_state_does_not_leak_across_requests() {
        // Two separate engines should not share state.
        let host = NoopSolverHost;
        let mut engine1 = TypeQueryEngine::new(&host);
        let engine2 = TypeQueryEngine::new(&host);

        let expr = TypeExpr::Primitive(PrimitiveName::String);
        engine1.solve(&expr);

        assert_eq!(engine1.solve_count(), 1);
        assert_eq!(
            engine2.solve_count(),
            0,
            "separate engine must have independent state"
        );
    }

    #[test]
    fn project_expr_surface_as_type_expr_preserves_callable_surface() {
        let host = NoopSolverHost;
        let mut engine = TypeQueryEngine::new(&host);
        let expr = TypeExpr::Function(std::sync::Arc::new(
            crate::analysis::type_expr::FunctionExpr {
                parameters: vec![crate::analysis::type_expr::FunctionParam {
                    name: Some("event".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                }],
                return_type: Some(std::sync::Arc::new(TypeExpr::Primitive(
                    PrimitiveName::Void,
                ))),
                type_parameters: Vec::new(),
            },
        ));

        let projected = engine
            .project_expr_surface_as_type_expr(&host, "", &expr)
            .expect("callable surface should project");

        assert!(
            matches!(projected, TypeExpr::Function(_)),
            "single callable surface should stay a Function, got: {projected:?}"
        );
    }

    #[test]
    fn project_member_reuses_request_scoped_projection_cache() {
        let host = NoopSolverHost;
        let scoped_host = ScopedTestHost::with_decls(&[(
            "Widget",
            TypeExpr::Object(std::sync::Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "title".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    )],
                },
            )),
        )]);
        let mut engine = TypeQueryEngine::new(&host);
        let subject = engine.intern_subject(SubjectKey::Decl {
            canonical_id: "scope_a".to_string(),
            symbol_name: "Widget".to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        });

        let first = engine
            .project_member(subject, "title", &scoped_host, "scope_a")
            .expect("first projected member should resolve");
        let steps_after_first = engine.total_steps();
        let solves_after_first = engine.solve_count();

        let second = engine
            .project_member(subject, "title", &scoped_host, "scope_a")
            .expect("second projected member should reuse cache");

        assert_eq!(first.name, second.name);
        assert_eq!(first.ty, second.ty);
        assert_eq!(
            engine.solve_count(),
            solves_after_first,
            "cached member projection should not trigger another scoped solve",
        );
        assert_eq!(
            engine.total_steps(),
            steps_after_first,
            "cached member projection should not add solver steps",
        );
    }

    #[test]
    fn project_surface_reuses_request_scoped_projection_cache() {
        let host = NoopSolverHost;
        let scoped_host = ScopedTestHost::with_decls(&[(
            "Widget",
            TypeExpr::Object(std::sync::Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: "title".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    )],
                },
            )),
        )]);
        let mut engine = TypeQueryEngine::new(&host);
        let subject = engine.intern_subject(SubjectKey::Decl {
            canonical_id: "scope_a".to_string(),
            symbol_name: "Widget".to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        });

        let first = engine
            .project_surface(subject, &scoped_host, "scope_a")
            .expect("first projected surface should resolve");
        let steps_after_first = engine.total_steps();
        let solves_after_first = engine.solve_count();

        let second = engine
            .project_surface(subject, &scoped_host, "scope_a")
            .expect("second projected surface should reuse cache");

        assert_eq!(first.members.len(), second.members.len());
        assert_eq!(first.members[0].name, second.members[0].name);
        assert_eq!(
            engine.solve_count(),
            solves_after_first,
            "cached surface projection should not trigger another scoped solve",
        );
        assert_eq!(
            engine.total_steps(),
            steps_after_first,
            "cached surface projection should not add solver steps",
        );
    }

    #[test]
    fn project_keyspace_reuses_request_scoped_projection_cache() {
        let host = NoopSolverHost;
        let scoped_host = ScopedTestHost::with_decls(&[(
            "Widget",
            TypeExpr::Object(std::sync::Arc::new(
                crate::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "title".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                        crate::analysis::type_expr::ObjectMember::Property(
                            crate::analysis::type_expr::ObjectProperty {
                                name: "count".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::Number),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            )),
        )]);
        let mut engine = TypeQueryEngine::new(&host);
        let subject = engine.intern_subject(SubjectKey::Decl {
            canonical_id: "scope_a".to_string(),
            symbol_name: "Widget".to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        });

        let first = engine
            .project_keyspace(subject, &scoped_host, "scope_a")
            .expect("first projected keyspace should resolve");
        let steps_after_first = engine.total_steps();
        let solves_after_first = engine.solve_count();

        let second = engine
            .project_keyspace(subject, &scoped_host, "scope_a")
            .expect("second projected keyspace should reuse cache");

        assert_eq!(first.members, second.members);
        assert_eq!(
            engine.solve_count(),
            solves_after_first,
            "cached keyspace projection should not trigger another scoped solve",
        );
        assert_eq!(
            engine.total_steps(),
            steps_after_first,
            "cached keyspace projection should not add solver steps",
        );
    }
}
