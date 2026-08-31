//! Sealed union/intersection composite payload substrate.
//!
//! [`CompositeList`] IS the opaque payload of
//! [`SemanticNodeData::Union`](super::SemanticNodeData::Union) /
//! [`SemanticNodeData::Intersection`](super::SemanticNodeData::Intersection).
//! Its member list is PRIVATE: a producer cannot assemble one by struct
//! literal — every mint names a [`CompositeCarrierCategory`], and the mint
//! match is EXHAUSTIVE, so each future carrier category must explicitly
//! choose canonical construction or a justified raw bypass. Reading is
//! unrestricted ([`Deref`](std::ops::Deref) to `[SemanticNodeId]`); only
//! CONSTRUCTION is confined.
//!
//! ## The carrier-category registry
//!
//! [`CompositeCarrierCategory`] is the exhaustive TYPE/CAPABILITY inventory
//! of every sanctioned raw-construction class — never a name-keyed source
//! scanner. `Canonical` is the sole DERIVED-composite mint; the bypass
//! categories are defined by CARRIER SEMANTICS (what the arm list MEANS),
//! not by the function that spells them:
//!
//! * [`CompositeCarrierCategory::AuthoredShell`] — authored-syntax lowering
//!   and equivalent locator-shape / decided-fact shell lowering. The arm
//!   list is the AUTHORED (or fact-decided) form in authored order under
//!   the authored scope; normalizing it would be a reduction performed
//!   inside lowering, which is forbidden — the shell stays recoverable for
//!   display while any DERIVED composite built FROM it routes canonical.
//! * [`CompositeCarrierCategory::OrderedCarrier`] — every order-sensitive
//!   heritage or overload carrier: the own-body-LAST heritage
//!   reconstruction, same-name method overload groups, possibly-callable
//!   member-value intersections, and their companion filters. Arm order is
//!   overload precedence and rendered-type-text fidelity; a commutative
//!   sort would silently reverse the observed overload set.
//! * [`CompositeCarrierCategory::PreservingRebuild`] — an order- and
//!   scope-preserving member-wise rebuild of an EXISTING composite (arm
//!   projection, realized-arm rewriting, expansion combining). The rebuilt
//!   list inherits the original carrier's semantics, so the rebuild must
//!   not re-decide them — least of all by reordering.
//! * [`CompositeCarrierCategory::QuerySubject`] — a query-argument
//!   representation: the `NormalizeUnion` / `NormalizeIntersection`
//!   SUBJECT (the pre-normalization member list interned verbatim so the
//!   query's subject stays distinct from its canonical result), and the
//!   uniform arity-1 key-domain `Union` shell that carries a `Pick` /
//!   `Omit` key set into an `Instantiate` key — an argument carrier by
//!   caller contract, never a published derived result.
//! * [`CompositeCarrierCategory::TestFixture`] — test-build-only fixture
//!   construction (absent from every ordinary production build profile;
//!   compiled solely under `cfg(any(test, feature = "test-support"))`, the
//!   same gate as the `for_tests` shim).
//!
//! ## Confinement — the disclosed language limit
//!
//! Confinement is exactly what Rust visibility gives, stated honestly:
//!
//! * OUT-OF-CRATE the mints are UNFORGEABLE — the member field is private,
//!   [`CompositeList::minted`] and every category constructor are
//!   `pub(crate)`, and [`CanonicalMint`]'s constructor is private to the
//!   canonical-algebra module. A foreign crate can read a composite but
//!   can construct neither a derived composite nor a bypass; this is
//!   provable by compile-fail
//!   (`tests/cases/compile-fail/composite_mint_unforgeable.rs`).
//! * IN-CRATE the same barrier does not hold — any function in this crate
//!   can, in principle, reach a `pub(crate)` constructor. The in-crate
//!   forcing function is the EXHAUSTIVE category match (here and at every
//!   consumer that classifies categories) with no wildcard arm: a new
//!   carrier category is a compile error at each match until its meaning
//!   is written down, so a new raw-construction class cannot appear as a
//!   runtime-invisible bypass. In-crate unforgeability is NOT claimed.

use std::sync::Arc;

use super::SemanticNodeId;

