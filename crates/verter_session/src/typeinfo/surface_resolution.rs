#![deny(missing_docs)]
//! Typed outcome of every resolution-to-surface producer.
//!
//! A producer that resolves a component surface (a one-level object surface,
//! a callable realization, a branch-merged emit set, a presence-only member
//! join, an event-name enumeration) can no longer report SUCCESS while
//! handing back nothing: it returns [`SurfaceResolution::Resolved`] with the
//! surface, the explicit [`SurfaceResolution::NoSurface`] claim ("the demand
//! resolved, and there is no such surface — the complete negative answer"),
//! or [`SurfaceResolution::Incomplete`] carrying a type-level NON-EMPTY
//! reason set.
//!
//! Every SUCCESS arm carries opaque evidence: `Resolved` and `OpenPresence`
//! hold their surface inside the proof-bearing [`Witnessed`] wrapper, and
//! `NoSurface` holds a bare [`SurfaceProof`]. The proof is minted by exactly
//! ONE private finalizer in this module ([`finalize`]); its field is private,
//! so no other module — and no other crate — can construct a success arm.
//! The crate-internal mint surface is the named claim-stating constructors
//! ([`SurfaceResolution::resolved`], [`SurfaceResolution::open_presence`],
//! [`SurfaceResolution::no_surface`]), which route through that finalizer.
//! `SurfaceResolution::Resolved(TypeInfoSurface::empty())` therefore does not
//! compile anywhere: a raw empty-success cannot be spelled, only claimed
//! through a constructor whose name states the claim.
//!
//! The incomplete claim is equally unforgeable: [`IncompleteSurface`]'s
//! fields are module-private and its reason is the [`NonEmptyReasons`]
//! newtype — a reason set that CANNOT represent emptiness. There is no
//! `Default`, no `unwrap_or_default`, no runtime empty-set normalizer, and no
//! spelling that converts a failed resolution into an empty success. This
//! mirrors the discipline `PublishedCompleteness` enforces at the
//! publication boundary (`crate::meta_resolve::output`), one level earlier:
//! at the producer.
//!
//! The carrier adds no query, no cache, and no additional resolution pass —
//! it transports reasons the producer already observed at the point of the
//! drop.

use crate::semantic_query::{
    PartialReason, PartialReasonSet, QueryError, ResultCompleteness, SemanticNodeData,
};

/// Opaque evidence that a success arm was minted by this module's private
/// finalizer. The field is private: no other module or crate can construct
/// one, so a [`SurfaceResolution`] success arm can never be forged — the
/// named constructors are the only mint.
#[derive(Debug)]
pub struct SurfaceProof(());

/// The ONE private finalizer. Every success-arm constructor routes through
/// this function; nothing else can mint a [`SurfaceProof`].
fn finalize() -> SurfaceProof {
    SurfaceProof(())
}

/// A produced surface together with the [`SurfaceProof`] that witnessed its
/// claim. Fields are private; the wrapper is created only by this module's
/// named constructors. Consumers read through [`std::ops::Deref`] or move
/// the surface out with [`Witnessed::into_inner`] — neither hands out a way
/// to construct one.
#[derive(Debug)]
pub struct Witnessed<T> {
    value: T,
    _proof: SurfaceProof,
}

impl<T> std::ops::Deref for Witnessed<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> Witnessed<T> {
    /// Move the witnessed surface out. Reading is unrestricted — only
    /// MINTING is sealed.
    pub(crate) fn into_inner(self) -> T {
        self.value
    }
}

/// A partial-reason set that is NON-EMPTY BY TYPE: the newtype has no empty
/// spelling. Constructors are either type-level ([`NonEmptyReasons::of`]
/// maps one closed [`PartialReason`] to its guaranteed non-empty bit) or
/// checked ([`NonEmptyReasons::new`] refuses an empty set with `None`).
/// There is no `Default` and no silent empty→`PROPAGATED` rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonEmptyReasons(PartialReasonSet);

