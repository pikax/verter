//! Closed semantic roles consumed by feature-specific analysis.

use std::sync::Arc;

use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

use crate::{ResolutionExactness, ResolutionProvenance, TopLevelOwnerId};

/// Content-free identity of one resolved declaration symbol.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSymbolIdentity {
    /// Canonical file that defines the symbol.
    pub canonical_id: Arc<str>,
    /// Exact neutral lexical owner of the declaration.
    pub owner: TopLevelOwnerId,
    /// Declaration symbol name in the defining file.
    pub symbol: Arc<str>,
}

/// Typed reason a prop callable role could not be decided.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum PropCallableRoleUnresolvedReason {
    /// No role-producing semantic analysis was available.
    AnalysisUnavailable,
    /// A required imported declaration could not be resolved.
    MissingDependency,
    /// Carrier resolution encountered a semantic cycle.
    Cycle,
    /// A semantic evaluator budget was exhausted.
    BudgetExceeded,
    /// The connected demand work or query-depth envelope was exhausted.
    WorkLimitExceeded,
    /// The carrier shape is not supported by the identity demand.
    Unsupported,
    /// Resolution failed for another typed fault.
    Fault,
}

/// Callable role of one resolved component prop.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PropCallableRole {
    /// The prop resolves to Svelte's package-backed `Snippet` symbol.
    SvelteSnippet {
        /// Exact resolved symbol identity.
        symbol: ResolvedSymbolIdentity,
        /// Exactness of the role proof.
        exactness: ResolutionExactness,
        /// Producer that established the role.
        provenance: ResolutionProvenance,
    },
    /// The prop was completely resolved and is not Svelte's `Snippet`.
    Other,
    /// The role could not be decided completely.
    Unresolved {
        /// Typed fail-closed reason.
        reason: PropCallableRoleUnresolvedReason,
    },
}

impl Default for PropCallableRole {
    fn default() -> Self {
        Self::Unresolved {
            reason: PropCallableRoleUnresolvedReason::AnalysisUnavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_callable_role_default_is_fail_closed() {
        assert_eq!(
            PropCallableRole::default(),
            PropCallableRole::Unresolved {
                reason: PropCallableRoleUnresolvedReason::AnalysisUnavailable,
            }
        );
    }

    #[test]
    fn svelte_snippet_role_round_trips_exact_identity() {
        let role = PropCallableRole::SvelteSnippet {
            symbol: ResolvedSymbolIdentity {
                canonical_id: Arc::from("/node_modules/svelte/index.d.ts"),
                owner: TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("Snippet"),
            },
            exactness: ResolutionExactness::ExactSymbolic,
            provenance: ResolutionProvenance::FrameworkSurface,
        };
        let encoded = serde_json::to_string(&role).expect("serialize role");
        let decoded: PropCallableRole = serde_json::from_str(&encoded).expect("deserialize role");
        assert_eq!(decoded, role);
    }
}
