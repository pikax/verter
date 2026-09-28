//! The type predicate a signature node carries beside its return — see
//! [`SemanticNodeData::Signature`](super::SemanticNodeData::Signature).

use super::{split_this_receiver, FunctionParam, SemanticNodeId};

/// The type predicate of a
/// [`SemanticNodeData::Signature`](super::SemanticNodeData::Signature) —
/// TypeScript's `TypePredicate` record: `x is T`, `asserts x is T`,
/// `asserts x`, `this is T`, `asserts this is T`, `asserts this`.
///
/// It rides BESIDE the signature's return exactly as the checker models it:
/// a signature carrying a type predicate returns `boolean`, one carrying an
/// assertion returns `void`, and the predicate is a separate facet, so every
/// return reader (`ReturnType<F>`, call resolution) sees the checker's
/// return without knowing about predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignaturePredicate {
    /// What the predicate talks about.
    pub subject: PredicateSubject,
    /// `asserts …` — an assertion signature rather than a type predicate.
    pub asserts: bool,
    /// The narrowed-to / asserted type; `None` only for the targetless
    /// assertion spellings `asserts x` / `asserts this`.
    pub ty: Option<SemanticNodeId>,
}

impl SignaturePredicate {
    /// The same predicate with its target rewritten by `map`.
    #[must_use]
    pub fn map_type(self, map: impl FnOnce(SemanticNodeId) -> SemanticNodeId) -> Self {
        Self {
            ty: self.ty.map(map),
            ..self
        }
    }

    /// The predicate of a lowered signature whose parameters are `params`:
    /// the authored subject resolved to its positional index, the target
    /// already lowered by the caller. `None` when the subject names no
    /// positional parameter (an erroneous annotation — TypeScript reports it
    /// and keeps only the `boolean` / `void` return).
    #[must_use]
    pub fn resolve(
        predicate: &verter_type_expr::TypePredicate,
        params: &[FunctionParam],
        ty: Option<SemanticNodeId>,
    ) -> Option<Self> {
        let subject = match &predicate.subject {
            verter_type_expr::TypePredicateSubject::This => PredicateSubject::This,
            verter_type_expr::TypePredicateSubject::Parameter(name) => {
                let (_, positional) = split_this_receiver(params);
                let index = positional
                    .iter()
                    .position(|param| param.name.as_deref() == Some(name.as_ref()))?;
                PredicateSubject::Parameter(u32::try_from(index).ok()?)
            }
        };
        Some(Self {
            subject,
            asserts: predicate.asserts,
            ty,
        })
    }

    /// The positional parameter this predicate talks about in `params`, or
    /// `None` for a receiver predicate.
    #[must_use]
    pub fn subject_parameter(self, params: &[FunctionParam]) -> Option<&FunctionParam> {
        match self.subject {
            PredicateSubject::This => None,
            PredicateSubject::Parameter(index) => split_this_receiver(params).1.get(index as usize),
        }
    }
}

/// The subject of a [`SignaturePredicate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateSubject {
    /// The positional parameter at this index — counted over the signature's
    /// parameters with the authored `this` receiver excluded, TypeScript's
    /// `parameterIndex` — so parameter names never enter predicate identity.
    Parameter(u32),
    /// The receiver (`this is T`).
    This,
}
