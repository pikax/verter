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

/// Typed reason an exact closed string-literal domain could not be decided.
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
pub enum ClosedLiteralDomainUnresolvedReason {
    AnalysisUnavailable,
    RevisionMismatch,
    MissingDependency,
    Cycle,
    BudgetExceeded,
    WorkLimitExceeded,
    Cancelled,
    Unsupported,
    Fault,
}

/// Exact closed string-literal domain of one semantic type demand.
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
pub enum ClosedLiteralDomain {
    /// Every demanded arm resolved and was a string literal. Members retain
    /// semantic union order and are de-duplicated by first occurrence.
    Strings(Arc<[Arc<str>]>),
    /// Resolution was complete and proved the domain open or non-string.
    NotClosed,
    /// Resolution was incomplete. No closed subset may be published.
    Unresolved {
        reason: ClosedLiteralDomainUnresolvedReason,
        exactness: ResolutionExactness,
    },
}

impl Default for ClosedLiteralDomain {
    fn default() -> Self {
        Self::Unresolved {
            reason: ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable,
            exactness: ResolutionExactness::Incomplete,
        }
    }
}

/// Typed reason a reactive-wrapper role could not be decided.
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
pub enum ReactiveWrapperUnresolvedReason {
    AnalysisUnavailable,
    RevisionMismatch,
    MissingDependency,
    Cycle,
    BudgetExceeded,
    WorkLimitExceeded,
    Cancelled,
    Unsupported,
    Fault,
}

/// Package-backed reactive wrapper family of one semantic type demand.
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
pub enum ReactiveWrapperRole {
    Ref,
    ShallowRef,
    ComputedRef,
    /// `defineModel`'s wrapper type. Kept as its OWN role rather than collapsed
    /// into [`Ref`](Self::Ref): it is the one wrapper source the compiler itself
    /// synthesises, and a consumer distinguishing a model binding from a plain
    /// ref must be able to.
    ModelRef,
    Reactive,
    ShallowReactive,
    None,
    Unresolved {
        reason: ReactiveWrapperUnresolvedReason,
    },
}

impl Default for ReactiveWrapperRole {
    fn default() -> Self {
        Self::Unresolved {
            reason: ReactiveWrapperUnresolvedReason::AnalysisUnavailable,
        }
    }
}

/// Exact package/import provenance for a wrapper-head identity.
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
pub struct ReactiveWrapperImportProvenance {
    pub authored_head: crate::facts::AuthoredReferenceHeadFact,
    pub package: Arc<str>,
    pub import_source: Arc<str>,
    pub local_binding: Arc<str>,
    pub owner_canonical: Arc<str>,
    pub imported_name: Arc<str>,
    pub terminal_import_source: Arc<str>,
    pub local_alias_hops: Arc<[Arc<str>]>,
    pub exactness: ResolutionExactness,
    pub provenance: ResolutionProvenance,
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
