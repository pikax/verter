#![deny(missing_docs)]
//! Typed outcome of every resolution-to-surface producer.
//!
//! A producer that resolves a component surface (a one-level object surface,
//! a callable realization, a branch-merged emit set, a presence-only member
//! join) can no longer report SUCCESS while handing back nothing: it returns
//! [`SurfaceResolution::Resolved`] with the surface, the explicit
//! [`SurfaceResolution::NoSurface`] claim ("the demand resolved, and there is
//! no such surface — the complete negative answer"), or
//! [`SurfaceResolution::Incomplete`] carrying a NON-EMPTY typed reason set.
//!
//! The raw empty-success pair is unconstructible: [`IncompleteSurface`]'s
//! fields are module-private, so the incomplete claim can only be minted
//! through the reason-taking constructors and can only be discharged through
//! the NAMED methods below — there is no `Default`, no `unwrap_or_default`,
//! and no spelling that converts a failed resolution into an empty success.
//! This mirrors the discipline `PublishedCompleteness` enforces at the
//! publication boundary (`crate::meta_resolve::output`), one level earlier:
//! at the producer.
//!
//! The carrier adds no query, no cache, and no additional resolution pass —
//! it transports a [`PartialReasonSet`] the producer already observed at the
//! point of the drop.

use crate::semantic_query::{PartialReasonSet, ResultCompleteness, SemanticNodeData};

/// The outcome of a resolution-to-surface producer.
///
/// `T` is the produced surface shape (a `TypeInfoSurface`, a realized
/// callable `SemanticNodeId`, a member list, …).
#[must_use]
#[derive(Debug)]
pub enum SurfaceResolution<T> {
    /// The producer resolved the demanded surface with its COMPLETE member
    /// domain. A genuinely empty surface (a component that declares nothing,
    /// a legitimately empty slot set) is `Resolved(empty)` — complete, exact,
    /// and warm-capable.
    Resolved(T),
    /// The producer resolved a positive PRESENCE-ONLY projection of an OPEN
    /// member domain (an open spread program, an open compound carrier):
    /// every carried member is real, but omission is not absence evidence.
    /// NOT a failure — complete-as-a-result and warm-capable; a consumer
    /// names this arm instead of ignoring a completeness bit.
    OpenPresence(T),
    /// The demand RESOLVED and the answer is: there is no such surface — the
    /// type has no one-level object surface (a primitive / union / function),
    /// the value is genuinely not callable, or a committed surface is
    /// deliberately declined (the open-symbolic gate). A COMPLETE negative
    /// answer, distinct from a failure: the caller publishes its documented
    /// empty/absent form and stays complete and warm-capable.
    NoSurface,
    /// The producer could NOT build the demanded surface, and names why. The
    /// reason set is non-empty by construction; the only discharges are the
    /// named methods on [`IncompleteSurface`], each of which either records
    /// the partiality or states in its name that the consumer substitutes
    /// authored source instead of claiming the resolved surface.
    Incomplete(IncompleteSurface<T>),
}

/// The reason-bearing incomplete claim of a [`SurfaceResolution`].
///
/// Fields are module-private: the claim cannot be forged by struct literal,
/// cannot be destructured around its reason, and cannot be turned into an
/// empty success — the compile-fail contract fixture pins all three.
#[derive(Debug)]
pub struct IncompleteSurface<T> {
    /// Why the surface could not be built. Never empty.
    reason: PartialReasonSet,
    /// The usable positive subset the producer DID build (presence-only
    /// members from the resolvable arms), when there is one. Never a stand-in
    /// for the complete surface.
    partial: Option<T>,
}

impl<T> SurfaceResolution<T> {
    /// The incomplete claim, from the typed reason set the producer observed
    /// at the drop. An empty set records [`PartialReasonSet::PROPAGATED`] so
    /// the claim always carries a reason.
    pub(crate) fn incomplete(reason: PartialReasonSet) -> Self {
        Self::Incomplete(IncompleteSurface {
            reason: non_empty(reason),
            partial: None,
        })
    }