impl NonEmptyReasons {
    /// One closed-taxonomy reason. Type-level non-empty: every
    /// [`PartialReason`] variant maps to exactly one non-zero bit (pinned by
    /// `partial_reason_taxonomy_covers_every_bit_exactly_once`).
    pub(crate) fn of(reason: PartialReason) -> Self {
        Self(reason.bit())
    }

    /// Checked bridge from the possibly-empty base set: `None` when `set`
    /// records no reason. The caller decides what an empty classification
    /// means — this constructor never substitutes one.
    pub(crate) fn new(set: PartialReasonSet) -> Option<Self> {
        (!set.is_empty()).then_some(Self(set))
    }

    /// The typed classification of a [`QueryError`]. Total: every error arm
    /// classifies to a non-empty set through the shared
    /// `query_error_partial_reasons` mapping (whose arms each name one
    /// closed-taxonomy bit); the checked bridge's structurally-unreachable
    /// empty case classifies as the explicit
    /// [`PartialReason::SemanticQueryFault`] — a fault that could not be
    /// classified is still a fault, never an empty claim.
    pub(crate) fn from_query_error(error: &QueryError) -> Self {
        Self::new(
            crate::project_semantic_dispatch::symbol_identity::query_error_partial_reasons(error),
        )
        .unwrap_or_else(|| Self::of(PartialReason::SemanticQueryFault))
    }

    /// Union with additional (possibly-empty) reasons — non-empty is
    /// preserved by construction.
    pub(crate) fn with(self, more: PartialReasonSet) -> Self {
        Self(self.0.union(more))
    }

    /// Union of two non-empty sets.
    pub(crate) fn union(self, other: Self) -> Self {
        Self(self.0.union(other.0))
    }

    /// The reasons as the base set type. Never empty.
    pub(crate) fn get(self) -> PartialReasonSet {
        self.0
    }
}

/// The outcome of a resolution-to-surface producer.
///
/// `T` is the produced surface shape (a `TypeInfoSurface`, a realized
/// callable `SemanticNodeId`, a member list, an event-name set, …).
#[must_use]
#[derive(Debug)]
pub enum SurfaceResolution<T> {
    /// The producer resolved the demanded surface with its COMPLETE member
    /// domain. A genuinely empty surface (a component that declares nothing,
    /// a legitimately empty slot set) is `resolved(empty)` — complete,
    /// exact, and warm-capable. Proof-bearing: constructible only through
    /// [`SurfaceResolution::resolved`].
    Resolved(Witnessed<T>),
    /// The producer resolved a positive PRESENCE-ONLY projection of an OPEN
    /// member domain (an open spread program, an open compound carrier, an
    /// unbound generic's constraint surface): every carried member is real,
    /// but omission is not absence evidence. NOT a failure —
    /// complete-as-a-result and warm-capable; a consumer names this arm
    /// instead of ignoring a completeness bit. Proof-bearing: constructible
    /// only through [`SurfaceResolution::open_presence`].
    OpenPresence(Witnessed<T>),
    /// The demand RESOLVED and the answer is: there is no such surface — the
    /// type has no one-level object surface (a primitive / union / function),
    /// the value is genuinely not callable, or a committed surface is
    /// deliberately declined (the open-symbolic gate). A COMPLETE negative
    /// answer, distinct from a failure: the caller publishes its documented
    /// empty/absent form and stays complete and warm-capable. Proof-bearing:
    /// constructible only through [`SurfaceResolution::no_surface`].
    NoSurface(SurfaceProof),
    /// The producer could NOT build the demanded surface, and names why. The
    /// reason set is non-empty BY TYPE ([`NonEmptyReasons`]); the only
    /// discharges are the named methods on [`IncompleteSurface`], each of
    /// which either records the partiality or states in its name that the
    /// consumer substitutes authored source instead of claiming the resolved
    /// surface.
    Incomplete(IncompleteSurface<T>),
}

