//! Shared types consumed by `verter_session`, `verter_ffi`, and the
//! shared semantic dispatch layer:
//!
//! - [`arena`] — append-only query-local node store (used as a scratch
//!   representation by prepared declarations and `display`-free diagnostics).
//! - [`builtin`] — `BuiltinUtility` enum + metadata (name, arity, compiler
//!   intrinsic classification). Consumed by the intrinsic registry and
//!   dispatch lower.
//! - [`host`] — identity / utility classification types
//!   (`ResolvedRootIdentity`, `UtilitySource`, `BareRefOrigin`,
//!   `RequestStatus`).
//! - [`prepared`] — prepared declaration bodies consumed by dispatch and
//!   component-meta's query engine.
//! - [`result`] — solver result exactness / execution status (used by the
//!   expansion pipeline and FFI).

pub mod arena;
pub mod builtin;
pub mod host;
pub mod prepared;
pub mod result;

// ---------------------------------------------------------------------------
// Re-exports for ergonomic access
// ---------------------------------------------------------------------------

pub use host::{BareRefOrigin, RequestStatus, ResolvedRootIdentity, UtilitySource};
pub use prepared::{PreparedTypeDecl, PreparedValueDecl};
pub use result::{
    ExecutionStatus, IncompleteReason, SolverDiagnostic, SolverExactness, SolverResult,
};
