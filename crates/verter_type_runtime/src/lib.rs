//! External TypeScript backend sessions for Verter type expansion.
//!
//! This crate owns the runtime concerns for communicating with external
//! TypeScript backends (tsserver, TSGO):
//!
//! - Process spawning and IPC transport
//! - Backend discovery (`find_tsserver`, `find_node`)
//! - File sync into backend sessions
//! - Minimal session lifecycle (start, shutdown, reconnect)
//! - Backend-specific query helpers
//!
//! It does NOT own:
//! - Semantic expansion request/result types (→ `verter_session::resolver_core`)
//! - SFC-origin request contracts (→ `verter_session::resolver_core`)
//! - Editor restart policy (→ `verter_lsp`)
//! - Background workspace sync policy (→ `verter_lsp`)
//! - Merged diagnostics strategy (→ `verter_lsp`)
//!
//! # Dependency Direction
//!
//! ```text
//! verter_session::resolver_core ──→ verter_type_runtime (for backend-backed expansion)
//! verter_lsp ────────→ verter_type_runtime (for sessions + orchestration)
//! ```
//!
//! `verter_type_runtime` does NOT depend on `verter_session::resolver_core` or `verter_session`.

pub mod backend;
pub mod codec;
pub mod contents_snapshot;
pub mod discovery;
pub mod protocol;
pub mod provider_adapter;
pub mod resilient;
pub mod trace;
pub mod traits;
pub mod tsgo;
pub mod tsserver;
pub mod uri;

// Re-exports for convenience
pub use backend::{
    ArtifactProfile, BackendError, BackendFuture, BackendTypeCompleteness, BackendTypeData,
    BackendTypeMember, BackendTypeQuery, GeneratedFileId, GeneratedQueryBackend,
};
pub use codec::{
    line_column_to_offset, line_column_to_offset_utf16, offset_to_line_column,
    offset_to_line_column_utf16, LineColumn, LineIndex, PositionEncoding,
};
pub use discovery::{detect_ts_major_version, find_node, find_tsserver};
pub use protocol::*;
pub use provider_adapter::TypeProviderAdapter;
pub use trace::{
    current_type_runtime_trace_context, format_type_runtime_trace_line, type_runtime_trace_enabled,
    type_runtime_trace_event, type_runtime_trace_scope_async, with_type_runtime_trace_context,
    with_type_runtime_trace_context_async, TypeRuntimeTraceContext, TypeRuntimeTraceEvent,
};
pub use traits::{ProviderFuture, ProviderPriority, TypeProvider};
pub use uri::{
    file_uri_to_path, normalize_file_uri_for_cache, path_to_file_uri_string, percent_decode,
};
