//! Query-boundary crate for the shared semantic engine.
//!
//! This crate owns the QUERY side of the host/query inversion of control:
//! the query layer defines here what it demands of the session host, and
//! `verter_session` implements that demand against its real machinery. The
//! dependency therefore points host → query, never query → host — the query
//! layer programs against [`QueryHostPort`] and neutral DTOs only.
//!
//! # Firewall contract
//!
//! The crate's dependency closure must never reach the parser/compiler
//! front-end (`verter_parser`, `verter_compiler`, `verter_session`,
//! `verter_type_expr_oxc`, or any `oxc_*` crate beyond the span primitive
//! `verter_span` wraps), under any feature. The firewall therefore proves
//! this crate cannot REACH the existing front-end / resolver machinery — it
//! cannot re-enter the shared engine to run a second query-time resolution.
//! It does NOT, on its own, prevent hand-rolling a brand-new `TypeExpr`
//! walker here (`verter_type_expr` IS a dependency); that the query layer
//! never becomes a second resolver is a Macro-Type-Traversal architectural
//! rule, not a property the dependency graph can enforce. The resolve-graph
//! closure guard in `tests/cases/dependency_closure_guard.rs` enforces the
//! reachability contract against the real `cargo metadata` graph.

#![forbid(unsafe_code)]

use verter_type_expr::locators::{AuthoredBodyLocator, TypeParamVisibility};
use verter_type_expr::{TypeExpr, TypeParam};

/// Why the host could not serve an authored-body lowering demand.
///
/// A closed, neutral error vocabulary mirroring the cache-semantics classes
/// of the session's typed locator-deref failures. The class distinctions are
/// load-bearing for cache admission and must never be collapsed:
///
/// * genuine, cacheable results — [`Self::UnknownSymbol`],
///   [`Self::AuthoredBodyAbsent`];
/// * transient no-warm signals (never promoted to a warm cache entry) —
///   [`Self::UnknownFile`], [`Self::LeaseMiss`];
/// * structural fail-closed non-results (the demand can never fabricate a
///   body) — [`Self::LocatorUnroutable`].
///
/// The class is ONE axis of warm admission, orthogonal to the serve's
/// [`QueryHostAdmission`]; a consumer ANDs the two, so even a genuine
/// cacheable miss is warm-admissible only off a
/// [`QueryHostAdmission::Cacheable`] serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryHostError {
    /// The locator's producing canonical is unknown to the host's live
    /// view — the file cannot be materialized. A TRANSIENT no-warm signal
    /// (the file may appear or load later), never a cacheable miss.
    UnknownFile,
    /// The locator anchor names no inventoried declaration in its producing
    /// file. A GENUINE, cacheable resolution result — the symbol truly does
    /// not exist. Distinct from [`Self::LeaseMiss`].
    UnknownSymbol,
    /// The anchored declaration exists but carries no authored type body at
    /// the addressed position (a value declaration without a type
    /// annotation, a type parameter without the requested bound slot). A
    /// GENUINE, cacheable absence.
    AuthoredBodyAbsent,
    /// The lowering hit a broken retained-parse lease: nothing ran, nothing
    /// was produced. A TRANSIENT no-warm signal — the caller must refuse
    /// warm admission of any derived value so a later demand under a live
    /// lease recovers. Never collapsed into [`Self::UnknownSymbol`].
    LeaseMiss,
    /// The locator does not structurally resolve against the authored
    /// source: a mismatched anchor, a stale or misplaced path step, or a
    /// payload position with no lowering route. Fail-closed — the host
    /// never fabricates a body.
    LocatorUnroutable,
}

/// The three load-bearing cache-semantics classes of [`QueryHostError`].
///
/// Module-private on purpose: the public classification surface is the
/// three named predicate helpers on the error itself. The enum exists so
/// the variant→class mapping is ONE exhaustive match (no wildcard) that a
/// newly added error variant cannot bypass silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheSemanticsClass {
    /// Never promoted to a warm cache entry; a later demand may recover.
    TransientNoWarm,
    /// A genuine resolution result, admissible like any other answer —
    /// subject, like any other answer, to the serve's admission axis.
    CacheableMiss,
    /// A structural non-result: the demand can never fabricate a body.
    StructuralFailClosed,
}

impl QueryHostError {
    /// The single exhaustive variant→class mapping. No wildcard by design:
    /// adding a variant forces an explicit cache-semantics classification
    /// here, mirroring the class docs on the enum.
    fn cache_semantics_class(self) -> CacheSemanticsClass {
        match self {
            Self::UnknownFile | Self::LeaseMiss => CacheSemanticsClass::TransientNoWarm,
            Self::UnknownSymbol | Self::AuthoredBodyAbsent => CacheSemanticsClass::CacheableMiss,
            Self::LocatorUnroutable => CacheSemanticsClass::StructuralFailClosed,
        }
    }

