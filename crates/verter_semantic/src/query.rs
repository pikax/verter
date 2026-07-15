//! Shared query result types for the semantic engine.
//!
//! All public session/protocol queries return [`QueryResult<T>`], which
//! provides a common envelope for complete, partial, and unavailable results
//! across native, WASM, MCP, LSP, and NAPI boundaries.

use serde::{Deserialize, Serialize};

use crate::revision::{RevisionMarker, SemanticDependency};

/// Whether a query result is fully resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Completeness {
    /// All dependencies were available; the result is authoritative.
    Complete,
    /// Some dependencies were missing; the result is best-effort.
    Partial,
    /// Critical dependencies were unavailable; no meaningful result.
    Unavailable,
}

/// Common result envelope for semantic queries.
///
/// One shared type for complete, partial, and unavailable outcomes across
/// all consumer boundaries (native, WASM, MCP, LSP, NAPI).
#[derive(Debug, Clone)]
pub struct QueryResult<T> {
    /// The query value (may be a default/partial value when not Complete).
    pub value: T,
    /// The input snapshot revisions used to produce this result.
    pub revision_marker: RevisionMarker,
    /// Whether the result is fully resolved.
    pub completeness: Completeness,
    /// Dependencies that were unavailable during evaluation.
    pub missing_inputs: Vec<SemanticDependency>,
    /// True if the caller-provided ref no longer resolves at this revision.
    pub stale_ref: bool,
}

impl<T> QueryResult<T> {
    /// Create a complete result with no missing inputs.
    pub fn complete(value: T, revision: RevisionMarker) -> Self {
        Self {
            value,
            revision_marker: revision,
            completeness: Completeness::Complete,
            missing_inputs: Vec::new(),
            stale_ref: false,
        }
    }

    /// Create a partial result indicating some inputs were missing.
    pub fn partial(value: T, revision: RevisionMarker, missing: Vec<SemanticDependency>) -> Self {
        Self {
            value,
            revision_marker: revision,
            completeness: Completeness::Partial,
            missing_inputs: missing,
            stale_ref: false,
        }
    }

    /// Create an unavailable result.
    pub fn unavailable(
        value: T,
        revision: RevisionMarker,
        missing: Vec<SemanticDependency>,
    ) -> Self {
        Self {
            value,
            revision_marker: revision,
            completeness: Completeness::Unavailable,
            missing_inputs: missing,
            stale_ref: false,
        }
    }

    /// Map the value while preserving metadata.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> QueryResult<U> {
        QueryResult {
            value: f(self.value),
            revision_marker: self.revision_marker,
            completeness: self.completeness,
            missing_inputs: self.missing_inputs,
            stale_ref: self.stale_ref,
        }
    }

    /// Returns true if the result is fully resolved.
    pub fn is_complete(&self) -> bool {
        self.completeness == Completeness::Complete
    }
}

/// Internal evaluation result with dependency tracking.
///
/// Used inside the semantic engine to track what a query depended on.
/// The session uses `dependencies` and `missing_dependencies` to decide
/// what to materialize next.
#[derive(Debug, Clone)]
pub struct SemanticEvaluation<T> {
    pub value: T,
    pub completeness: Completeness,
    pub dependencies: Vec<SemanticDependency>,
    pub missing_dependencies: Vec<SemanticDependency>,
}

impl<T> SemanticEvaluation<T> {
    pub fn complete(value: T, deps: Vec<SemanticDependency>) -> Self {
        Self {
            value,
            completeness: Completeness::Complete,
            dependencies: deps,
            missing_dependencies: Vec::new(),
        }
    }

    pub fn partial(
        value: T,
        deps: Vec<SemanticDependency>,
        missing: Vec<SemanticDependency>,
    ) -> Self {
        Self {
            value,
            completeness: Completeness::Partial,
            dependencies: deps,
            missing_dependencies: missing,
        }
    }

