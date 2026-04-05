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
#[allow(dead_code)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum OpKey {
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
    IndexedAccess {
        object: SubjectId,
        index_hash: u64,
    },
    Conditional {
        check_hash: u64,
        extends_hash: u64,
        true_hash: u64,
        false_hash: u64,
        distributive: bool,
    },
    StructuralTransform {
        subject: SubjectId,
        transform_hash: u64,
    },
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
#[allow(dead_code)]
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
    #[allow(dead_code)]
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
    /// Shared request-scoped caches (relation, instantiation, keyspace, member).
    caches: SolverCaches,
    /// Trace accumulator: all external decls visited across all solves.
    visited_decls: Vec<ResolvedRootIdentity>,
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
            caches: SolverCaches::default(),
            visited_decls: Vec::new(),
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
            self.visited_decls.extend(cached.visited_decls.iter().cloned());
            return (cached.result.clone(), cached.visited_decls.clone());
        }
        self.audit.op_cache_miss("TopLevel");

        let mut state = SolveState::with_caches(
            SolveLimits::default(),
            std::mem::take(&mut self.instantiation_cache),
            std::mem::take(&mut self.caches),
        );
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
        self.caches = std::mem::take(&mut state.relation_caches);

        let result = SolverResult {
            value: result_expr,
            exactness: state.exactness,
            execution_status: state.execution_status,
            incomplete_reasons: state.incomplete_reasons,
            diagnostics: state.diagnostics,
        };

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
}