    /// True for the TRANSIENT no-warm class — [`Self::UnknownFile`] /
    /// [`Self::LeaseMiss`]: nothing ran against a live view or a live
    /// lease, so nothing derived from this failure may be admitted warm; a
    /// later demand may recover.
    pub fn is_transient_no_warm(self) -> bool {
        self.cache_semantics_class() == CacheSemanticsClass::TransientNoWarm
    }

    /// True for the GENUINE cacheable-miss class — [`Self::UnknownSymbol`]
    /// / [`Self::AuthoredBodyAbsent`]: the demand resolved to a real
    /// absence in the authored source, a result like any other — and, like
    /// any other, warm-admissible only when the serve that produced it is
    /// [`QueryHostAdmission::Cacheable`].
    pub fn is_cacheable_miss(self) -> bool {
        self.cache_semantics_class() == CacheSemanticsClass::CacheableMiss
    }

    /// True for the STRUCTURAL fail-closed class —
    /// [`Self::LocatorUnroutable`]: the locator does not resolve against
    /// the authored source and the host never fabricates a body.
    pub fn is_structural_fail_closed(self) -> bool {
        self.cache_semantics_class() == CacheSemanticsClass::StructuralFailClosed
    }
}

impl std::fmt::Display for QueryHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UnknownFile => "producing canonical is unknown to the host's live view",
            Self::UnknownSymbol => "locator anchor names no inventoried declaration",
            Self::AuthoredBodyAbsent => "declaration carries no authored body at the position",
            Self::LeaseMiss => "retained-parse lease is broken; lowering did not run",
            Self::LocatorUnroutable => "locator does not resolve against the authored source",
        };
        f.write_str(message)
    }
}

impl std::error::Error for QueryHostError {}

/// The derefed authored SHAPE of a lowered declaration body: the whole body,
/// or the ordered same-name merged contributors preserved as a DISTINCT
/// carrier (never collapsed to an intersection — the merged-decl peer-merge
/// reducer needs the contributor structure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredBodyShape {
    /// A single authored body / sub-position expression.
    Single(TypeExpr),
    /// The ordered same-name merged contributors of a whole merged decl
    /// body, in source order.
    Merged(Vec<TypeExpr>),
}

/// The neutral product of one authored-body lowering demand: the authored
/// body shape plus the owning declaration's generic parameters and their
/// TS lexical visibility from the lowered position (so the caller can bind
/// them as `TypeParam` shells in the authored position's own lexical scope
/// under the correct per-position frame).
///
/// This DTO legitimately owns transient `TypeExpr`: it is the BODY-SOURCE
/// lowering product (the same role as the session's decl-body memo output,
/// which stays lower-crate typed IR by design). It is NOT a stored hot
/// carrier, so the `NoTypeExpr` bar does not apply to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredBodyLowering {
    /// The lowered authored body shape.
    pub shape: AuthoredBodyShape,
    /// The owning declaration's FULL header type-parameter list, in source
    /// order — never pre-truncated. Which of them the shape may reference
    /// is `visibility`'s to say.
    pub type_parameters: Vec<TypeParam>,
    /// TS lexical visibility of `type_parameters` from the lowered
    /// position: a body sees every parameter; a constraint bound sees
    /// every sibling (forward refs included); a default bound sees prior
    /// siblings only, with self / later siblings present-as-shadow but
    /// forbidden as references.
    pub visibility: TypeParamVisibility,
}

/// Cache-admission signal carried on every host serve: whether a value
/// derived from the serve may enter a SHARED cache warm.
///
/// This is the port-level projection of the host's completion fence. A
/// serve built against superseded state (a FENCED flight that published
/// nothing) is still valid for the requesting caller's read, but a derived
/// value admitted warm would pair live fact stamps with a superseded
/// payload — an entry the read-side fact rail cannot reject. That holds
/// for NEGATIVE answers exactly as for lowerings: a genuine miss observed
/// against a fenced surface is a superseded-state observation too. The
/// signal therefore travels BY VALUE with the serve on every outcome arm,
/// and every consumer that writes a shared cache must consult it before
/// warm admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryHostAdmission {
    /// The serve is the store-current surface: values derived from it may
    /// enter shared caches (subject to the consumer's own gates).
    Cacheable,
    /// The serve was FENCED — built against superseded state, published
    /// nothing. Valid for the requesting caller's read ONLY; values
    /// derived from it must never be admitted warm.
    ReturnOnly,
}

impl QueryHostAdmission {
    /// Maps the host serve's publication bit onto the admission signal:
    /// `store_published == false` marks a FENCED / superseded serve that
    /// published nothing → [`Self::ReturnOnly`].
    pub fn from_store_published(store_published: bool) -> Self {
        if store_published {
            Self::Cacheable
        } else {
            Self::ReturnOnly
        }
    }
}