/// The reason-bearing incomplete claim of a [`SurfaceResolution`].
///
/// Fields are module-private: the claim cannot be forged by struct literal,
/// cannot be destructured around its reason, and cannot be turned into an
/// empty success — the compile-fail contract fixtures pin all three.
#[derive(Debug)]
pub struct IncompleteSurface<T> {
    /// Why the surface could not be built. Non-empty by type.
    reason: NonEmptyReasons,
    /// The usable positive subset the producer DID build (presence-only
    /// members from the resolvable arms), when there is one. Never a
    /// stand-in for the complete surface.
    partial: Option<T>,
}

impl<T> SurfaceResolution<T> {
    /// The COMPLETE-claim mint: the producer resolved the full member
    /// domain. Routes through the private finalizer.
    pub(crate) fn resolved(value: T) -> Self {
        Self::Resolved(Witnessed {
            value,
            _proof: finalize(),
        })
    }

    /// The PRESENCE-ONLY-claim mint: every carried member is real, the
    /// domain is open. Routes through the private finalizer.
    pub(crate) fn open_presence(value: T) -> Self {
        Self::OpenPresence(Witnessed {
            value,
            _proof: finalize(),
        })
    }

    /// The COMPLETE-NEGATIVE-claim mint: there is no such surface. Routes
    /// through the private finalizer.
    pub(crate) fn no_surface() -> Self {
        Self::NoSurface(finalize())
    }

    /// The incomplete claim, from the type-level non-empty reason set the
    /// producer observed at the drop.
    pub(crate) fn incomplete(reason: NonEmptyReasons) -> Self {
        Self::Incomplete(IncompleteSurface {
            reason,
            partial: None,
        })
    }

    /// The incomplete claim carrying the usable positive subset the producer
    /// still built (the resolvable arms of a compound surface).
    pub(crate) fn incomplete_with(reason: NonEmptyReasons, partial: T) -> Self {
        Self::Incomplete(IncompleteSurface {
            reason,
            partial: Some(partial),
        })
    }

    /// Map the produced surface shape, preserving the claim, proof, and
    /// reason.
    pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> SurfaceResolution<U> {
        match self {
            Self::Resolved(witnessed) => SurfaceResolution::Resolved(Witnessed {
                value: f(witnessed.value),
                _proof: witnessed._proof,
            }),
            Self::OpenPresence(witnessed) => SurfaceResolution::OpenPresence(Witnessed {
                value: f(witnessed.value),
                _proof: witnessed._proof,
            }),
            Self::NoSurface(proof) => SurfaceResolution::NoSurface(proof),
            Self::Incomplete(inc) => SurfaceResolution::Incomplete(IncompleteSurface {
                reason: inc.reason,
                partial: inc.partial.map(f),
            }),
        }
    }

    /// Demote a CLOSED claim to the presence-only open arm: `Resolved`
    /// becomes `OpenPresence` (the members are real, the domain is open);
    /// every other arm is unchanged. The carried proof moves with the value.
    pub(crate) fn into_open_presence(self) -> Self {
        match self {
            Self::Resolved(witnessed) => Self::OpenPresence(witnessed),
            other => other,
        }
    }

    /// Fold OBSERVED READ PARTIALITY into the claim: with `None` the claim
    /// is unchanged; with reasons, a success claim DEMOTES to `Incomplete`
    /// carrying its produced surface as the usable subset (`Resolved` /
    /// `OpenPresence` keep their members, `NoSurface` becomes a subset-less
    /// incomplete), and an existing `Incomplete` unions the reasons in. A
    /// producer that computed a partial read can therefore never hand its
    /// result onward under a reason-free complete/warm claim.
    pub(crate) fn with_read_partiality(self, read_partiality: Option<NonEmptyReasons>) -> Self {
        let Some(reasons) = read_partiality else {
            return self;
        };
        match self {
            Self::Resolved(witnessed) | Self::OpenPresence(witnessed) => {
                Self::incomplete_with(reasons, witnessed.into_inner())
            }
            Self::NoSurface(_) => Self::incomplete(reasons),
            Self::Incomplete(inc) => Self::Incomplete(IncompleteSurface {
                reason: inc.reason.union(reasons),
                partial: inc.partial,
            }),
        }
    }

