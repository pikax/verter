//! Native type solver for demand-driven symbolic type resolution.
//!
//! This module replaces the legacy lightweight evaluator with a cache-owned,
//! demand-driven solver that can expand TypeScript types accurately using
//! generous operational safety rails instead of ad hoc budget cutoffs.
//!
//! # Architecture
//!
//! The solver operates over a pipeline:
//!
//! `shallow state → frontier → prepared declarations → query arena → solver → projection`
//!
//! ## Ownership boundary
//!
//! - `verter_session` owns file readiness, frontier traversal, and prepared
//!   declaration caching.
//! - `verter_semantic::analysis::type_solver` owns the solver kernel: arena,
//!   relations, projections, recursion handling, and built-in utility semantics.
//! - Consumers enter the solver with resolved root identities only — the solver
//!   never reopens route discovery from raw import specifiers or source text.
//!
//! ## Modules
//!
//! - [`host`]: `TypeSolverHost` trait — the load-bearing boundary between
//!   session (host) and solver.
//! - [`result`]: Exactness model, execution status, relation outcomes.
//! - [`arena`]: Query-local node interning with memoization tables.
//! - [`prepared`]: Prepared declaration structures consumed by the solver.
//! - [`substitution`]: Generic substitution environments and applied-node keys.
//! - [`relate`]: Tri-state assignability and unification engine.
//! - [`project`]: Demand-driven projections: member, keyspace, surface, normalize.
//! - [`recursion`]: SCC discovery, cycle classes, fixed-point handling.
//! - [`builtin`]: Built-in TypeScript utility type semantics.
//! - [`display`]: Debug/display helpers for traces and tests.

pub mod arena;
pub mod builtin;
pub mod display;
pub mod host;
pub mod lower;
pub mod prepared;
pub mod project;
pub mod recursion;
pub mod relate;
pub mod result;
pub mod solve;
pub mod substitution;

// ---------------------------------------------------------------------------
// Re-exports for ergonomic access
// ---------------------------------------------------------------------------

pub use host::{
    NoopSolverHost, RequestStatus, ResolvedRootIdentity, SolverProjection, TypeSolverHost,
    UtilitySource,
};
pub use prepared::{PreparedTypeDecl, PreparedValueDecl};
pub use result::{
    ExecutionStatus, IncompleteReason, RelationMode, RelationResult, SolverExactness, SolverResult,
};