/// One host serve of an authored-body lowering demand: the serve's
/// cache-admission signal plus the demand outcome.
///
/// Admission is metadata about the HOST SERVE — how this particular answer
/// was produced — not intrinsic to the answer itself, hence a wrapper
/// field alongside the outcome, present REGARDLESS of which arm the
/// outcome took: the same authored body (or the same genuine miss) is
/// [`QueryHostAdmission::Cacheable`] when served store-current and
/// [`QueryHostAdmission::ReturnOnly`] when served from a fenced flight. A
/// top-level `Result` cannot carry this — it would drop the admission on
/// the error arm, and a genuine miss observed against a fenced surface
/// would read as warm-admissible. Warm admission is the AND of the two
/// axes: the serve's admission and the outcome's own cache-semantics
/// class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHostServe {
    /// The serve's cache-admission signal — present on success and failure
    /// alike.
    pub admission: QueryHostAdmission,
    /// The demand outcome: the owned typed-IR lowering, or the concrete
    /// neutral error.
    pub outcome: Result<AuthoredBodyLowering, QueryHostError>,
}

/// The host port: what the query layer demands of the session host.
///
/// The query layer OWNS this trait; the session host implements it. The
/// query side never names a session, parser, or compiler type — demands are
/// expressed in neutral locators and answered in neutral typed IR, so the
/// implementor (not the query layer) owns all routing, retention, and
/// lease machinery.
pub trait QueryHostPort {
    /// Lower the authored declaration body identified by `locator`,
    /// answering with a [`QueryHostServe`]: the serve's cache-admission
    /// signal ([`QueryHostAdmission`]) plus the outcome — the owned
    /// typed-IR lowering, or the concrete neutral [`QueryHostError`] (the
    /// cache-semantics vocabulary is pinned by this crate and cannot be
    /// replaced by an implementor). The return type is deliberately NOT a
    /// `Result`: the admission signal must reach the consumer on the error
    /// arm too, so a genuine miss observed against a fenced surface stays
    /// return-only.
    ///
    /// The locator addresses an authored, parse-backed source position
    /// content-free (anchor + producer-emitted path); the host derefs it
    /// through its own retained-parse machinery, never fabricates a body,
    /// and never falls back to a transient re-parse.
    fn lower_authored_body(&self, locator: &AuthoredBodyLocator) -> QueryHostServe;
}

#[cfg(test)]
mod tests {
    use super::{QueryHostAdmission, QueryHostError};

    /// The completion-fence mapping itself: an unpublished (FENCED /
    /// superseded) serve maps to `ReturnOnly`, a store-published serve to
    /// `Cacheable`. Discriminating: inverting the mapping (or hardcoding
    /// either admission) fails one of the two arms.
    #[test]
    fn from_store_published_maps_fence_bit_onto_admission() {
        assert_eq!(
            QueryHostAdmission::from_store_published(false),
            QueryHostAdmission::ReturnOnly,
            "an unpublished (fenced) serve must refuse warm admission"
        );
        assert_eq!(
            QueryHostAdmission::from_store_published(true),
            QueryHostAdmission::Cacheable,
            "a store-published serve is admissible"
        );
    }

    /// Every error variant belongs to EXACTLY ONE cache-semantics class,
    /// and to the RIGHT one. Discriminating: reclassifying any variant, or
    /// letting a helper answer `true` for a foreign class, fails the
    /// per-variant expected triple; the exactly-one assertion additionally
    /// rejects overlapping or orphaned classes.
    #[test]
    fn error_class_helpers_partition_the_vocabulary() {
        // (variant, transient no-warm, cacheable miss, structural fail-closed)
        let expected = [
            (QueryHostError::UnknownFile, true, false, false),
            (QueryHostError::LeaseMiss, true, false, false),
            (QueryHostError::UnknownSymbol, false, true, false),
            (QueryHostError::AuthoredBodyAbsent, false, true, false),
            (QueryHostError::LocatorUnroutable, false, false, true),
        ];
        for (error, transient, miss, fail_closed) in expected {
            assert_eq!(
                error.is_transient_no_warm(),
                transient,
                "{error:?}: transient no-warm class"
            );
            assert_eq!(
                error.is_cacheable_miss(),
                miss,
                "{error:?}: cacheable-miss class"
            );
            assert_eq!(
                error.is_structural_fail_closed(),
                fail_closed,
                "{error:?}: structural fail-closed class"
            );

            let claimed = [
                error.is_transient_no_warm(),
                error.is_cacheable_miss(),
                error.is_structural_fail_closed(),
            ]
            .into_iter()
            .filter(|claims| *claims)
            .count();
            assert_eq!(
                claimed, 1,
                "{error:?} must belong to exactly one cache-semantics class"
            );
        }
    }
}