/// The sealed member-list payload of a semantic union / intersection.
///
/// The member list is reachable through [`Deref`](std::ops::Deref) /
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
            // Bypass categories carry the list VERBATIM — authored order,
            // overload order, or the rebuilt original's order is exactly
            // what each category exists to preserve.
            CompositeCarrierCategory::AuthoredShell(_witness) => Self { members },
            CompositeCarrierCategory::OrderedCarrier(_witness) => Self { members },
            CompositeCarrierCategory::PreservingRebuild(_witness) => Self { members },
            CompositeCarrierCategory::QuerySubject(_witness) => Self { members },
            #[cfg(any(test, feature = "test-support"))]
            CompositeCarrierCategory::TestFixture(_witness) => Self { members },
        }
    }

    /// Mint under [`CompositeCarrierCategory::AuthoredShell`].
    #[must_use]
    pub(crate) fn authored_shell(members: Arc<[SemanticNodeId]>) -> Self {
        Self::minted(
            members,
            CompositeCarrierCategory::AuthoredShell(AuthoredShellMint { _sealed: () }),
        )
    }

    /// Mint under [`CompositeCarrierCategory::OrderedCarrier`].
    #[must_use]
    pub(crate) fn ordered_carrier(members: Arc<[SemanticNodeId]>) -> Self {
        Self::minted(
            members,
            CompositeCarrierCategory::OrderedCarrier(OrderedCarrierMint { _sealed: () }),
        )
    }

    /// Mint under [`CompositeCarrierCategory::PreservingRebuild`].
    #[must_use]
    pub(crate) fn preserving_rebuild(members: Arc<[SemanticNodeId]>) -> Self {
        Self::minted(
            members,
            CompositeCarrierCategory::PreservingRebuild(PreservingRebuildMint { _sealed: () }),
        )
    }

    /// Mint under [`CompositeCarrierCategory::QuerySubject`].
    #[must_use]
    pub(crate) fn query_subject(members: Arc<[SemanticNodeId]>) -> Self {
        Self::minted(
            members,
            CompositeCarrierCategory::QuerySubject(QuerySubjectMint { _sealed: () }),
        )
    }

    /// Mint under [`CompositeCarrierCategory::TestFixture`] — test builds
    /// only; the variant does not exist in an ordinary production build.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub(crate) fn test_fixture(members: Arc<[SemanticNodeId]>) -> Self {
        Self::minted(
            members,
            CompositeCarrierCategory::TestFixture(TestFixtureMint { _sealed: () }),
        )
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
/// payload may be minted under. See the module docs for each category's
/// carrier semantics and its bypass justification.
pub(crate) enum CompositeCarrierCategory {
    /// A derived semantic composite constructed by the canonical
    /// union/intersection algebra — the sole derived-composite mint (its
    /// witness constructor is private to that module).
    Canonical(crate::project_semantic_dispatch::canonical_algebra::CanonicalMint),
    /// Authored-syntax / locator-shape / decided-fact shell lowering:
    /// authored order, authored scope, no reduction.
    AuthoredShell(AuthoredShellMint),
    /// An order-sensitive heritage or overload carrier: arm order is
    /// overload precedence and rendered-type-text fidelity.
    OrderedCarrier(OrderedCarrierMint),
    /// An order- and scope-preserving member-wise rebuild of an existing
    /// composite: the rebuilt list inherits the original's carrier
    /// semantics.
    PreservingRebuild(PreservingRebuildMint),
    /// A query-argument representation: the `NormalizeUnion` /
    /// `NormalizeIntersection` subject, or the uniform arity-1 key-domain
    /// argument shell — the member list, verbatim.
    QuerySubject(QuerySubjectMint),
    /// Test-build-only fixture construction; the variant is compiled out
    /// of every ordinary production build profile.
    #[cfg(any(test, feature = "test-support"))]
    TestFixture(TestFixtureMint),
}

/// Witness of an [`CompositeCarrierCategory::AuthoredShell`] mint.
pub(crate) struct AuthoredShellMint {
    _sealed: (),
}

/// Witness of an [`CompositeCarrierCategory::OrderedCarrier`] mint.
pub(crate) struct OrderedCarrierMint {
    _sealed: (),
}

/// Witness of a [`CompositeCarrierCategory::PreservingRebuild`] mint.
pub(crate) struct PreservingRebuildMint {
    _sealed: (),
}

/// Witness of a [`CompositeCarrierCategory::QuerySubject`] mint.
pub(crate) struct QuerySubjectMint {
    _sealed: (),
}

/// Witness of a [`CompositeCarrierCategory::TestFixture`] mint — test
/// builds only.
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct TestFixtureMint {
    _sealed: (),
}
