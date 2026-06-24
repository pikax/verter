//! Publication-authority admitted-token chain for the per-macro projectors.
//!
//! # Why this module exists
//!
//! The component-meta publication boundary publishes member `TypeExpr`s into
//! `ExpandedField` DTOs (consumers read `ExpandedField.r#type` by design). The
//! invariant this module enforces is INPUT AUTHORITY, not DTO readability:
//! outside the terminal publication sink, non-sink production code must NOT be
//! able to choose a raw semantic-graph subject ([`SemanticNodeId`]), forge a
//! surface/member wrapper ([`SurfaceMember`]), pair it with its own
//! cursor/scope, and reverse-materialize a bare member `TypeExpr` — bypassing
//! policy admission (`visibility.is_public()` + the published-field edge
//! record).
//!
//! A member/signature `TypeExpr` may cross the publication boundary only from
//! an unforgeable, policy-admitted publication subject. This module is that
//! subject: a sealed four-token chain whose private fields and private
//! [`Seal`] make a token impossible to construct anywhere except the admission
//! functions below. The terminal sink ([`super::output_sink`]) consumes an
//! [`AdmittedPublishedMember`] and never a forgeable `(&SurfaceMember,
//! ProjectionCursor)` pair.
//!
//! # The chain
//!
//! 1. [`ResolvedMacroPayload`] — a macro's payload node resolved through the
//!    shared dispatch (`ResolveMacroPayload`, Navigate). Minted by
//!    [`resolve_macro_payload`].
//! 2. [`ResolvedPayloadSurface`] — the payload's enumerable surface node
//!    (empty-path `MacroObjectSurface` projection). Minted by
//!    [`resolve_payload_surface`] / [`resolve_payload_surface_with_scope`]. Its
//!    [`PublishedSurfaceKind`] is DERIVED from the payload's `macro_kind`,
//!    never caller-supplied.
//! 3. [`SurfaceMemberCandidate`] — one enumerated surface member, its
//!    [`SurfaceMember`] MOVED out of the surface's member vector (no per-token
//!    allocation). Produced in bulk by [`read_surface_member_candidates`].
//! 4. [`AdmittedPublishedMember`] — a candidate that passed policy admission.
//!    Minted by [`admit_published_member`] ONLY when all of: the member is
//!    public; the descending cursor's surface kind matches the candidate's
//!    kind (derived, not asserted); `descend_published_member` succeeds; and
//!    the published-field edge was recorded. This is the sole input the sink's
//!    [`super::output_sink::surface_member_to_expanded_field`] accepts.
//!
//! # Single-engine / typed-IR / shallow invariants
//!
//! The chain is pure plumbing over the EXISTING resolution: it wraps the
//! `ResolveMacroPayload` / `ProjectPath` dispatches the projectors already
//! ran. It adds no second resolver pass, no `resolve_type` engine, no source
//! slicing, and no eager expansion. Publication demand stays `Navigate` (the
//! descended member cursor is a `Navigate` terminal carrier) — shallow by
//! default.

// The token chain is a SEALED publication-authority API: its private fields +
// private `Seal` are the load-bearing structural primary (forging a token is a
// compile error). Some retained fields / accessors (`owner` / `surface_node` /
// `kind` / `macro_*` / `into_member`) are part of that sealed record — they
// describe the admission a token represents and are read by the
// structural-guard token-construction check and future sink consumers — but
// are not all consumed by the current per-member sink, which reads only
// `member()` + `cursor()`. Mirrors the `#![allow(dead_code)]` substrate
// convention on the sibling `macro_payload_substrate` / `projection_demand`
// modules: the deliberate, fully-private surface stays stable as consumers
// wire in, without a clippy breakage in the interim.
#![allow(dead_code)]

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::meta_resolve::projection_demand::{ProjectionCursor, PublishedSurfaceKind};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{DeclIdentity, SemanticNodeId, SurfaceMember};

use super::macro_payload_substrate::PayloadSurfaceScope;

/// Private mint seal. A token carries a `_seal: Seal` field; because [`Seal`]
/// is private to this module and has no public constructor, the only code that
/// can place a `Seal` into a token — and therefore the only code that can
/// construct any token in the chain — is the admission functions in this
/// module. No `#[derive(Default)]`, no `pub` constructor.
struct Seal;

