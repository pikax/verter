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
use super::result::SolverResult;
use super::solve::{project_to_type_expr, resolve_node, SolveLimits, SolveState};
use super::substitution::SubstitutionEnv;
use crate::analysis::type_expr::TypeExpr;

// ---------------------------------------------------------------------------
// SubjectKey / SubjectId — canonical subject normalization (scaffolding)
// ---------------------------------------------------------------------------

/// Interned handle into `TypeQueryEngine.subjects`. Cheap to copy and hash.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    #[allow(dead_code)]
    ProjectMember { subject: SubjectId, member: String },
    #[allow(dead_code)]
    ProjectKeyspace { subject: SubjectId },
    #[allow(dead_code)]
    ProjectSurface { subject: SubjectId },
    #[allow(dead_code)]
    IndexedAccess { object: SubjectId, index_hash: u64 },
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
    #[allow(dead_code)]
    subjects: FxHashMap<SubjectKey, SubjectId>,
    #[allow(dead_code)]
    subject_keys: Vec<SubjectKey>,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    audit: A,
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
}

fn hash_expr(expr: &TypeExpr) -> u64 {
    let mut hasher = DefaultHasher::new();
    expr.hash(&mut hasher);
    hasher.finish()
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

    /// Scaffolding helper used only by `engine_subject_interning`.
    impl<'a, A: AuditSink> TypeQueryEngine<'a, A> {
        fn intern_subject(&mut self, key: SubjectKey) -> SubjectId {
            if let Some(&id) = self.subjects.get(&key) {
                return id;
            }
            let id = SubjectId(self.next_subject_id);
            self.next_subject_id += 1;
            self.subjects.insert(key.clone(), id);
            self.subject_keys.push(key);
            id
        }
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
}
