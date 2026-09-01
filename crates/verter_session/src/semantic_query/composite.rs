//! Sealed union/intersection composite payload substrate.
//!
//! [`CompositeList<K>`] IS the opaque payload of
//! [`SemanticNodeData::Union`](super::SemanticNodeData::Union) /
//! [`SemanticNodeData::Intersection`](super::SemanticNodeData::Intersection).
//! Its member list is PRIVATE: a producer cannot assemble one by struct
//! literal — every mint names a [`CompositeCarrierCategory`], and the mint
//! match is EXHAUSTIVE, so each future carrier category must explicitly
//! choose canonical construction or a justified raw bypass. Reading is
//! unrestricted ([`Deref`](std::ops::Deref) to [`CompositeMembers`], which
//! derefs on to `[SemanticNodeId]`); only CONSTRUCTION is confined.
//!
//! ## Kind binding — the anti-replay seal
//!
//! The payload is GENERIC over a sealed [`CompositeKind`] marker
//! ([`UnionKind`] / [`IntersectionKind`]), and each enum variant carries its
//! own kind: `Union(CompositeList<UnionKind>)`,
//! `Intersection(CompositeList<IntersectionKind>)`. A payload EXTRACTED
//! from one composite kind therefore cannot be REPLAYED into the other —
//! `Intersection(union_payload)` is a type error, provable by compile-fail
//! (`tests/cases/compile-fail/composite_replay_cross_kind.rs`). Same-kind
//! reconstruction from an extracted payload reproduces the byte-identical
//! node value (the member list is unforgeable and immutable), so no NEW
//! derived composite is mintable from a read: the canonical builder stays
//! the sole derived-composite mint.
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
//!   and equivalent locator-shape shell lowering. The arm list is the
//!   AUTHORED form in authored order under the authored scope; normalizing
//!   it would be a reduction performed inside lowering, which is forbidden
//!   — the shell stays recoverable for display while any DERIVED composite
//!   built FROM it routes canonical.
//! * [`CompositeCarrierCategory::OrderedCarrier`] — every order-sensitive
//!   heritage or overload carrier: the own-body-LAST heritage
//!   reconstruction, same-name method overload groups, possibly-callable
//!   member-value intersections, and their companion filters. Arm order is
//!   overload precedence and rendered-type-text fidelity; a commutative
//!   sort would silently reverse the observed overload set.
//! * [`CompositeCarrierCategory::PreservingRebuild`] — an order- and
//!   scope-preserving member-wise rebuild of an EXISTING composite whose
//!   carrier semantics the rebuild site could not prove safe to re-decide.
//!   The rebuilt list inherits the original carrier's semantics verbatim.
//! * [`CompositeCarrierCategory::QuerySubject`] — a query-argument
//!   representation: the `NormalizeUnion` / `NormalizeIntersection`
//!   SUBJECT (the pre-normalization member list interned verbatim so the
//!   query's subject stays distinct from its canonical result), and the
//!   uniform arity-1 key-domain `Union` shell that carries a `Pick` /
//!   `Omit` key set into an `Instantiate` key — an argument carrier by
//!   caller contract, never a published derived result.
//! * [`CompositeCarrierCategory::TestFixture`] — test-build-only fixture
//!   construction (absent from every ordinary production build profile;
//!   compiled solely under `cfg(any(test, feature = "test-support"))`,
//!   the same gate as the `for_tests` shim).
//!
//! ## The at-rest origin category
//!
//! Every mint stamps its category onto the payload as the at-rest
//! [`CompositeOriginCategory`] fact, readable through
//! [`CompositeMembers::origin_category`]. This is what lets a REBUILD site
//! classify by carrier semantics instead of by transformation shape: a
//! member-wise rebuild of a `Canonical`, `CanonicalUnproven` or
//! `AuthoredShell` composite is a DERIVED result and routes back through
//! the canonical authority, while a rebuild of an `OrderedCarrier` (or of
//! any category whose original semantics the tag cannot prove
//! re-decidable — `PreservingRebuild`, `QuerySubject`, `TestFixture`)
//! preserves the arm list verbatim. The pre-seal flow closure uses the
//! same fact as its O(1) canonicality test — keyed on exactly
//! `Canonical`, which the canonical builder stamps ONLY on a COMPLETE
//! canonicalization (an incomplete run — over-cap arm set, exhausted
//! compare budget, dangling arm, undecided peek — stamps
//! `CanonicalUnproven` instead): a `Canonical`-tagged top is proven
//! closed, so the closure skips the pipeline (and deposits no evidence)
//! instead of re-proving a no-op, while an unproven top pays the full
//! re-close and its evidence re-deposit on every resurfacing —
//! budget-degraded results are never skip-classified canonical form.
//!
//! **Identity discipline.** The category is EXCLUDED from `Eq` / `Hash`:
//! arena identity stays `(members, kind, sidecar scope)` exactly as
//! before, so canonical dedup, memo keys, and every pinned node identity
//! are unchanged. Under the arena's content dedup the retained tag is
//! FIRST-WINS: two mints of the byte-identical list under the same scope
//! share one node, tagged by whichever interned first. A cross-category
//! collision requires the two member lists to be BYTE-IDENTICAL. A list
//! byte-identical to a `Canonical`-STAMPED output IS in canonical form
//! (the stamp is refused on incomplete evidence, and a complete
//! canonicalization is deterministic over immutable node data), so every
//! collision agrees on the VALUE: re-closing a canonical-form list is the
//! idempotence no-op, and preserving a list keeps that same list. A
//! canonical-form node that FIRST interned under a bypass mint (or as
//! `CanonicalUnproven`) simply loses the skip — the closure re-proves the
//! no-op — never a wrong value.
//!
//! One order-bearing first-wins residue is DISCLOSED OPEN, not closed: an
//! ordered mint whose arm list is byte-identical to an earlier
//! `Canonical`-tagged twin is misread as re-decidable at the rebuild
//! sites, and the fail-closed callable guard
//! (`value_may_contribute_call_signatures`) narrows only the
//! overload-group class of that misread — an intersection rebuild whose
//! arms may carry call signatures preserves order regardless of the tag.
//! Order semantics INDEPENDENT of callability are not covered by it:
//! own-body-LAST heritage topology, rendered-type-text fidelity, and the
//! first-encountered entry/keyspace slots of the intersection surface
//! merge (`merge_intersection_surfaces_with_graph`) all consume arm
//! order, so a collided ordered carrier whose rebuilt arms are
//! signature-free CAN be reordered by a commutative re-decide. The live
//! window is narrow — ordered mints share the `Global` identity domain
//! with canonical multi-arm mints, and canonical order (node-id order)
//! typically coincides with construction order, so the colliding twin
//! usually already carries the ordered spelling — but it is NOT empty.
//!
//! ## Confinement — the disclosed language limit
//!
//! Confinement is exactly what Rust visibility plus the kind types give,
//! stated honestly:
//!
//! * OUT-OF-CRATE the mints are UNFORGEABLE — the member field is private,
//!   [`CompositeList::minted`] and every category constructor are
//!   `pub(crate)`, and [`CanonicalMint`]'s constructor is private to the
//!   canonical-algebra module. A foreign crate can read a composite but
//!   can construct neither a derived composite nor a bypass, and an
//!   extracted payload replays only into its own kind as the identical
//!   value; all of it is provable by compile-fail
//!   (`tests/cases/compile-fail/composite_mint_unforgeable.rs`,
//!   `composite_struct_literal_forge.rs`,
//!   `composite_replay_cross_kind.rs`).
//! * IN-CRATE the same barrier does not hold — any function in this crate
//!   can, in principle, reach a `pub(crate)` constructor. The in-crate
//!   forcing function is the EXHAUSTIVE category match (here and at every
//!   consumer that classifies categories) with no wildcard arm: a new
//!   carrier category is a compile error at each match until its meaning
//!   is written down, so a new raw-construction class cannot appear as a
//!   runtime-invisible bypass. In-crate unforgeability is NOT claimed.