/// Derive the published-surface kind a macro's surface publishes under.
///
/// AUTHORITY-INTERNAL: the kind is a function of the macro's own
/// [`AnalyzedMacroKind`] (which the [`ResolvedMacroPayload`] token already
/// carries), never a caller-supplied value. This is the load-bearing
/// derived-kind rule — a caller cannot assert a kind that disagrees with the
/// macro it resolved.
fn published_surface_kind_for(macro_kind: AnalyzedMacroKind) -> PublishedSurfaceKind {
    match macro_kind {
        AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
            PublishedSurfaceKind::Props
        }
        AnalyzedMacroKind::DefineEmits => PublishedSurfaceKind::Emits,
        AnalyzedMacroKind::DefineSlots => PublishedSurfaceKind::Slots,
        AnalyzedMacroKind::DefineExpose => PublishedSurfaceKind::Exposed,
        AnalyzedMacroKind::DefineModel => PublishedSurfaceKind::Model,
        AnalyzedMacroKind::DefineOptions => PublishedSurfaceKind::Options,
    }
}

/// A macro's payload node, resolved through the shared dispatch.
///
/// Minted ONLY by [`resolve_macro_payload`] on a real resolved node. All
/// fields private; no `Clone`/`Copy`/`Default`/builder.
pub(crate) struct ResolvedMacroPayload {
    owner: DeclIdentity,
    macro_index: usize,
    macro_kind: AnalyzedMacroKind,
    node: SemanticNodeId,
    _seal: Seal,
}

impl ResolvedMacroPayload {
    /// The owner decl identity of the macro's SFC scope.
    pub(crate) fn owner(&self) -> &DeclIdentity {
        &self.owner
    }

    /// The resolved payload's semantic graph node.
    pub(crate) fn node(&self) -> SemanticNodeId {
        self.node
    }

    /// The macro kind (drives the derived published-surface kind).
    pub(crate) fn macro_kind(&self) -> AnalyzedMacroKind {
        self.macro_kind
    }

    /// The macro's stable index in the snapshot.
    pub(crate) fn macro_index(&self) -> usize {
        self.macro_index
    }
}

/// A macro payload's enumerable surface node.
///
/// Minted ONLY by [`resolve_payload_surface`] /
/// [`resolve_payload_surface_with_scope`]. Its [`PublishedSurfaceKind`] is
/// DERIVED from the payload's `macro_kind` inside this module, never
/// caller-supplied.
pub(crate) struct ResolvedPayloadSurface {
    owner: DeclIdentity,
    macro_index: usize,
    macro_kind: AnalyzedMacroKind,
    kind: PublishedSurfaceKind,
    node: SemanticNodeId,
    _seal: Seal,
}

impl ResolvedPayloadSurface {
    /// The owner decl identity.
    pub(crate) fn owner(&self) -> &DeclIdentity {
        &self.owner
    }

    /// The surface's semantic graph node (the enumerable `Object` shell).
    pub(crate) fn node(&self) -> SemanticNodeId {
        self.node
    }

    /// The derived published-surface kind.
    pub(crate) fn kind(&self) -> &PublishedSurfaceKind {
        &self.kind
    }

    /// The macro's stable index in the snapshot.
    pub(crate) fn macro_index(&self) -> usize {
        self.macro_index
    }

    /// The macro kind.
    pub(crate) fn macro_kind(&self) -> AnalyzedMacroKind {
        self.macro_kind
    }

    /// The owner canonical id (the SFC scope file) — equal to the `file`
    /// argument the projectors thread into the sink as `scope_canonical_id`.
    pub(crate) fn owner_canonical(&self) -> &Arc<str> {
        &self.owner.canonical_id
    }
}

/// One enumerated surface member, its [`SurfaceMember`] MOVED out of the
/// surface's member vector. Carries the surface's derived kind so admission
/// can compare against the cursor's surface kind without a caller assertion.
///
/// Minted ONLY by [`read_surface_member_candidates`]. The owned `member` is the perf
/// point — no `Arc`/`Box`/per-token allocation; the member is moved from the
/// vector the surface enumeration already produced.
pub(crate) struct SurfaceMemberCandidate {
    owner: DeclIdentity,
    surface_node: SemanticNodeId,
    member: SurfaceMember,
    kind: PublishedSurfaceKind,
    _seal: Seal,
}

impl SurfaceMemberCandidate {
    /// The candidate member (published-DTO-side data, read-only).
    pub(crate) fn member(&self) -> &SurfaceMember {
        &self.member
    }

    /// The candidate's derived published-surface kind.
    pub(crate) fn kind(&self) -> &PublishedSurfaceKind {
        &self.kind
    }
}

