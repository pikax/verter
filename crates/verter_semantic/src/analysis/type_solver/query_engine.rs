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

use super::arena::{NodeId, QueryArena, SolverCaches};
use super::audit::{AuditSink, NoopAudit, RecordingAudit};
use super::host::{ResolvedRootIdentity, TypeSolverHost};
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

    /// Solve and return trace (for Phase 1 macro expansion).
    pub fn solve_with_trace(
        &mut self,
        expr: &TypeExpr,
    ) -> (SolverResult<TypeExpr>, Vec<ResolvedRootIdentity>) {
        let top_level_key = OpKey::TopLevel {
            expr_hash: hash_expr(expr),
            scope_canonical_id: String::new(),
        };
        if let Some(cached) = self.op_cache.get(&top_level_key) {
            self.audit.op_cache_hit("TopLevel");
            self.visited_decls
                .extend(cached.visited_decls.iter().cloned());
            return (cached.result.clone(), cached.visited_decls.clone());
        }
        self.audit.op_cache_miss("TopLevel");

        let mut state = SolveState::with_caches(
            SolveLimits::default(),
            std::mem::take(&mut self.instantiation_cache),
            std::mem::take(&mut self.caches),
        );
        state.projection_cache = std::mem::take(&mut self.projection_cache);
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
        self.instantiation_cache = std::mem::take(&mut state.instantiation_cache);
        self.projection_cache = std::mem::take(&mut state.projection_cache);
        self.caches = std::mem::take(&mut state.relation_caches);
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
        let top_level_key = OpKey::TopLevel {
            expr_hash: hash_expr(expr),
            scope_canonical_id: scope_canonical_id.to_string(),
        };
        if let Some(cached) = self.op_cache.get(&top_level_key) {
            self.audit.op_cache_hit("TopLevel_scoped");
            self.visited_decls
                .extend(cached.visited_decls.iter().cloned());
            return (cached.result.clone(), cached.visited_decls.clone());
        }
        self.audit.op_cache_miss("TopLevel_scoped");

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
        let result_expr = project_to_type_expr(&self.arena, resolved);

        let visited = std::mem::take(&mut state.visited_external_decls);
        self.visited_decls.extend(visited.iter().cloned());
        self.instantiation_cache = std::mem::take(&mut state.instantiation_cache);
        self.projection_cache = std::mem::take(&mut state.projection_cache);
        self.caches = std::mem::take(&mut state.relation_caches);
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
                let type_ref = TypeExpr::named(symbol_name);
                let resolved =
                    self.resolve_expr_node_scoped(scoped_host, scope_canonical_id, &type_ref);
                let surface = project::project_surface(&self.arena, resolved.node);
                if let Some(member) = surface
                    .value
                    .properties
                    .iter()
                    .find(|property| property.name == member_name)
                {
                    return Some(ProjectedMember {
                        name: member.name.clone(),
                        ty: project_to_type_expr(&self.arena, member.ty),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    });
                }

                let projected = project::project_member(
                    &mut self.arena,
                    &mut self.caches,
                    resolved.node,
                    member_name,
                );
                projected.value.map(|ty| ProjectedMember {
                    name: member_name.to_string(),
                    ty: project_to_type_expr(&self.arena, ty),
                    optional: false,
                    readonly: false,
                    is_method: false,
                })
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
                let type_ref = TypeExpr::named(symbol_name);
                let resolved =
                    self.resolve_expr_node_scoped(scoped_host, scope_canonical_id, &type_ref);
                let projected =
                    project::project_keyspace(&self.arena, &mut self.caches, resolved.node);
                Some(projected_keyspace_from_result(&projected.value))
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
                self.project_expr_surface(scoped_host, scope_canonical_id, &type_ref)
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
        let projected = project::project_surface(&self.arena, resolved.node);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::type_expr::PrimitiveName;
    use crate::analysis::type_solver::host::NoopSolverHost;
    use crate::analysis::type_solver::result::ExecutionStatus;

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
}
