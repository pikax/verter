//! Revision-stamped semantic facts for dynamic template-class subjects.
//!
//! This artifact is intentionally demand-shaped: it contains only bindings and
//! prop members selected from one `RawTemplateData` value by the session. It
//! stores no `TypeExpr`, semantic graph handle, or display string authority.

use std::sync::Arc;

use verter_no_typeexpr::NoTypeExpr;
use verter_type_expr::facts::SemanticTypeSource;
use verter_type_expr::locators::MacroPayloadLocator;
use verter_type_expr::{
    ClosedLiteralDomain, DeclBindingKey, ReactiveWrapperImportProvenance, ReactiveWrapperRole,
    ResolvedSymbolIdentity, TypeExprScope,
};

use super::Hash16;

/// Exact identity of a requested dynamic-class subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, NoTypeExpr)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TemplateClassSubject {
    Binding {
        /// Converter lookup label only; declaration is semantic authority.
        label: Arc<str>,
        declaration: DeclBindingKey,
    },
    Prop {
        /// Converter lookup label only; payload is semantic authority.
        label: Arc<str>,
        props_root: Arc<str>,
        payload: MacroPayloadLocator,
        scope: TypeExprScope,
    },
    /// The raw template requested a subject that could not be joined to an
    /// exact same-revision declaration/locator.
    Unresolved {
        label: Arc<str>,
        props_root: Option<Arc<str>>,
    },
}

impl TemplateClassSubject {
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Binding { label, .. }
            | Self::Prop { label, .. }
            | Self::Unresolved { label, .. } => label,
        }
    }
}

/// Whether a requested fact set is safe to treat as exact reusable output.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub enum TemplateClassFactsCompleteness {
    Complete,
    ReturnOnly,
}

/// Exact reactive-wrapper proof for one requested subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct ReactiveWrapperProof {
    pub role: ReactiveWrapperRole,
    /// Exact resolved wrapper head. Absent for complete non-wrapper proofs and
    /// unresolved subjects.
    pub symbol: Option<ResolvedSymbolIdentity>,
    /// Exact package/import route evidence for `symbol`.
    pub import_provenance: Option<ReactiveWrapperImportProvenance>,
    /// Typed semantic source of the wrapper's inner argument when faithfully
    /// representable without retaining a graph handle.
    pub inner_source: Option<SemanticTypeSource>,
    /// Closed-domain decision for the wrapper inner argument.
    pub inner_domain: ClosedLiteralDomain,
    pub completeness: TemplateClassFactsCompleteness,
}

/// One requested subject's exact semantic decision.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct TemplateClassSemanticFactRow {
    pub subject: TemplateClassSubject,
    /// Effective class domain: the wrapper's inner domain for a proven wrapper,
    /// otherwise the subject type's domain.
    pub domain: ClosedLiteralDomain,
    pub wrapper: ReactiveWrapperProof,
}

/// Immutable revision-stamped template-class fact projection.
///
/// `S` is the owning session's dependency-signature carrier. Keeping it generic
/// lets this neutral crate define the artifact without depending upward on
/// `verter_session`; production instantiates it with `ReadSetSignature`.
#[derive(Debug, Clone)]
pub struct TemplateClassSemanticFacts<S> {
    owner_canonical: Arc<str>,
    owner_whole_hash: Hash16,
    requested_subjects: Arc<[TemplateClassSubject]>,
    rows: Arc<[TemplateClassSemanticFactRow]>,
    completeness: TemplateClassFactsCompleteness,
    dependency_signature: S,
}

impl<S> TemplateClassSemanticFacts<S> {
    #[must_use]
    pub fn new(
        owner_canonical: Arc<str>,
        owner_whole_hash: Hash16,
        requested_subjects: Arc<[TemplateClassSubject]>,
        rows: Arc<[TemplateClassSemanticFactRow]>,
        completeness: TemplateClassFactsCompleteness,
        dependency_signature: S,
    ) -> Self {
        Self {
            owner_canonical,
            owner_whole_hash,
            requested_subjects,
            rows,
            completeness,
            dependency_signature,
        }
    }

    #[must_use]
    pub fn owner_canonical(&self) -> &str {
        &self.owner_canonical
    }

    #[must_use]
    pub const fn owner_whole_hash(&self) -> Hash16 {
        self.owner_whole_hash
    }

    #[must_use]
    pub fn requested_subjects(&self) -> &[TemplateClassSubject] {
        &self.requested_subjects
    }

    #[must_use]
    pub fn rows(&self) -> &[TemplateClassSemanticFactRow] {
        &self.rows
    }

    #[must_use]
    pub const fn completeness(&self) -> TemplateClassFactsCompleteness {
        self.completeness
    }

    #[must_use]
    pub fn dependency_signature(&self) -> &S {
        &self.dependency_signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_type_expr::{
        ClosedLiteralDomainUnresolvedReason, ReactiveWrapperUnresolvedReason, ResolutionExactness,
        TopLevelOwnerId,
    };

    #[test]
    fn template_class_artifact_keeps_exact_revision_and_requested_rows_only() {
        let subject = TemplateClassSubject::Binding {
            label: Arc::from("variant"),
            declaration: DeclBindingKey::new(TopLevelOwnerId::ordinary_file(), "variant"),
        };
        let row = TemplateClassSemanticFactRow {
            subject: subject.clone(),
            domain: ClosedLiteralDomain::Strings(Arc::from([
                Arc::<str>::from("primary"),
                Arc::<str>::from("secondary"),
            ])),
            wrapper: ReactiveWrapperProof {
                role: ReactiveWrapperRole::None,
                symbol: None,
                import_provenance: None,
                inner_source: None,
                inner_domain: ClosedLiteralDomain::NotClosed,
                completeness: TemplateClassFactsCompleteness::Complete,
            },
        };
        let facts = TemplateClassSemanticFacts::new(
            Arc::from("/src/App.vue"),
            [7; 16],
            Arc::from([subject]),
            Arc::from([row]),
            TemplateClassFactsCompleteness::Complete,
            41_u64,
        );
        assert_eq!(facts.owner_whole_hash(), [7; 16]);
        assert_eq!(facts.requested_subjects().len(), 1);
        assert_eq!(facts.rows().len(), 1);
        assert_eq!(*facts.dependency_signature(), 41);

        let unresolved = ReactiveWrapperProof {
            role: ReactiveWrapperRole::Unresolved {
                reason: ReactiveWrapperUnresolvedReason::AnalysisUnavailable,
            },
            symbol: None,
            import_provenance: None,
            inner_source: None,
            inner_domain: ClosedLiteralDomain::Unresolved {
                reason: ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable,
                exactness: ResolutionExactness::Incomplete,
            },
            completeness: TemplateClassFactsCompleteness::ReturnOnly,
        };
        assert_eq!(
            unresolved.completeness,
            TemplateClassFactsCompleteness::ReturnOnly
        );
    }
}