/// A policy-ADMITTED published member — the sole input the terminal sink's
/// publication API ([`super::output_sink::surface_member_to_expanded_field`])
/// accepts.
///
/// Minted ONLY by [`admit_published_member`], and ONLY when ALL admission
/// conditions hold (public visibility, derived-kind/cursor match,
/// `descend_published_member` success, and a recorded published-field edge).
/// The lifetime `'a` ties to the descending cursor's borrow of the owning
/// [`crate::meta_resolve::projection_demand::SurfaceProjection`].
pub(crate) struct AdmittedPublishedMember<'a> {
    owner: DeclIdentity,
    surface_node: SemanticNodeId,
    member: SurfaceMember,
    cursor: ProjectionCursor<'a>,
    kind: PublishedSurfaceKind,
    _seal: Seal,
}

impl<'a> AdmittedPublishedMember<'a> {
    /// The owner decl identity.
    pub(crate) fn owner(&self) -> &DeclIdentity {
        &self.owner
    }

    /// The parent surface node the member was admitted from.
    pub(crate) fn surface_node(&self) -> SemanticNodeId {
        self.surface_node
    }

    /// The admitted member (read-only published-DTO-side data).
    pub(crate) fn member(&self) -> &SurfaceMember {
        &self.member
    }

    /// The descended member cursor (`Navigate`-terminal carrier for a
    /// shallow-by-default member; an explicit-child cursor when the consumer
    /// walked a deep path). [`ProjectionCursor`] is `Copy`.
    pub(crate) fn cursor(&self) -> ProjectionCursor<'a> {
        self.cursor
    }

    /// The admitted member's published-surface kind.
    pub(crate) fn kind(&self) -> &PublishedSurfaceKind {
        &self.kind
    }

    /// MOVE the admitted member's [`SurfaceMember`] out of the token. Lets the
    /// sink consume the member by value where it needs ownership without a
    /// clone.
    pub(crate) fn into_member(self) -> SurfaceMember {
        self.member
    }

    /// TEST-ONLY direct construction for the cache-rail self-root validation
    /// tests, which drive the production seam
    /// (`output_sink::surface_member_to_expanded_field`) with synthetic
    /// member/cursor values to assert per-member cache collapse — they need an
    /// admitted token without resolving a real macro surface. Named `_for_test`
    /// so it can never masquerade as a production minter; the production path
    /// is [`admit_published_member`]. Gated `#[cfg(test)]` so it has zero
    /// footprint outside test builds.
    #[cfg(test)]
    pub(crate) fn admitted_for_test(
        owner: DeclIdentity,
        surface_node: SemanticNodeId,
        member: SurfaceMember,
        cursor: ProjectionCursor<'a>,
        kind: PublishedSurfaceKind,
    ) -> Self {
        Self {
            owner,
            surface_node,
            member,
            cursor,
            kind,
            _seal: Seal,
        }
    }
}

// =====================================================================
// Admission functions — the ONLY token minters.
// =====================================================================

/// Resolve a type-based macro's payload through the shared dispatch and mint a
/// [`ResolvedMacroPayload`] token on success.
///
/// Wraps [`super::resolve_macro_payload`] (the existing payload-node resolver
/// — `ResolveMacroPayload`, Navigate — plus the silent-miss diagnostic
/// probes). Mints the token ONLY on a real resolved node; on a cycle / error /
/// unresolved-decl it pushes a diagnostic and returns `None`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_macro_payload(
    dispatch: &ProjectSemanticDispatch<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    macro_kind: AnalyzedMacroKind,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<ResolvedMacroPayload> {
    let node = super::resolve_macro_payload(
        dispatch,
        owner,
        file,
        macro_index,
        mac,
        macro_kind,
        expansion_kind,
        diag_sink,
    )?;
    Some(ResolvedMacroPayload {
        owner: owner.clone(),
        macro_index,
        macro_kind,
        node,
        _seal: Seal,
    })
}

/// Resolve a payload's enumerable surface and mint a [`ResolvedPayloadSurface`]
/// token. The published-surface kind is DERIVED from `payload.macro_kind`.
///
/// Wraps [`super::resolve_payload_surface`]; the surface provenance is taken
/// from the single-source-of-truth [`super::macro_payload_surface_provenance`]
/// for the payload's macro kind.
pub(crate) fn resolve_payload_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload: &ResolvedMacroPayload,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<ResolvedPayloadSurface> {
    let node = super::resolve_payload_surface(
        dispatch,
        payload.node,
        payload.macro_index,
        expansion_kind,
        super::macro_payload_surface_provenance(payload.macro_kind),
        diag_sink,
    )?;
    Some(ResolvedPayloadSurface {
        owner: payload.owner.clone(),
        macro_index: payload.macro_index,
        macro_kind: payload.macro_kind,
        kind: published_surface_kind_for(payload.macro_kind),
        node,
        _seal: Seal,
    })
}

