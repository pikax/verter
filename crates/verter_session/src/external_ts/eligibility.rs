//! Composition of a per-project [`ProjectEligibility`] from the five real
//! SHARED-precondition facts, as TYPED, PROVENANCE-BEARING inputs.
//!
//! SHARED (attaching to the editor's already-running engine) requires
//! PROVENANCE-TYPED POSITIVE evidence for every precondition; an incomplete,
//! partial, or absent fact set yields OWNED, never SHARED. This module turns
//! the five facts into the pre-composition verdict [`mode`](super::mode)
//! consumes. The runtime facts are supplied by the live editor-attach
//! integration; this layer is pure logic over the typed inputs.
//!
//! ## Why typed facts, not bare bools
//!
//! Each fact is a DISTINCT two-state type carrying its positive evidence, not a
//! `bool`. Five `bool`s at a call site are trivially swappable (pass the proxy
//! flag where the gate flag belongs and nothing complains); five distinct fact
//! types make that swap a COMPILE error. And the composition's OWNED output can
//! only ever be a [`ProjectEligibility::Owned`] carrying an
//! [`EligibilityFailure`] — the restricted eligibility-INPUT set — so a derived
//! decision reason ([`OwnedReason::IncompleteComponent`] etc.) is unrepresentable
//! as a composition result, mirroring the `EligibilityFailure ⊂ OwnedReason`
//! discipline in [`mode`](super::mode).

use std::sync::Arc;

use crate::file_artifact_store::ProjectIdentity;

use super::mode::{
    editor_binding_matches, EligibilityFailure, ProjectEligibility, SharedSessionFacts,
};
use super::resolver::ProjectResolution;

/// The engine-version capability-gate fact. `Cleared` carries the observed
/// engine version the gate `validate` produced (the positive evidence); the
/// absence is `NotGreen`. Distinct from every other fact type, so it cannot be
/// swapped for another precondition's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionGateFact {
    /// The version gate cleared green; the payload is the in-band observed
    /// engine version that satisfied it.
    Cleared {
        /// The engine version the gate observed and accepted.
        observed_version: Arc<str>,
    },
    /// The version gate has not cleared green for the attach candidate.
    NotGreen,
}

/// The non-owning-attach-liveness fact. `Live` carries the
/// [`SharedSessionFacts`] the live attach produced (the sealed provenance type,
/// so attach-liveness is witnessed by real SHARED-session facts, never a bare
/// flag); the absence is `NotLive`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachFact {
    /// A live non-owning attach exists; the payload is the attached editor
    /// engine's sealed SHARED-session facts.
    Live(SharedSessionFacts),
    /// No live non-owning attach to the editor's engine.
    NotLive,
}

/// The project-binding fact (the `ProjectNotBound` precondition). `Bound`
/// carries the resolved configured project's identity; the absence is
/// `NotBound`. Built from a [`ProjectResolution`] via
/// [`BindingFact::from_resolution`] so "bound" means a real
/// [`ProjectResolution::ProjectBinding`] — never `NoProject` / `Ambiguous` /
/// `SyntheticScratch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingFact {
    /// The carrier source resolved to a configured project; the payload is that
    /// project's canonical identity.
    Bound(ProjectIdentity),
    /// The carrier source has no resolved configured-project binding.
    NotBound,
}

/// The proxy/interposition-availability fact: can Verter interpose the editor's
/// full TS-LSP connection (so carrier-path leak suppression is enforceable)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyFact {
    /// Verter can interpose the editor's connection; leak suppression is
    /// enforceable.
    Available,
    /// Verter cannot interpose the editor's connection; SHARED is unsafe.
    Unavailable,
}

/// The editor-binding-identity fact. `Matched` carries the identity both sides
/// agree on; the disagreement is `Mismatch`. Built from the two identities via
/// [`EditorBindingFact::evaluate`], which routes through the ONE binding-witness
/// primitive [`editor_binding_matches`] — never a second equality path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorBindingFact {
    /// The editor bound the carrier to the same project Verter resolved; the
    /// payload is that agreed identity.
    Matched(ProjectIdentity),
    /// The editor bound the carrier to a different configured project than the
    /// one Verter resolved.
    Mismatch,
}

