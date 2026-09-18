//! Frozen call substitutions, binder-space-qualified, composed through the
//! existing `CanonicalTypeSubstitution` map in the application-defined
//! direction:
//!
//! ```text
//! apply(compose_after(first, second), T) = apply(second, apply(first, T))
//! ```
//!
//! Identity maps are elided. Compose nodes carry chain depth and flatten
//! past [`MAX_SUBSTITUTION_CHAIN_DEPTH`]. Projection onto a term walks only
//! referenced binders.

use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

use super::records::{BinderSpaceId, CallSubstitutionId, TypeToken};

/// Flatten compose chains beyond this depth (history-independent bound).
pub const MAX_SUBSTITUTION_CHAIN_DEPTH: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstError {
    EscapingInferenceVar,
    WrongBinderSpace,
    /// A Compose node cannot be applied except through the store, which
    /// walks `first` then `second`. Direct `apply_term` is a misuse.
    UnresolvedCompose,
}

/// Kernel-local term the substitution laws are stated over.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum SubstTerm {
    Binder(SemanticNodeId),
    Constructed {
        ctor: u32,
        args: Box<[SubstTerm]>,
    },
    /// Temporary solver slot. Must not escape a published map.
    InferenceVar {
        id: u32,
        space: BinderSpaceId,
    },
}

/// Interned substitution node. Domain is the binder space of parameters
/// this map substitutes; codomain is the space of residual/call binders
/// the image lives in. Same-space maps have `domain == codomain`.
///
/// Map payloads are [`CanonicalTypeSubstitution`] (the existing node-id
/// substitution authority). Structured kernel terms ([`SubstTerm::Constructed`])
/// are rewritten by recursive projection onto referenced binders; rewriting
/// an interned graph node that *contains* binders is `substitute_canonical`
/// on the type graph and is not duplicated here.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum CallSubstitution {
    Identity {
        domain: BinderSpaceId,
        codomain: BinderSpaceId,
    },
    Map {
        domain: BinderSpaceId,
        codomain: BinderSpaceId,
        map: CanonicalTypeSubstitution,
    },
    Compose {
        domain: BinderSpaceId,
        codomain: BinderSpaceId,
        first: CallSubstitutionId,
        second: CallSubstitutionId,
        depth: u8,
    },
}

impl CallSubstitution {
    #[must_use]
    pub fn identity(space: BinderSpaceId) -> Self {
        Self::Identity {
            domain: space,
            codomain: space,
        }
    }

    #[must_use]
    pub fn identity_across(domain: BinderSpaceId, codomain: BinderSpaceId) -> Self {
        Self::Identity { domain, codomain }
    }

    #[must_use]
    pub fn map(space: BinderSpaceId, map: CanonicalTypeSubstitution) -> Self {
        Self::map_across(space, space, map)
    }

    #[must_use]
    pub fn map_across(
        domain: BinderSpaceId,
        codomain: BinderSpaceId,
        map: CanonicalTypeSubstitution,
    ) -> Self {
        if map.bindings().is_empty() {
            Self::Identity { domain, codomain }
        } else {
            Self::Map {
                domain,
                codomain,
                map,
            }
        }
    }

    #[must_use]
    pub fn domain(&self) -> BinderSpaceId {
        match *self {
            Self::Identity { domain, .. }
            | Self::Map { domain, .. }
            | Self::Compose { domain, .. } => domain,
        }
    }

    #[must_use]
    pub fn codomain(&self) -> BinderSpaceId {
        match *self {
            Self::Identity { codomain, .. }
            | Self::Map { codomain, .. }
            | Self::Compose { codomain, .. } => codomain,
        }
    }

    /// Domain of this map (binders it substitutes).
    #[must_use]
    pub fn space(&self) -> BinderSpaceId {
        self.domain()
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        matches!(self, Self::Identity { .. })
    }

    #[must_use]
    pub fn is_same_space_identity(&self) -> bool {
        match *self {
            Self::Identity { domain, codomain } => domain == codomain,
            _ => false,
        }
    }
}

/// Apply a canonical map to a binder token. Unmapped binders are identity.
#[must_use]
pub fn apply_canonical(map: &CanonicalTypeSubstitution, binder: SemanticNodeId) -> SemanticNodeId {
    for &(param, bound) in map.bindings() {
        if param == binder {
            return bound;
        }
    }
    binder
}

/// `apply(compose_after(first, second), T) = apply(second, apply(first, T))`.
#[must_use]
pub fn compose_canonical(
    first: &CanonicalTypeSubstitution,
    second: &CanonicalTypeSubstitution,
) -> CanonicalTypeSubstitution {
    if first.bindings().is_empty() {
        return second.clone();
    }
    if second.bindings().is_empty() {
        return first.clone();
    }
    let mut out = Vec::with_capacity(
        first
            .bindings()
            .len()
            .saturating_add(second.bindings().len()),
    );
    // bounded-loop: one pass per first binding, then unmatched second bindings.
    for &(param, bound) in first.bindings() {
        out.push((param, apply_canonical(second, bound)));
    }
    for &(param, bound) in second.bindings() {
        if !first.bindings().iter().any(|(p, _)| *p == param) {
            out.push((param, bound));
        }
    }
    CanonicalTypeSubstitution::new(out)
}

impl CallSubstitution {
    /// Apply this node to a term. Compose is resolved by the store; a Map
    /// or Identity applies directly. Inference variables must not appear in
    /// the output of a published map.
    pub fn apply_term(&self, term: &SubstTerm) -> Result<SubstTerm, SubstError> {
        match self {
            Self::Identity { .. } => reject_escaping(term.cloned_if_var()?),
            Self::Map { map, domain, .. } => apply_map_term(map, *domain, term),
            Self::Compose { .. } => Err(SubstError::UnresolvedCompose),
        }
    }
}

impl SubstTerm {
    fn cloned_if_var(&self) -> Result<SubstTerm, SubstError> {
        match self {
            Self::InferenceVar { .. } => Err(SubstError::EscapingInferenceVar),
            other => Ok(other.clone()),
        }
    }
}

fn reject_escaping(term: SubstTerm) -> Result<SubstTerm, SubstError> {
    match term {
        SubstTerm::InferenceVar { .. } => Err(SubstError::EscapingInferenceVar),
        other => Ok(other),
    }
}

fn apply_map_term(
    map: &CanonicalTypeSubstitution,
    space: BinderSpaceId,
    term: &SubstTerm,
) -> Result<SubstTerm, SubstError> {
    match term {
        SubstTerm::InferenceVar {
            space: var_space, ..
        } if *var_space == space => Err(SubstError::EscapingInferenceVar),
        SubstTerm::InferenceVar { .. } => Err(SubstError::EscapingInferenceVar),
        SubstTerm::Binder(id) => Ok(SubstTerm::Binder(apply_canonical(map, *id))),
        SubstTerm::Constructed { ctor, args } => {
            let mut mapped = Vec::with_capacity(args.len());
            // bounded-loop: one recursive apply per demanded argument.
            for arg in args.iter() {
                mapped.push(apply_map_term(map, space, arg)?);
            }
            Ok(SubstTerm::Constructed {
                ctor: *ctor,
                args: mapped.into_boxed_slice(),
            })
        }
    }
}

/// Type-token apply used when a recipe carries tokens rather than terms.
#[must_use]
pub fn apply_token(map: &CanonicalTypeSubstitution, token: TypeToken) -> TypeToken {
    let as_node = SemanticNodeId(token.as_u64());
    TypeToken::from_raw(apply_canonical(map, as_node).0)
}