use std::marker::PhantomData;
use std::sync::Arc;

use super::SemanticNodeId;

mod sealed {
    /// Seals [`super::CompositeKind`]: no foreign kind marker can make a
    /// third composite payload type.
    pub trait Sealed {}
}

/// Sealed marker of a composite payload's kind. Exactly two implementors
/// exist — [`UnionKind`] and [`IntersectionKind`] — matching the two enum
/// variants that carry a composite payload.
pub trait CompositeKind: sealed::Sealed + 'static {}

/// Kind marker of [`SemanticNodeData::Union`](super::SemanticNodeData::Union)'s
/// payload. Uninhabited: it exists only at the type level.
#[derive(Debug)]
pub enum UnionKind {}
impl sealed::Sealed for UnionKind {}
impl CompositeKind for UnionKind {}

/// Kind marker of
/// [`SemanticNodeData::Intersection`](super::SemanticNodeData::Intersection)'s
/// payload. Uninhabited: it exists only at the type level.
#[derive(Debug)]
pub enum IntersectionKind {}
impl sealed::Sealed for IntersectionKind {}
impl CompositeKind for IntersectionKind {}

/// The kind-erased core of a composite payload: the sealed member list
/// plus the at-rest origin-category fact. This is what a reader that
/// handles `Union` and `Intersection` uniformly binds (via
/// [`SemanticNodeData::composite_members`](super::SemanticNodeData::composite_members));
/// it is NOT constructible outside the mints, and holding one grants no
/// replay — neither enum variant accepts a bare `CompositeMembers`.
#[derive(Debug, Clone)]
pub struct CompositeMembers {
    members: Arc<[SemanticNodeId]>,
    category: CompositeOriginCategory,
}