    /// Promote to a [`QueryResult`] by attaching a revision marker.
    pub fn into_query_result(self, revision: RevisionMarker) -> QueryResult<T> {
        QueryResult {
            value: self.value,
            revision_marker: revision,
            completeness: self.completeness,
            missing_inputs: self.missing_dependencies,
            stale_ref: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_result_has_no_missing() {
        let r = QueryResult::complete(42, RevisionMarker::initial());
        assert!(r.is_complete());
        assert!(r.missing_inputs.is_empty());
        assert!(!r.stale_ref);
    }

    #[test]
    fn partial_result_carries_missing_deps() {
        use crate::revision::DependencyKind;
        let missing = vec![SemanticDependency {
            kind: DependencyKind::ProviderSnapshot,
            key: "tsgo".into(),
            revision: 0,
        }];
        let r = QueryResult::partial(42, RevisionMarker::initial(), missing.clone());
        assert_eq!(r.completeness, Completeness::Partial);
        assert_eq!(r.missing_inputs.len(), 1);
        assert_eq!(r.missing_inputs[0].kind, DependencyKind::ProviderSnapshot);
    }

    #[test]
    fn map_preserves_metadata() {
        let r = QueryResult::complete(10, RevisionMarker::initial());
        let mapped = r.map(|v| v * 2);
        assert_eq!(mapped.value, 20);
        assert!(mapped.is_complete());
    }

    #[test]
    fn evaluation_promotes_to_query_result() {
        let eval = SemanticEvaluation::complete(99, vec![]);
        let marker = RevisionMarker {
            workspace_revision: 5,
            ..RevisionMarker::initial()
        };
        let result = eval.into_query_result(marker);
        assert_eq!(result.value, 99);
        assert_eq!(result.revision_marker.workspace_revision, 5);
        assert!(result.is_complete());
    }

    #[test]
    fn unavailable_result() {
        let r: QueryResult<Option<i32>> =
            QueryResult::unavailable(None, RevisionMarker::initial(), vec![]);
        assert_eq!(r.completeness, Completeness::Unavailable);
        assert!(!r.is_complete());
    }

    #[test]
    fn stale_ref_flag() {
        let mut r = QueryResult::complete(10, RevisionMarker::initial());
        assert!(!r.stale_ref);
        r.stale_ref = true;
        assert!(r.stale_ref);
        // Still complete even with stale ref
        assert!(r.is_complete());
    }

    #[test]
    fn semantic_evaluation_partial_carries_both_dep_lists() {
        use crate::revision::DependencyKind;
        let deps = vec![SemanticDependency {
            kind: DependencyKind::ParserSnapshot,
            key: "a.vue".into(),
            revision: 1,
        }];
        let missing = vec![SemanticDependency {
            kind: DependencyKind::ProviderSnapshot,
            key: "tsgo".into(),
            revision: 0,
        }];
        let eval = SemanticEvaluation::partial(42, deps.clone(), missing.clone());
        assert_eq!(eval.completeness, Completeness::Partial);
        assert_eq!(eval.dependencies.len(), 1);
        assert_eq!(eval.missing_dependencies.len(), 1);

        // Promote to QueryResult
        let result = eval.into_query_result(RevisionMarker::initial());
        assert_eq!(result.completeness, Completeness::Partial);
        assert_eq!(result.missing_inputs.len(), 1);
        assert_eq!(result.missing_inputs[0].key, "tsgo");
    }

    #[test]
    fn map_on_partial_preserves_completeness() {
        use crate::revision::DependencyKind;
        let missing = vec![SemanticDependency {
            kind: DependencyKind::WorkspaceResolution,
            key: "ext".into(),
            revision: 0,
        }];
        let r = QueryResult::partial(10, RevisionMarker::initial(), missing);
        let mapped = r.map(|v| v.to_string());
        assert_eq!(mapped.value, "10");
        assert_eq!(mapped.completeness, Completeness::Partial);
        assert_eq!(mapped.missing_inputs.len(), 1);
    }
}
