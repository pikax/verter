//! Semantic type expansion contract.
//!
//! Defines the [`TypeExpander`] trait and associated request/result types.
//! Three backends implement this trait:
//! - `VerterTypeExpander` — native OXC-based resolution (this crate)
//! - `TsserverTypeExpander` — tsserver-backed expansion (this crate, consumes `verter_type_runtime`)
//! - `TsgoTypeExpander` — TSGO-backed expansion (this crate, consumes `verter_type_runtime`)
//!
//! # Request Invariant
//!
//! All expansion requests use canonical file ID + SFC-absolute `Span`.
//! Never raw script content + offset.
//!
//! # Coordinate System
//!
//! The source-of-truth coordinate system is always Verter's SFC-absolute spans.
//! Backend sessions (tsserver/TSGO) operate on generated artifacts with explicit
//! span mappings — the translation is handled by the artifact layer.

use std::future::Future;
use std::pin::Pin;

use verter_analysis::type_expr::TypeExpr;
use verter_span::Span;

// ---------------------------------------------------------------------------
// Expansion Profile
// ---------------------------------------------------------------------------

/// Controls what the generated artifact includes and how expansion behaves.
///
/// Backend selection is per session/project config.
/// Profile selection is per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpansionProfile {
    /// Minimal generated script: imports + type decls + setup body.
    /// No template TSX, no IDE-only helpers.
    ComponentMeta,
    /// Full IDE-oriented generated artifact (existing LSP path).
    Lsp,
}

// ---------------------------------------------------------------------------
// Backend Selection
// ---------------------------------------------------------------------------

/// Which backend handles type expansion for this session/project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TypeExpansionBackend {
    /// Native OXC-based resolver (current Verter behavior).
    #[default]
    Verter,
    /// TypeScript >=5.9 via tsserver IPC.
    Tsserver,
    /// TSGO via LSP JSON-RPC.
    Tsgo,
    /// Deterministic pre-expansion eligibility check, commits to exactly one backend.
    Auto,
}

// ---------------------------------------------------------------------------
// Request / Result
// ---------------------------------------------------------------------------

/// Request to expand a type. Input is always canonical ID + SFC-absolute span.
#[derive(Debug, Clone)]
pub struct TypeExpansionRequest {
    /// Canonical file ID (e.g., "/src/Button.vue").
    pub canonical_id: String,
    /// SFC-absolute span of the type expression to expand.
    pub span: Span,
    /// Controls artifact generation and expansion behavior.
    pub profile: ExpansionProfile,
}

/// Result of expanding a type.
#[derive(Debug, Clone)]
pub struct TypeExpansionResult {
    /// The resolved type expression.
    pub type_expr: TypeExpr,
    /// Expanded members (for object types).
    pub members: Vec<ExpandedMember>,
    /// How complete is this expansion?
    pub completeness: ExpansionCompleteness,
}

/// How complete is the expansion result?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionCompleteness {
    /// Fully resolved — all members and types are structurally known.
    Exact,
    /// Partially resolved — some members or nested types may be missing.
    LowerBound,
    /// The type was resolved but could not be structurally decomposed.
    /// The `type_expr` may contain raw type text from the backend.
    OpaqueFallback,
}