impl CompositeMembers {
    /// The shared member-list allocation (refcount clone, never a deep
    /// copy).
    #[must_use]
    pub(crate) fn members_arc(&self) -> Arc<[SemanticNodeId]> {
        Arc::clone(&self.members)
    }

    /// The at-rest origin-category fact — which carrier category this
    /// payload was minted under (first-wins under arena content dedup; see
    /// the module docs' identity discipline for why every collision is
    /// value-benign).
    #[must_use]
    pub(crate) fn origin_category(&self) -> CompositeOriginCategory {
        self.category
    }
}

/// Identity is the member list ONLY — the origin category is an at-rest
/// fact, never an identity dimension (see the module docs).
impl PartialEq for CompositeMembers {
    fn eq(&self, other: &Self) -> bool {
        self.members == other.members
    }
}
impl Eq for CompositeMembers {}
impl std::hash::Hash for CompositeMembers {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.members.hash(state);
    }
}

impl std::ops::Deref for CompositeMembers {
    type Target = [SemanticNodeId];

    fn deref(&self) -> &Self::Target {
        &self.members
    }
}

/// The sealed, KIND-BOUND member-list payload of a semantic union /
/// intersection.
///
/// The member list is reachable through [`Deref`](std::ops::Deref) (to
/// [`CompositeMembers`], then to `[SemanticNodeId]`); construction goes
/// through [`Self::minted`] with an explicit [`CompositeCarrierCategory`],
/// and the kind parameter pins which enum variant the payload can inhabit.
#[derive(Debug)]
pub struct CompositeList<K: CompositeKind> {
    core: CompositeMembers,
    _kind: PhantomData<fn() -> K>,
}

impl<K: CompositeKind> Clone for CompositeList<K> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            _kind: PhantomData,
        }
    }
}

impl<K: CompositeKind> PartialEq for CompositeList<K> {
    fn eq(&self, other: &Self) -> bool {
        self.core == other.core
    }
}
impl<K: CompositeKind> Eq for CompositeList<K> {}
impl<K: CompositeKind> std::hash::Hash for CompositeList<K> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.core.hash(state);
    }
}

