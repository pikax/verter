//! Shared types for the retired native type solver kernel.
//!
//! The arena-based solver (`solve_type`, `relate`, `project`, `TypeQueryEngine`,
//! `TypeSolverHost`) has been retired as part of the D-Cutover. What survives
//! here are the compact data carriers that remain in use across
//! `verter_session`, `verter_ffi`, and the shared semantic dispatch layer:
//!
//! - [`arena`] — append-only query-local node store (still used as a scratch
//!   representation by prepared declarations and `display`-free diagnostics).
//! - [`builtin`] — `BuiltinUtility` enum + metadata (name, arity, compiler
//!   intrinsic classification). Consumed by the intrinsic registry and
//!   dispatch lower.
//! - [`host`] — identity / utility classification types
//!   (`ResolvedRootIdentity`, `UtilitySource`, `BareRefOrigin`,
//!   `RequestStatus`). The `TypeSolverHost` trait has been retired.
//! - [`prepared`] — prepared declaration bodies consumed by dispatch and
//!   component-meta's query engine.
//! - [`query_engine`] — projection result carriers (`ProjectedSurface`,
//!   `ProjectedMember`, `ProjectedKeyspace`). The standalone `TypeQueryEngine`
//!   itself is retired.
//! - [`result`] — solver result exactness / execution status (used by the
//!   expansion pipeline and FFI).

pub mod arena;
pub mod builtin;
pub mod host;
pub mod prepared;
pub mod query_engine;
pub mod result;

// ---------------------------------------------------------------------------
// Re-exports for ergonomic access
// ---------------------------------------------------------------------------

pub use host::{BareRefOrigin, RequestStatus, ResolvedRootIdentity, UtilitySource};
pub use prepared::{PreparedTypeDecl, PreparedValueDecl};
pub use query_engine::{ProjectedKeyspace, ProjectedMember, ProjectedSurface};
pub use result::{
    ExecutionStatus, IncompleteReason, SolverDiagnostic, SolverExactness, SolverResult,
};