impl BindingFact {
    /// Derive the binding fact from a [`ProjectResolution`]: a real
    /// [`ProjectResolution::ProjectBinding`] yields `Bound` with the binding's
    /// configured-project identity; every other state (`NoProject`,
    /// `Ambiguous`, `SyntheticScratch`) yields `NotBound`. "tsgo seems to know
    /// this file" is NOT a binding — only a resolved `ProjectBinding` is.
    #[must_use]
    pub fn from_resolution(resolution: &ProjectResolution) -> Self {
        match resolution {
            ProjectResolution::ProjectBinding(binding) => {
                BindingFact::Bound(binding.env_dims().project_identity)
            }
            ProjectResolution::NoProject
            | ProjectResolution::Ambiguous(_)
            | ProjectResolution::SyntheticScratch(_) => BindingFact::NotBound,
        }
    }
}

impl EditorBindingFact {
    /// Evaluate the editor-binding witness through the shared
    /// [`editor_binding_matches`] primitive: equal identities yield `Matched`
    /// (carrying the agreed identity), a mismatch yields `Mismatch`.
    #[must_use]
    pub fn evaluate(expected: &ProjectIdentity, editor_bound: &ProjectIdentity) -> Self {
        if editor_binding_matches(expected, editor_bound) {
            EditorBindingFact::Matched(*expected)
        } else {
            EditorBindingFact::Mismatch
        }
    }
}

/// The five SHARED-precondition facts for one project, gathered as typed inputs.
///
/// [`compose_eligibility`] reduces these to a [`ProjectEligibility`]. There is no
/// bare-bool constructor: each field is its own fact type, so the five cannot be
/// positionally confused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibilityFacts {
    /// The engine-version capability-gate clearance.
    pub version_gate: VersionGateFact,
    /// The non-owning-attach liveness.
    pub attach: AttachFact,
    /// The configured-project binding presence.
    pub binding: BindingFact,
    /// The proxy/interposition availability.
    pub proxy: ProxyFact,
    /// The editor-binding-identity agreement.
    pub editor_binding: EditorBindingFact,
}

/// Compose a per-project [`ProjectEligibility`] from the five typed facts.
///
/// SHARED requires ALL-positive evidence: only when the gate is `Cleared`, the
/// attach is `Live`, the binding is `Bound`, the proxy is `Available`, and the
/// editor binding is `Matched` does this return [`ProjectEligibility::Eligible`].
/// ANY missing/negative fact yields [`ProjectEligibility::Owned`] carrying the
/// matching [`EligibilityFailure`], evaluated in the STRICT precedence the
/// failure enum encodes:
///
/// 1. [`EligibilityFailure::VersionGateNotGreen`]
/// 2. [`EligibilityFailure::AttachNotLive`]
/// 3. [`EligibilityFailure::ProjectNotBound`]
/// 4. [`EligibilityFailure::ProxyUnavailable`]
/// 5. [`EligibilityFailure::EditorBindingMismatch`]
///
/// The OWNED arm can only carry an [`EligibilityFailure`] (the restricted
/// eligibility-INPUT set), so a derived decision reason
/// ([`super::mode::OwnedReason::IncompleteComponent`] etc.) can never be smuggled
/// out of this composition — fail-closed by construction.
#[must_use]
pub fn compose_eligibility(facts: &EligibilityFacts) -> ProjectEligibility {
    // Strict fail-closed precedence, in the order the failure enum encodes: the
    // FIRST missing positive fact decides the OWNED reason. Only an all-positive
    // set falls through to `Eligible`.
    if matches!(facts.version_gate, VersionGateFact::NotGreen) {
        return ProjectEligibility::Owned(EligibilityFailure::VersionGateNotGreen);
    }
    if matches!(facts.attach, AttachFact::NotLive) {
        return ProjectEligibility::Owned(EligibilityFailure::AttachNotLive);
    }
    if matches!(facts.binding, BindingFact::NotBound) {
        return ProjectEligibility::Owned(EligibilityFailure::ProjectNotBound);
    }
    if matches!(facts.proxy, ProxyFact::Unavailable) {
        return ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable);
    }
    if matches!(facts.editor_binding, EditorBindingFact::Mismatch) {
        return ProjectEligibility::Owned(EligibilityFailure::EditorBindingMismatch);
    }
    ProjectEligibility::Eligible
}

#[cfg(test)]
#[path = "eligibility_tests.rs"]
mod tests;