    /// Discharge into an `Option`, RECORDING the incomplete arm's typed
    /// reason into the request-result completeness signal and the active
    /// per-cold-compute scope (`fold_result_completeness`) first. `Resolved`
    /// and `OpenPresence` yield their surface; `NoSurface` and a subset-less
    /// `Incomplete` yield `None`; an `Incomplete` with a usable subset yields
    /// the subset — after the partiality is on record, so the subset can
    /// never pass as the complete surface.
    pub(crate) fn recorded(self) -> Option<T> {
        match self {
            Self::Resolved(witnessed) | Self::OpenPresence(witnessed) => {
                Some(witnessed.into_inner())
            }
            Self::NoSurface(_) => None,
            Self::Incomplete(inc) => inc.into_recorded_partial(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<T> SurfaceResolution<T> {
    /// TEST-ONLY `Option` view with NO completeness recording: `Resolved` /
    /// `OpenPresence` yield the surface, `NoSurface` and `Incomplete` yield
    /// `None`. Production discharges go through the named claim-stating
    /// methods; tests use this to assert on the produced value directly.
    #[must_use]
    pub fn resolved_for_tests(self) -> Option<T> {
        match self {
            Self::Resolved(witnessed) | Self::OpenPresence(witnessed) => {
                Some(witnessed.into_inner())
            }
            Self::NoSurface(_) | Self::Incomplete(_) => None,
        }
    }
}

impl<T> IncompleteSurface<T> {
    /// The typed reason set. Non-empty by type.
    pub(crate) fn reasons(&self) -> PartialReasonSet {
        self.reason.get()
    }

    /// The typed reason set as the non-empty witness type.
    pub(crate) fn non_empty_reasons(&self) -> NonEmptyReasons {
        self.reason
    }

    /// RECORD the typed partiality (request-sticky + the active
    /// per-cold-compute scope), then hand back the usable positive subset if
    /// the producer built one. The one discharge for consumers that publish
    /// resolved surfaces: the partiality is on record before any value flows.
    pub(crate) fn into_recorded_partial(self) -> Option<T> {
        crate::request_context::fold_result_completeness(ResultCompleteness::partial(
            self.reason.get(),
        ));
        self.partial
    }

    /// Discard the resolution for a consumer that substitutes the AUTHORED
    /// source on resolution failure (the TSC declaration projection splices
    /// the authored text verbatim). Such a consumer publishes no resolved
    /// surface, so there is no completeness claim to gate — the method name
    /// states that at the call site. Returns the usable subset if any.
    pub(crate) fn into_authored_fallback(self) -> Option<T> {
        self.partial
    }
}

/// Classify a node at a surface ROOT / terminal-hop position: `Some` when
/// the node is an UNRESOLVED carrier (the resolver-owned proof the DEMANDED
/// surface is unavailable, not an authoritative empty surface), `None` when
/// it is a resolved shape.
///
/// ROOT positions are the drop points where the WHOLE demanded surface
/// vanishes when the carrier is unresolved (a macro payload root, a deep
/// path's terminal hop, a `$props()` / event-map root, an emit conditional
/// branch): there the emptiness is indistinguishable from "nothing was
/// declared", so every unresolved carrier — including an honest authored
/// miss — must name a reason. Contrast [`stable_member_carrier_partiality`],
/// the MEMBER-position classifier where a stable authored miss stays a
/// COMPLETE explicit carrier.
pub(crate) fn unresolved_node_partiality(
    data: Option<&SemanticNodeData>,
) -> Option<NonEmptyReasons> {
    match data {
        None => Some(NonEmptyReasons::of(PartialReason::MissingSemanticNodeData)),
        Some(SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)) => {
            Some(NonEmptyReasons::of(PartialReason::MissingDependency))
        }
        Some(SemanticNodeData::RawFallback { .. }) => {
            Some(NonEmptyReasons::of(PartialReason::SemanticQueryFault))
        }
        Some(SemanticNodeData::Opaque(error)) => Some(NonEmptyReasons::from_query_error(error)),
        Some(_) => None,
    }
}

/// Classify a node at a MEMBER-VALUE / compound-arm position: `Some` when
/// the carrier is an OPERATIONAL failure that must mark the surface partial,
/// `None` when it is a resolved shape OR a STABLE unresolved carrier.
///
/// A stable unresolved authored reference (an undeclared name's `BareRef`
/// mirror, an honest `Miss` / `RaiseMiss`, the walker's well-formed
/// `OpenSurface` / `UnmodeledPosition` markers) stays a COMPLETE explicit
/// carrier at a member position: the published surface retains or
/// fail-closed-drops the member deterministically, recomputation can never
/// improve it, and the result stays warm-capable — the established
/// stable-carrier contract. An IMPORT-BACKED unresolvable is different: the
/// authored reference names an import whose dependency owner did not
/// resolve, so the surface behind it is genuinely unavailable — those (a
/// `BareRef` whose name is an authored import of its authoring file, every
/// `ImportType`), raw fallbacks, and operational faults (budget /
/// cancellation / torn state / cycles) name a partial reason here.
pub(crate) fn stable_member_carrier_partiality(
    ctx: &dyn crate::resolver_core::ResolverContext,
    data: Option<&SemanticNodeData>,
) -> Option<NonEmptyReasons> {
    match data {
        None => Some(NonEmptyReasons::of(PartialReason::MissingSemanticNodeData)),
        Some(bare @ SemanticNodeData::BareRef(_)) => bare_ref_import_partiality(ctx, bare),
        Some(SemanticNodeData::ImportType(_)) => {
            Some(NonEmptyReasons::of(PartialReason::MissingDependency))
        }
        Some(SemanticNodeData::RawFallback { .. }) => {
            Some(NonEmptyReasons::of(PartialReason::SemanticQueryFault))
        }
        Some(SemanticNodeData::Opaque(error)) => stable_query_error_partiality(error),
        Some(_) => None,
    }
}

/// The MEMBER-position classification of a [`crate::semantic_query::QueryError`]:
/// honest stable misses and the walker's well-formed open markers are
/// COMPLETE (`None`); every operational fault names its partial reason.
pub(crate) fn stable_query_error_partiality(
    error: &crate::semantic_query::QueryError,
) -> Option<NonEmptyReasons> {
    match error {
        crate::semantic_query::QueryError::Miss
        | crate::semantic_query::QueryError::RaiseMiss
        | crate::semantic_query::QueryError::OpenSurface
        | crate::semantic_query::QueryError::UnmodeledPosition => None,
        other => Some(NonEmptyReasons::from_query_error(other)),
    }
}

/// Whether an unresolved `BareRef` mirror is IMPORT-BACKED: its name is an
/// authored import of its authoring file, so the reference survived lowering
/// unresolved only because the imported dependency owner did not resolve —
/// `MISSING_DEPENDENCY`. A non-import name (an undeclared local, a genuinely
/// scope-less mirror) is the STABLE authored carrier — `None`. Reads the
/// already-indexed shallow import table; no re-resolution.
pub(crate) fn bare_ref_import_partiality(
    ctx: &dyn crate::resolver_core::ResolverContext,
    data: &SemanticNodeData,
) -> Option<NonEmptyReasons> {
    let (name, scope) = data.bare_ref_head()?;
    let crate::semantic_query::NodeScopeId::File {
        canonical_id,
        owner,
        ..
    } = scope
    else {
        return None;
    };
    let state = ctx.shallow_file_state(canonical_id)?;
    // Imports index PER top-level owner; a framework component's instance
    // owner shares the file's authored import table, so consult the scope's
    // exact owner first and the ordinary-file owner as the shared fallback.
    let import_backed =
        state.import_target_in(*owner, name).is_some() || state.import_target(name).is_some();
    import_backed.then_some(NonEmptyReasons::of(PartialReason::MissingDependency))
}
