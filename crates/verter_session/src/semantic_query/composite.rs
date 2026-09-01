//! Sealed union/intersection composite payload substrate.
//!
//! [`CompositeList`] is the future opaque payload of
//! [`SemanticNodeData::Union`](super::SemanticNodeData::Union) /
//! [`SemanticNodeData::Intersection`](super::SemanticNodeData::Intersection).
//! Its member list is PRIVATE: a producer cannot assemble one by struct
//! literal — every mint names a [`CompositeCarrierCategory`], and the mint
//! match is EXHAUSTIVE, so each future carrier category must explicitly
//! choose canonical construction or a justified raw bypass.
//!
//! Today the enum payload is still the bare `Arc<[SemanticNodeId]>`; the
//! canonical union/intersection builder
//! ([`canonical_algebra`](crate::project_semantic_dispatch::canonical_algebra))
//! is the substrate's sole live consumer, minting under
//! [`CompositeCarrierCategory::Canonical`] and interning the unwrapped
//! member list. Flipping the payload to `CompositeList` — which turns every
//! remaining raw constructor into a compile error until it names its
//! category — lands together with the authored / ordered-carrier / rebuild
//! bypass categories and the out-of-crate compile-fail proof.
//!
//! Confinement is exactly what Rust visibility gives: the mints are
//! unforgeable from OUTSIDE the crate (private fields, `pub(crate)`
//! constructors), and [`CanonicalMint`]'s constructor is private to the
//! canonical-algebra module, so only the canonical builder can mint the
//! `Canonical` category. IN-crate, the exhaustive category match — not the
//! type system — is the forcing function for bypass classification.

use std::sync::Arc;

use super::SemanticNodeId;

/// The sealed member-list payload of a semantic union / intersection.
///
/// The member list is reachable only through [`Deref`](std::ops::Deref) /
/// [`Self::members_arc`]; construction goes through [`Self::minted`] with an
/// explicit [`CompositeCarrierCategory`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompositeList {
    members: Arc<[SemanticNodeId]>,
}

impl CompositeList {
    /// Mint a composite member list under an explicit carrier category.
    ///
    /// EXHAUSTIVE over [`CompositeCarrierCategory`]: a new category fails to
    /// compile here until this mint decides what minting under it means.
    #[must_use]
    pub(crate) fn minted(
        members: Arc<[SemanticNodeId]>,
        category: CompositeCarrierCategory,
    ) -> Self {
        match category {
            // The canonical builder is the sole derived-composite mint: the
            // witness proves the member list already carries the canonical
            // algebra (flattened, absorbed, structurally deduplicated,
            // deterministically ordered).
            CompositeCarrierCategory::Canonical(_witness) => Self { members },
        }
    }

    /// The shared member-list allocation (refcount clone, never a deep copy).
    #[must_use]
    pub(crate) fn members_arc(&self) -> Arc<[SemanticNodeId]> {
        Arc::clone(&self.members)
    }
}

impl std::ops::Deref for CompositeList {
    type Target = [SemanticNodeId];

    fn deref(&self) -> &Self::Target {
        &self.members
    }
}

/// Exhaustive registry of the carrier categories a union / intersection
/// payload may be minted under.
///
/// `Canonical` is the only category live today: derived semantic composites,
/// minted exclusively by the canonical algebra (its witness constructor is
/// private to that module). The bypass categories — authored-syntax /
/// locator-shape shells, order-sensitive heritage and overload carriers,
/// order-and-scope-preserving rebuilds of existing composites, and the
/// normalize-query subject representation — land with the payload flip;
/// adding each is an explicit new variant every mint match must classify.
pub(crate) enum CompositeCarrierCategory {
    /// A derived semantic composite constructed by the canonical
    /// union/intersection algebra.
    Canonical(crate::project_semantic_dispatch::canonical_algebra::CanonicalMint),
}
