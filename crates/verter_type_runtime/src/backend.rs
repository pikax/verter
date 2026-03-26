//! Backend session traits and error types.
//!
//! Defines [`GeneratedQueryBackend`] — the trait that tsserver and TSGO sessions
//! implement for file sync and type queries on generated artifacts.
//!
//! This crate exposes backend sessions, NOT semantic expanders. The semantic
//! expansion API (`TypeExpander`) lives in `verter_resolver`.

use std::future::Future;
use std::pin::Pin;

// ---------------------------------------------------------------------------
// Backend Errors
// ---------------------------------------------------------------------------

/// Errors from backend runtime operations.
///
/// These are translated into `TypeExpansionError` by the resolver layer.
/// Runtime implementation details do not leak past the resolver boundary.
#[derive(Debug, Clone)]
pub enum BackendError {
    /// Backend process is not running or could not be started.
    Unavailable,
    /// Backend process failed to start.
    StartupFailed(String),
    /// Transport connection was closed.
    TransportClosed,
    /// Query timed out.
    TimedOut,
    /// Unexpected response format from the backend.
    ProtocolViolation(String),
    /// The backend does not support this query type.
    UnsupportedQuery,
    /// The backend reported an error.
    BackendReported(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "backend unavailable"),
            Self::StartupFailed(msg) => write!(f, "backend startup failed: {msg}"),
            Self::TransportClosed => write!(f, "transport connection closed"),
            Self::TimedOut => write!(f, "query timed out"),
            Self::ProtocolViolation(msg) => write!(f, "protocol violation: {msg}"),
            Self::UnsupportedQuery => write!(f, "unsupported query"),
            Self::BackendReported(msg) => write!(f, "backend error: {msg}"),
        }
    }
}

impl std::error::Error for BackendError {}

// ---------------------------------------------------------------------------
// Generated File Identity
// ---------------------------------------------------------------------------

/// Typed runtime identity for a generated artifact within a backend session.
///
/// Replaces raw string paths at the runtime seam. Scoped to the owning
/// runtime session for unambiguous cleanup and eviction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GeneratedFileId {
    /// Canonical file ID of the source SFC.
    pub canonical_id: String,
    /// Which artifact profile produced this file.
    pub profile: ArtifactProfile,
    /// Session-local key for disambiguation.
    pub runtime_key: String,
}

/// Artifact profile — controls what the generated file contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactProfile {
    /// Minimal script for type expansion.
    ComponentMeta,
    /// Full IDE artifact (LSP path).
    Lsp,
}

// ---------------------------------------------------------------------------
// Backend Type Query/Result
// ---------------------------------------------------------------------------

/// What kind of type information to query from the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTypeQuery {
    /// Get the type text at the given offset.
    TypeAtOffset,
    /// Get the members (properties) of the type at the given offset.
    MembersAtOffset,
    /// Get the documentation text at the given offset.
    DocumentationAtOffset,
}

/// Universal response envelope from a backend type query.
///
/// Queries may leave non-requested fields empty. Returns `UnsupportedQuery`
/// instead if the backend cannot support the query at all.
#[derive(Debug, Clone, Default)]
pub struct BackendTypeData {
    /// Type text as returned by the backend (e.g., `checker.typeToString()` output).
    pub type_text: Option<String>,
    /// Members of the type (for object types).
    pub members: Vec<BackendTypeMember>,
    /// Documentation text.
    pub documentation: Option<String>,
    /// How complete is this result?
    pub completeness: BackendTypeCompleteness,
}

/// A member of a type as reported by the backend.
#[derive(Debug, Clone)]
pub struct BackendTypeMember {
    pub name: String,
    pub type_text: Option<String>,
    pub optional: bool,
    pub documentation: Option<String>,
}

/// How complete is the backend's type data?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendTypeCompleteness {
    /// Fully resolved.
    #[default]
    Exact,
    /// Partially resolved (some members may be missing).
    Partial,
    /// Could not resolve.
    Failed,
}

// ---------------------------------------------------------------------------
// Boxed Future
// ---------------------------------------------------------------------------

/// A boxed, Send future for backend operations.
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + Send + 'a>>;

// ---------------------------------------------------------------------------
// GeneratedQueryBackend trait
// ---------------------------------------------------------------------------

/// A running TypeScript backend session (tsserver or TSGO).
///
/// Exposes backend sessions, not semantic expanders.
/// The semantic expansion API (`TypeExpander`) lives in `verter_resolver` and
/// consumes this trait.
pub trait GeneratedQueryBackend: Send + Sync {
    /// Sync a generated file into the backend session.
    ///
    /// `revision` tracks the source snapshot used to build the content.
    /// The backend caches content by `file_id` and only re-sends if content changed.
    fn sync_file<'a>(
        &'a self,
        file_id: &'a GeneratedFileId,
        revision: u64,
        content: &'a str,
    ) -> BackendFuture<'a, ()>;

    /// Close a generated file in the backend session.
    fn close_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()>;

    /// Evict a generated file — close and remove from internal caches.
    fn evict_file<'a>(&'a self, file_id: &'a GeneratedFileId) -> BackendFuture<'a, ()>;

    /// Query type data at a generated offset.
    ///
    /// `expected_revision` must match the currently synced revision for this file.
    /// If it doesn't (stale query), returns `BackendError::ProtocolViolation`.
    fn query_type_data<'a>(
        &'a self,
        file_id: &'a GeneratedFileId,
        expected_revision: u64,
        generated_offset: u32,
        query: BackendTypeQuery,
    ) -> BackendFuture<'a, BackendTypeData>;

    /// Gracefully shut down the backend session.
    fn shutdown(&self) -> BackendFuture<'_, ()>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_error_display() {
        assert_eq!(BackendError::Unavailable.to_string(), "backend unavailable");
        assert!(BackendError::TimedOut.to_string().contains("timed out"));
    }

    #[test]
    fn backend_type_completeness_default_is_exact() {
        assert_eq!(
            BackendTypeCompleteness::default(),
            BackendTypeCompleteness::Exact
        );
    }

    #[test]
    fn generated_file_id_equality() {
        let a = GeneratedFileId {
            canonical_id: "/src/A.vue".into(),
            profile: ArtifactProfile::ComponentMeta,
            runtime_key: "session-1".into(),
        };
        let b = GeneratedFileId {
            canonical_id: "/src/A.vue".into(),
            profile: ArtifactProfile::ComponentMeta,
            runtime_key: "session-1".into(),
        };
        let c = GeneratedFileId {
            canonical_id: "/src/A.vue".into(),
            profile: ArtifactProfile::Lsp,
            runtime_key: "session-1".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c, "different profile should differ");
    }

    #[test]
    fn backend_type_data_default_is_empty() {
        let data = BackendTypeData::default();
        assert!(data.type_text.is_none());
        assert!(data.members.is_empty());
        assert!(data.documentation.is_none());
        assert_eq!(data.completeness, BackendTypeCompleteness::Exact);
    }
}