impl<K: CompositeKind> CompositeList<K> {
    /// Mint a composite member list under an explicit carrier category.
    ///
    /// EXHAUSTIVE over [`CompositeCarrierCategory`]: a new category fails to
    /// compile here until this mint decides what minting under it means.
    #[must_use]
    pub(crate) fn minted(
        members: Arc<[SemanticNodeId]>,
        category: CompositeCarrierCategory,
    ) -> Self {
        let category = match category {
            // The canonical builder is the sole derived-composite mint. The
            // witness carries the builder's completeness verdict: only a
            // COMPLETE canonicalization proves the member list carries the
            // canonical algebra (flattened, absorbed, structurally
            // deduplicated, deterministically ordered), so only it stamps
            // the skip-eligible `Canonical` fact; an incomplete run
            // (budgeted, over-cap, dangling-arm, undecided-peek) stamps
            // `CanonicalUnproven` — same value, no canonical-form claim.
            CompositeCarrierCategory::Canonical(witness) => {
                if witness.certifies_canonical_form() {
                    CompositeOriginCategory::Canonical
                } else {
                    CompositeOriginCategory::CanonicalUnproven
                }
            }
            // Bypass categories carry the list VERBATIM — authored order,
            // overload order, or the rebuilt original's order is exactly
            // what each category exists to preserve.
            CompositeCarrierCategory::AuthoredShell(_witness) => {
                CompositeOriginCategory::AuthoredShell
            }
            CompositeCarrierCategory::OrderedCarrier(_witness) => {
                CompositeOriginCategory::OrderedCarrier
            }
            CompositeCarrierCategory::PreservingRebuild(_witness) => {
                CompositeOriginCategory::PreservingRebuild
            }
            CompositeCarrierCategory::QuerySubject(_witness) => {
                CompositeOriginCategory::QuerySubject
            }
            #[cfg(any(test, feature = "test-support"))]
            CompositeCarrierCategory::TestFixture(_witness) => CompositeOriginCategory::TestFixture,
        };
        Self {
            core: CompositeMembers { members, category },
            _kind: PhantomData,
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
}

impl<K: CompositeKind> std::ops::Deref for CompositeList<K> {
    type Target = CompositeMembers;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// The at-rest projection of a payload's mint category: a plain fact enum
/// (no witnesses) rebuild sites and the pre-seal closure dispatch on.
/// EXHAUSTIVE consumers match it without a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompositeOriginCategory {
    /// Minted by a COMPLETE canonicalization: the list is PROVEN canonical
    /// form. This is the sole tag the pre-seal closure's O(1) skip
    /// accepts.
    Canonical,
    /// Minted by the canonical algebra from an INCOMPLETE canonicalization
    /// (over-cap arm set, exhausted compare budget, dangling arm,
    /// undecided bounded peek): the value is the deterministic budgeted
    /// result, but canonical FORM is unproven — the list may still carry
    /// structural duplicates the skipped tier would have collapsed. The
    /// O(1) skip refuses it, so a resurfacing top pays the full re-close
    /// and its evidence re-deposit (`incomplete` ⇒ ReturnOnly, never
    /// warm). A rebuild of one is a DERIVED result and re-routes
    /// canonical.
    CanonicalUnproven,
    /// Authored-syntax / locator-shape shell: authored order, authored
    /// scope. A member-wise rebuild of one is a DERIVED result.
    AuthoredShell,
    /// Order-sensitive heritage / overload carrier: arm order is overload
    /// precedence — never re-decided.
    OrderedCarrier,
    /// A preserving rebuild: the ORIGINAL's category was not proven
    /// re-decidable, so downstream rebuilds stay preserving too.
    PreservingRebuild,
    /// A query-argument carrier (normalize subject, arity-1 key-domain
    /// shell): shape is caller contract — never re-decided.
    QuerySubject,
    /// Test-build-only fixture construction.
    #[cfg(any(test, feature = "test-support"))]
    TestFixture,
}

/// Exhaustive registry of the carrier categories a union / intersection
/// payload may be minted under. See the module docs for each category's
/// carrier semantics and its bypass justification.
pub(crate) enum CompositeCarrierCategory {
    /// A derived semantic composite constructed by the canonical
    /// union/intersection algebra — the sole derived-composite mint (its
    /// witness constructor is private to that module).
    Canonical(crate::project_semantic_dispatch::canonical_algebra::CanonicalMint),
    /// Authored-syntax / locator-shape shell lowering: authored order,
    /// authored scope, no reduction.
    AuthoredShell(AuthoredShellMint),
    /// An order-sensitive heritage or overload carrier: arm order is
    /// overload precedence and rendered-type-text fidelity.
    OrderedCarrier(OrderedCarrierMint),
    /// An order- and scope-preserving member-wise rebuild of an existing
    /// composite whose carrier semantics were not proven re-decidable: the
    /// rebuilt list inherits the original's semantics verbatim.
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