/// Scope-gated payload-surface resolver (the emit-class branch-merge path).
///
/// Wraps [`super::resolve_payload_surface_with_scope`] for emit-class macro
/// object payloads (undecided `Conditional` branch merge); other scopes fall
/// through to the single-dispatch surface. The kind is DERIVED from the
/// payload's macro kind exactly as in [`resolve_payload_surface`].
pub(crate) fn resolve_payload_surface_with_scope(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload: &ResolvedMacroPayload,
    expansion_kind: MacroExpansionKind,
    scope: PayloadSurfaceScope,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<ResolvedPayloadSurface> {
    let node = super::resolve_payload_surface_with_scope(
        dispatch,
        payload.node,
        payload.macro_index,
        expansion_kind,
        scope,
        diag_sink,
    )?;
    Some(ResolvedPayloadSurface {
        owner: payload.owner.clone(),
        macro_index: payload.macro_index,
        macro_kind: payload.macro_kind,
        kind: published_surface_kind_for(payload.macro_kind),
        node,
        _seal: Seal,
    })
}

/// Enumerate the surface's members into [`SurfaceMemberCandidate`] tokens,
/// MOVING each [`SurfaceMember`] out of the enumerated vector.
///
/// AUTHORITY-PRIVATE enumeration: wraps the single shared
/// [`super::read_surface_members`] node→members reader (this is NOT a second
/// `read_surface_members` definition — it is the candidate-tokenising wrapper
/// over that one reader). Each candidate carries the surface's derived kind so
/// admission can compare against the cursor's surface kind.
pub(crate) fn read_surface_member_candidates(
    ctx: &dyn ResolverContext,
    surface: &ResolvedPayloadSurface,
) -> Vec<SurfaceMemberCandidate> {
    let members = super::read_surface_members(ctx, surface.node);
    members
        .into_iter()
        .map(|member| SurfaceMemberCandidate {
            owner: surface.owner.clone(),
            surface_node: surface.node,
            member,
            kind: surface.kind.clone(),
            _seal: Seal,
        })
        .collect()
}

/// The admission gate. Mint an [`AdmittedPublishedMember`] ONLY when ALL hold:
///
/// 1. `member.visibility.is_public()` — Vue does not publish `private` /
///    `protected` class members onto a published surface.
/// 2. the descending `cursor`'s surface kind MATCHES the candidate's derived
///    kind (compared against the surface the cursor already carries — derived,
///    not caller-asserted).
/// 3. `cursor.descend_published_member(member.name)` succeeds (the member is
///    in the published surface the consumer demanded).
/// 4. the published-field edge is recorded via
///    [`ProjectSemanticDispatch::record_published_field_edge`] BEFORE the mint.
///
/// Condition (4) is recorded UNIFORMLY for every macro here — this fixes the
/// historical drift where the options projector descended the member but did
/// not record the edge, while props/emits/slots/expose did.
///
/// Returns `None` (no mint, no edge recorded) when (1) or (2) fail, and `None`
/// (no mint) when (3) fails — the member is simply not part of the demanded
/// published surface.
pub(crate) fn admit_published_member<'a>(
    candidate: SurfaceMemberCandidate,
    cursor: &ProjectionCursor<'a>,
    dispatch: &ProjectSemanticDispatch<'_>,
) -> Option<AdmittedPublishedMember<'a>> {
    // (1) Visibility gate.
    if !candidate.member.visibility.is_public() {
        return None;
    }
    // (2) Derived-kind / cursor match: the descending cursor must address the
    // SAME published surface the candidate was enumerated from. The cursor
    // carries `surface: &PublishedSurfaceKind`; compare it against the
    // candidate's derived kind. A mismatch means the caller paired a cursor
    // for a different surface — refuse.
    if cursor.surface != &candidate.kind {
        return None;
    }
    // (3) Descend into the published member. `None` ⇒ the member is out of the
    // demanded surface (a narrowed projection that does not admit this name).
    let member_cursor = cursor.descend_published_member(candidate.member.name.as_ref())?;
    // (4) Record the published-field origin edge BEFORE the mint. This is the
    // semantic-provenance rail (`MemberEdgeProvenance::PublishedField`) the
    // Rule-5 compliance validator reads; recording it here covers every macro
    // uniformly.
    dispatch.record_published_field_edge(
        &candidate.owner,
        candidate.surface_node,
        candidate.member.value,
        &candidate.member.name,
    );
    Some(AdmittedPublishedMember {
        owner: candidate.owner,
        surface_node: candidate.surface_node,
        member: candidate.member,
        cursor: member_cursor,
        kind: candidate.kind,
        _seal: Seal,
    })
}
