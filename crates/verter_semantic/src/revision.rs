//! Revision tracking for semantic query invalidation.
//!
//! A [`RevisionMarker`] identifies the exact input snapshot set used for a query
//! result. Revision markers are session-scoped — they are comparable only within
//! the same session/workspace context.
//!
//! [`SemanticDependency`] declares what a query depends on so the session can
//! decide what to materialize next when dependencies are missing.

use serde::{Deserialize, Serialize};

/// Monotonically increasing revision counter within a session.
///
/// Each input domain (workspace, parser, compiler, provider) has its own
/// revision counter that increments on every change.
pub type Revision = u64;

/// Identifies the exact input snapshot set used for a query result.
///
/// Revision markers are session-scoped snapshot identities, not global content
/// hashes. They are comparable only within the same session/workspace context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevisionMarker {
    pub workspace_revision: Revision,
    pub parser_revision: Revision,
    pub compiler_revision: Revision,
    pub provider_revision: Revision,
}

impl RevisionMarker {
    /// Create a revision marker with all domains at zero (initial state).
    pub fn initial() -> Self {
        Self {
            workspace_revision: 0,
            parser_revision: 0,
            compiler_revision: 0,
            provider_revision: 0,
        }
    }

    /// Returns true if any domain revision in `self` is newer than `other`.
    pub fn is_newer_than(&self, other: &RevisionMarker) -> bool {
        self.workspace_revision > other.workspace_revision
            || self.parser_revision > other.parser_revision
            || self.compiler_revision > other.compiler_revision
            || self.provider_revision > other.provider_revision
    }
}

/// What kind of input a semantic query depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DependencyKind {
    /// Workspace source text, resolution, or config.
    WorkspaceResolution,
    /// Parser-owned parsed snapshot for a file.
    ParserSnapshot,
    /// Compiler-owned lowered IR for a file.
    CompilerIr,
    /// External type provider snapshot.
    ProviderSnapshot,
    /// A resolved cross-file fact from the semantic DB itself.
    SemanticFact,
}

/// A single declared dependency of a semantic query.
///
/// The semantic query engine returns these so the session can decide what
/// to materialize next when inputs are missing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticDependency {
    pub kind: DependencyKind,
    /// Canonical file ID or other stable key identifying the dependency target.
    pub key: String,
    /// The revision at which this dependency was observed.
    pub revision: Revision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_revision_is_all_zeros() {
        let r = RevisionMarker::initial();
        assert_eq!(r.workspace_revision, 0);
        assert_eq!(r.parser_revision, 0);
        assert_eq!(r.compiler_revision, 0);
        assert_eq!(r.provider_revision, 0);
    }

    #[test]
    fn is_newer_than_detects_any_domain_advance() {
        let base = RevisionMarker::initial();
        let newer_ws = RevisionMarker {
            workspace_revision: 1,
            ..base
        };
        assert!(newer_ws.is_newer_than(&base));
        assert!(!base.is_newer_than(&newer_ws));
    }

    #[test]
    fn is_newer_than_returns_false_for_equal() {
        let r = RevisionMarker::initial();
        assert!(!r.is_newer_than(&r));
    }

    #[test]
    fn semantic_dependency_equality() {
        let d1 = SemanticDependency {
            kind: DependencyKind::ParserSnapshot,
            key: "file.vue".into(),
            revision: 3,
        };
        let d2 = d1.clone();
        assert_eq!(d1, d2);

        let d3 = SemanticDependency {
            kind: DependencyKind::CompilerIr,
            key: "file.vue".into(),
            revision: 3,
        };
        assert_ne!(d1, d3);
    }
}