/// A member of an expanded object type.
#[derive(Debug, Clone)]
pub struct ExpandedMember {
    /// Property name.
    pub name: String,
    /// Resolved type expression for this member.
    pub type_expr: TypeExpr,
    /// Raw backend/display type text for this member, if available.
    pub raw_type: Option<String>,
    /// Whether this member is optional (`?`).
    pub optional: bool,
    /// JSDoc or documentation description, if available.
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the type expansion layer.
///
/// Runtime implementation details (transport, process) do not leak past this
/// boundary — they are normalized into `BackendFailure`.
#[derive(Debug, Clone)]
pub enum TypeExpansionError {
    /// The source file could not be found or loaded.
    SourceUnavailable,
    /// The request was malformed (invalid span, unsupported profile, etc.).
    InvalidRequest,
    /// SFC span → generated offset mapping failed.
    MappingFailed,
    /// The backend returned no expansion result for this span.
    NoExpansionResult,
    /// The selected backend does not support this query.
    UnsupportedByBackend,
    /// The backend process failed.
    BackendFailure(BackendFailureKind),
}

impl std::fmt::Display for TypeExpansionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceUnavailable => write!(f, "source file unavailable"),
            Self::InvalidRequest => write!(f, "invalid expansion request"),
            Self::MappingFailed => write!(f, "SFC-to-generated span mapping failed"),
            Self::NoExpansionResult => write!(f, "no expansion result from backend"),
            Self::UnsupportedByBackend => write!(f, "unsupported by selected backend"),
            Self::BackendFailure(kind) => write!(f, "backend failure: {kind:?}"),
        }
    }
}

impl std::error::Error for TypeExpansionError {}

/// Kind of backend failure (normalized from runtime errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendFailureKind {
    /// Backend process is not running or could not be started.
    Unavailable,
    /// Backend process died during the query.
    Died,
    /// Query timed out.
    TimedOut,
    /// Protocol violation (unexpected response format).
    ProtocolViolation,
}

// ---------------------------------------------------------------------------
// TypeExpander trait
// ---------------------------------------------------------------------------

/// Boxed async future for type expansion (allows `dyn TypeExpander`).
pub type ExpanderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TypeExpansionError>> + Send + 'a>>;

/// Core trait: expand a type at an SFC-absolute span to its resolved structural form.
///
/// All three backends (Verter, tsserver, TSGO) implement this natively async.
/// The Verter path keeps core semantic work synchronous where appropriate,
/// with the async boundary at host/source-loading and real I/O edges.
pub trait TypeExpander: Send + Sync {
    /// Expand the type at the given SFC span.
    fn expand_type<'a>(
        &'a self,
        request: &'a TypeExpansionRequest,
    ) -> ExpanderFuture<'a, TypeExpansionResult>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_profile_component_meta_and_lsp_are_distinct() {
        assert_ne!(ExpansionProfile::ComponentMeta, ExpansionProfile::Lsp);
    }

    #[test]
    fn backend_default_is_verter() {
        assert_eq!(
            TypeExpansionBackend::default(),
            TypeExpansionBackend::Verter
        );
    }

    #[test]
    fn type_expansion_request_uses_sfc_absolute_span() {
        let request = TypeExpansionRequest {
            canonical_id: "/src/Button.vue".to_string(),
            span: Span::new(100, 120),
            profile: ExpansionProfile::ComponentMeta,
        };
        // Request uses canonical_id + SFC-absolute span, never raw script content
        assert_eq!(request.span.start, 100);
        assert_eq!(request.span.end, 120);
    }

    #[test]
    fn expansion_completeness_variants() {
        // Exact = fully resolved
        let exact = ExpansionCompleteness::Exact;
        // LowerBound = some members may be missing
        let lower = ExpansionCompleteness::LowerBound;
        // OpaqueFallback = resolved but not structurally decomposed
        let opaque = ExpansionCompleteness::OpaqueFallback;

        assert_ne!(exact, lower);
        assert_ne!(lower, opaque);
        assert_ne!(exact, opaque);
    }

    #[test]
    fn expanded_member_carries_optionality_and_description() {
        let member = ExpandedMember {
            name: "count".to_string(),
            type_expr: TypeExpr::primitive(verter_analysis::type_expr::PrimitiveName::Number),
            raw_type: Some("number".to_string()),
            optional: true,
            description: Some("The counter value".to_string()),
        };
        assert!(member.optional);
        assert!(member.description.is_some());
    }

    #[test]
    fn type_expansion_error_display() {
        let err = TypeExpansionError::BackendFailure(BackendFailureKind::TimedOut);
        let msg = err.to_string();
        assert!(msg.contains("backend failure"), "got: {msg}");
        assert!(msg.contains("TimedOut"), "got: {msg}");
    }
}