    /// The incomplete claim carrying the usable positive subset the producer
    /// still built (the resolvable arms of a compound surface).
    pub(crate) fn incomplete_with(reason: PartialReasonSet, partial: T) -> Self {
        Self::Incomplete(IncompleteSurface {
            reason: non_empty(reason),
            partial: Some(partial),
        })
    }

    /// Map the produced surface shape, preserving the claim and reason.
    pub(crate) fn map<U>(self, f: impl FnOnce(T) -> U) -> SurfaceResolution<U> {
        match self {
            Self::Resolved(value) => SurfaceResolution::Resolved(f(value)),
            Self::OpenPresence(value) => SurfaceResolution::OpenPresence(f(value)),
            Self::NoSurface => SurfaceResolution::NoSurface,
            Self::Incomplete(inc) => SurfaceResolution::Incomplete(IncompleteSurface {
                reason: inc.reason,
                partial: inc.partial.map(f),
            }),
        }
    }

    /// Demote a CLOSED claim to the presence-only open arm: `Resolved`
    /// becomes `OpenPresence` (the members are real, the domain is open);
    /// every other arm is unchanged.
    pub(crate) fn into_open_presence(self) -> Self {
        match self {
            Self::Resolved(value) => Self::OpenPresence(value),
            other => other,
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
            Self::Resolved(value) | Self::OpenPresence(value) => Some(value),
            Self::NoSurface => None,
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
            Self::Resolved(value) | Self::OpenPresence(value) => Some(value),
            Self::NoSurface | Self::Incomplete(_) => None,
        }
    }
}

impl<T> IncompleteSurface<T> {
    /// The typed reason set. Never empty.
    pub(crate) fn reasons(&self) -> PartialReasonSet {
        self.reason
    }

    /// RECORD the typed partiality (request-sticky + the active
    /// per-cold-compute scope), then hand back the usable positive subset if
    /// the producer built one. The one discharge for consumers that publish
    /// resolved surfaces: the partiality is on record before any value flows.
    pub(crate) fn into_recorded_partial(self) -> Option<T> {
        crate::request_context::fold_result_completeness(ResultCompleteness::partial(self.reason));
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

/// Normalize a producer-observed reason set: the incomplete claim never
/// carries an empty set.
fn non_empty(reason: PartialReasonSet) -> PartialReasonSet {
    if reason.is_empty() {
        PartialReasonSet::PROPAGATED
    } else {
        reason
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
) -> Option<PartialReasonSet> {
    match data {
        None => Some(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA),
        Some(SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)) => {
            Some(PartialReasonSet::MISSING_DEPENDENCY)
        }
        Some(SemanticNodeData::RawFallback { .. }) => Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
        Some(SemanticNodeData::Opaque(error)) => Some(
            crate::project_semantic_dispatch::symbol_identity::query_error_partial_reasons(error),
        ),
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
) -> Option<PartialReasonSet> {
    match data {
        None => Some(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA),
        Some(bare @ SemanticNodeData::BareRef(_)) => bare_ref_import_partiality(ctx, bare),
        Some(SemanticNodeData::ImportType(_)) => Some(PartialReasonSet::MISSING_DEPENDENCY),
        Some(SemanticNodeData::RawFallback { .. }) => Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
        Some(SemanticNodeData::Opaque(error)) => stable_query_error_partiality(error),
        Some(_) => None,
    }
}

/// The MEMBER-position classification of a [`crate::semantic_query::QueryError`]:
/// honest stable misses and the walker's well-formed open markers are
/// COMPLETE (`None`); every operational fault names its partial reason.
pub(crate) fn stable_query_error_partiality(
    error: &crate::semantic_query::QueryError,
) -> Option<PartialReasonSet> {
    match error {
        crate::semantic_query::QueryError::Miss
        | crate::semantic_query::QueryError::RaiseMiss
        | crate::semantic_query::QueryError::OpenSurface
        | crate::semantic_query::QueryError::UnmodeledPosition => None,
        other => Some(
            crate::project_semantic_dispatch::symbol_identity::query_error_partial_reasons(other),
        ),
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
) -> Option<PartialReasonSet> {
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
    import_backed.then_some(PartialReasonSet::MISSING_DEPENDENCY)
}
