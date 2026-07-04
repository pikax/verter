//! Engine-mode selection for external-TS carrier requests: OWNED vs SHARED,
//! decided per redirect-ON project-reference connected component.
//!
//! The OWNED `--api` instance is the universal default and correctness
//! baseline; SHARED — attaching to the editor's already-running engine — is
//! an optional optimization chosen only when every precondition holds for
//! every project in the decision unit
//! (`docs/arch/external-ts-engine-architecture.md` §2.10).
//!
//! ## The decision unit: the redirect-ON reference component
//!
//! With source-of-project-reference redirect ON, project A importing
//! project B's carrier resolves to B's source carrier inside A's Program —
//! A's and B's carriers are entangled in one Program slice. Serving A
//! SHARED while B is OWNED would split that edge across two engines: a
//! split-brain answer whose cross-file queries (references, rename,
//! auto-import) are silently incomplete. The selection unit is therefore
//! the **undirected connected component over redirect-ON reference edges**:
//!
//! - [`RedirectReferenceGraph::connected_component`] is entry-independent:
//!   an edge connects both endpoints regardless of which side declared the
//!   reference, so rooting at ANY member yields the same component.
//!   Directed reachability would be entry-dependent — the split-brain hole.
//! - [`select_component_mode`] takes `(graph, root)` and computes the
//!   component INTERNALLY from that same graph, then returns ONE decision
//!   covering the FULL member set: SHARED only when EVERY member is
//!   [`ProjectEligibility::Eligible`] AND no reference anywhere in the
//!   snapshot is unresolved; any owned or unresolved member — or any
//!   unresolved reference elsewhere in the snapshot (see Fail-closed) — turns
//!   the WHOLE component OWNED. Because the component is computed from the
//!   graph the eligibilities live on, a mismatched/stale member set is not
//!   passable — the whole-component guarantee holds by construction.
//! - [`failover_component_to_owned`] binds to the PRIOR decision and reuses
//!   its exact member set, moving the WHOLE component to OWNED on a
//!   mid-flight failure (a closed redirect, a member dropping out) — never
//!   a member subset.
//! - A reference under `disableSourceOfProjectReferenceRedirect: true`
//!   decouples the Programs (the boundary is the emitted-declaration-shaped
//!   API carrier), so it is NOT an edge of this graph: callers feed
//!   redirect-ON references only into [`RedirectReferenceGraph`].
//!
//! There is deliberately NO per-file or per-single-project mode-selection
//! API — a narrower unit is the split-brain escape hatch. Guard:
//! `shared_mode_failover_is_per_reference_closure`.
//!
//! ## Fail-closed
//!
//! [`select_component_mode`] applies four fail-closed levels in STRICT
//! precedence, so a snapshot that cannot be proven SHARED-safe never serves
//! SHARED:
//!
//! 1. **Member-local incompleteness** → [`OwnedReason::IncompleteComponent`].
//!    A member of THIS component is absent from the graph (still a component
//!    member, its eligibility unknown), or declares a
//!    [`RedirectRef::Unresolved`] redirect-ON reference the live layer could
//!    not resolve to a canonical identity. Runs FIRST, so the declaring
//!    component of an unresolved reference always reports THIS reason.
//! 2. **Snapshot-wide unresolved poison** →
//!    [`OwnedReason::UnresolvedRedirectInSnapshot`]. Any node ANYWHERE in the
//!    snapshot — including one OUTSIDE the queried component — declares a
//!    [`RedirectRef::Unresolved`] reference
//!    ([`RedirectReferenceGraph::any_unresolved_redirect_refs`]). Its
//!    identity-less target could be the SHARED endpoint of a real
//!    cross-project edge with the queried component, so no component in the
//!    snapshot may be served SHARED while any reference is unresolved.
//! 3. **Per-member eligibility** → the first ineligible member's mapped
//!    [`EligibilityFailure`] (in canonical member order).
//! 4. **No SHARED session** → [`OwnedReason::SharedSessionUnavailable`] when
//!    every member is eligible and the snapshot is clean but no live SHARED
//!    session was supplied. Otherwise the component is served SHARED.
//!
//! SHARED is never assumed for an absent member, a dropped (unresolved)
//! dependency, or a snapshot carrying any unresolved reference.
//!
//! A RESOLVED redirect-ON reference is a real graph edge, so its two
//! endpoints already share one component and are decided as ONE unit — never
//! split. An UNRESOLVED reference carries no identity and forms no edge, so
//! the decision layer cannot yet see whether it re-enters the snapshot as the
//! OTHER endpoint of a cross-project edge; poisoning SHARED snapshot-wide is
//! the conservative safety move until it can. Reconstructing that target-side
//! edge — resolving the reference URI to the target's canonical
//! [`ProjectIdentity`] so the two endpoints join one component and the poison
//! lifts — is owned by the live editor-attach integration.
//!
//! ## Scope: the headless decision substrate
//!
//! This module is the pure decision layer. SHARED preconditions enter as
//! opaque per-project facts — [`ProjectEligibility`], computed by the
//! caller from version-gate clearance, attach liveness, project binding,
//! proxy availability, and the editor-binding identity witness
//! ([`editor_binding_matches`]) — and nodes are keyed by the canonical
//! [`ProjectIdentity`], never raw tsconfig paths or URIs. This module does
//! not link the tsgo API layer. The live editor-attach leg — computing real
//! [`ProjectEligibility`] from gate/attach/binding state, resolving each
//! `ProjectBinding` reference URI to its referenced project's canonical
//! [`ProjectIdentity`] before graph construction, the warm-state cache
//! keyed on [`EngineIdentity`], and mode renegotiation on editor
//! reconnect — is owned by the live editor-attach integration. That
//! integration also owns the provider-selection correction for TypeScript 7
//! release-candidate installations currently classified as tsserver in the
//! LSP's `select_type_provider`.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::file_artifact_store::ProjectIdentity;

/// Which engine serves an external-TS request: the OWNED spawned `--api`
/// baseline (the universal default) or the SHARED editor-attach engine
/// (opt-in, preconditions per [`ProjectEligibility`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServeMode {
    /// Verter's own spawned engine instance — always available, the
    /// correctness baseline every component can fail over to.
    Owned,
    /// The editor's already-running engine, attached non-owningly —
    /// selected only when every component member is eligible.
    Shared,
}

/// The restricted set of per-project SHARED-precondition failures — the ONLY
/// values a caller may feed as an eligibility INPUT
/// ([`ProjectEligibility::Owned`]).
///
/// This is a strict subset of [`OwnedReason`]: it excludes the DERIVED /
/// failover reasons ([`OwnedReason::ComponentMemberOwned`],
/// [`OwnedReason::IncompleteComponent`],
/// [`OwnedReason::UnresolvedRedirectInSnapshot`],
/// [`OwnedReason::RedirectClosed`], [`OwnedReason::SharedSessionUnavailable`])
/// that only the decision layer produces — never a per-project input. A
/// nonsensical input such as
/// "this project is ineligible because a sibling is owned" is therefore
/// unrepresentable. The decision maps each failure through
/// [`OwnedReason::from`] when composing a member's OWNED reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EligibilityFailure {
    /// The engine-version capability gate has not cleared green for the
    /// attach candidate.
    VersionGateNotGreen,
    /// No live non-owning attach to the editor's engine.
    AttachNotLive,
    /// The carrier source has no resolved configured-project binding.
    ProjectNotBound,
    /// Verter cannot interpose the editor's full TS-LSP connection, so
    /// carrier-path leak suppression is unenforceable.
    ProxyUnavailable,
    /// The editor bound the carrier to a different configured project than
    /// the one Verter resolved ([`editor_binding_matches`] returned false).
    EditorBindingMismatch,
}

/// Why a component is served OWNED (explainable, per decision) — the
/// DECISION-output superset. Every [`EligibilityFailure`] maps into it (via
/// [`OwnedReason::from`]), plus the derived/failover reasons the decision
/// layer alone produces ([`Self::ComponentMemberOwned`],
/// [`Self::IncompleteComponent`], [`Self::UnresolvedRedirectInSnapshot`],
/// [`Self::RedirectClosed`], [`Self::SharedSessionUnavailable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnedReason {
    /// The engine-version capability gate has not cleared green for the
    /// attach candidate.
    VersionGateNotGreen,
    /// No live non-owning attach to the editor's engine.
    AttachNotLive,
    /// The carrier source has no resolved configured-project binding.
    ProjectNotBound,
    /// Verter cannot interpose the editor's full TS-LSP connection, so
    /// carrier-path leak suppression is unenforceable.
    ProxyUnavailable,
    /// The editor bound the carrier to a different configured project than
    /// the one Verter resolved ([`editor_binding_matches`] returned false).
    EditorBindingMismatch,
    /// A SHARED redirect closed or failed mid-flight.
    RedirectClosed,
    /// A sibling member of the same redirect-ON component is (or became)
    /// OWNED, so this member is served OWNED with it — the whole component
    /// moves as a unit.
    ComponentMemberOwned,
    /// A component member's eligibility is unknown — the member is absent
    /// from the graph, or a redirect-ON reference it declares could not be
    /// resolved to a canonical identity. Fail-closed.
    IncompleteComponent,
    /// A redirect-ON reference SOMEWHERE in the decision snapshot is unresolved, so
    /// the live layer cannot prove any component is independent of the unresolved
    /// (identity-less) target — the WHOLE snapshot fails closed to OWNED rather than
    /// risk serving one endpoint of a real cross-project edge SHARED while the other
    /// is OWNED. Distinct from `IncompleteComponent` (which is a member of THIS
    /// queried component being absent or declaring the unresolved ref).
    UnresolvedRedirectInSnapshot,
    /// Every member is eligible, but no SHARED engine session was supplied
    /// ([`EngineSessionCandidates::shared`] is `None`), so the component
    /// cannot be served SHARED. Fail-closed to the OWNED baseline.
    SharedSessionUnavailable,
}

impl From<EligibilityFailure> for OwnedReason {
    /// Map a per-project eligibility INPUT failure to its DECISION-output
    /// reason. Total over the restricted input set.
    fn from(failure: EligibilityFailure) -> Self {
        match failure {
            EligibilityFailure::VersionGateNotGreen => OwnedReason::VersionGateNotGreen,
            EligibilityFailure::AttachNotLive => OwnedReason::AttachNotLive,
            EligibilityFailure::ProjectNotBound => OwnedReason::ProjectNotBound,
            EligibilityFailure::ProxyUnavailable => OwnedReason::ProxyUnavailable,
            EligibilityFailure::EditorBindingMismatch => OwnedReason::EditorBindingMismatch,
        }
    }
}

/// The strict subset of [`OwnedReason`]s a MID-FLIGHT failover can carry: a
/// closed/failed redirect, or a sibling member of the same component going
/// OWNED. Every OTHER reason is decided at SELECTION time, never mid-flight —
/// an eligibility-input reason, [`OwnedReason::UnresolvedRedirectInSnapshot`],
/// [`OwnedReason::IncompleteComponent`], and
/// [`OwnedReason::SharedSessionUnavailable`] are therefore NOT valid failover
/// causes and are unrepresentable at [`failover_component_to_owned`]. Maps
/// into [`OwnedReason`] via [`OwnedReason::from`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailoverCause {
    /// A SHARED redirect closed or failed mid-flight.
    RedirectClosed,
    /// A sibling member of the same redirect-ON component went OWNED, so the
    /// whole component moves with it.
    ComponentMemberOwned,
}

impl From<FailoverCause> for OwnedReason {
    /// Map a mid-flight failover cause to its decision-output OWNED reason.
    /// Total over the 2-variant failover-valid subset.
    fn from(cause: FailoverCause) -> Self {
        match cause {
            FailoverCause::RedirectClosed => OwnedReason::RedirectClosed,
            FailoverCause::ComponentMemberOwned => OwnedReason::ComponentMemberOwned,
        }
    }
}

/// Per-project SHARED eligibility, BEFORE component composition.
///
/// Computed by the caller from opaque facts (version gate ∧ attach-live ∧
/// project-bound ∧ proxy-available ∧ editor-binding match); this substrate
/// only composes the per-project verdicts over the component. An ineligible
/// verdict carries an [`EligibilityFailure`] — the restricted INPUT set, so a
/// derived/failover reason can never be smuggled in as an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectEligibility {
    /// Every SHARED precondition holds for this project.
    Eligible,
    /// At least one precondition fails; the project (and therefore its
    /// whole component) is served OWNED for this reason.
    Owned(EligibilityFailure),
}

/// One redirect-ON project reference a project declares, as seen by the
/// mode decision: either RESOLVED to the referenced project's canonical
/// identity, or UNRESOLVED — the live layer could not resolve the reference
/// URI to a canonical [`ProjectIdentity`].
///
/// An unresolved reference has no identity, so it joins NO component edge. It
/// fails SHARED closed at TWO precedence levels in [`select_component_mode`]:
/// its declaring project's WHOLE component fails to OWNED with
/// [`OwnedReason::IncompleteComponent`] (member-local), AND — because its
/// identity-less target could be the SHARED endpoint of a real cross-project
/// edge with some OTHER component — every OTHER component in the same snapshot
/// fails to OWNED with [`OwnedReason::UnresolvedRedirectInSnapshot`]
/// (snapshot-wide). Silently dropping it — treating "did not resolve" as "no
/// dependency" — is the fail-OPEN bug: it would let an
/// [`ProjectEligibility::Eligible`] project with a dropped cross-project
/// dependency (its own, or a peer's) go SHARED.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedirectRef {
    /// A redirect-ON reference resolved to the referenced project's
    /// canonical identity — a component edge in both directions.
    Resolved(ProjectIdentity),
    /// A redirect-ON reference the live layer could NOT resolve to a
    /// canonical identity — it forms no edge, but fails the declaring
    /// project's component closed (member-local) AND poisons SHARED
    /// snapshot-wide for every other component.
    Unresolved,
}

/// A project node: its redirect-ON references (resolved edges and any
/// unresolved references) and its eligibility.
#[derive(Debug, Clone)]
struct ProjectNode {
    redirect_on_refs: Vec<RedirectRef>,
    eligibility: ProjectEligibility,
}

/// The redirect-ON project-reference graph the mode decision runs over.
///
/// Nodes are keyed by the canonical [`ProjectIdentity`] — never raw
/// tsconfig paths/URIs, whose case/symlink/normalization drift would mint
/// duplicate nodes and break the component invariant. Edges are the
/// REDIRECT-ON reference edges only: the caller resolves each reference to
/// its canonical identity and excludes references under
/// `disableSourceOfProjectReferenceRedirect: true` before insertion.
///
/// A [`ProjectIdentity`] that appears as a [`RedirectRef::Resolved`]
/// reference but was never inserted is an ABSENT member: it still joins the
/// component (through the undirected edge) and fails the component's mode
/// closed to OWNED. A [`RedirectRef::Unresolved`] reference forms no edge
/// but likewise fails its declaring project's component closed AND poisons
/// SHARED snapshot-wide for every other component
/// ([`Self::any_unresolved_redirect_refs`]).
#[derive(Debug, Clone, Default)]
pub struct RedirectReferenceGraph {
    nodes: FxHashMap<ProjectIdentity, ProjectNode>,
}

impl RedirectReferenceGraph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) a project node with its redirect-ON references
    /// and its pre-composed eligibility.
    ///
    /// `redirect_on_refs` must contain ONLY redirect-ON references. Each is
    /// either [`RedirectRef::Resolved`] (already resolved to the referenced
    /// project's canonical [`ProjectIdentity`] — a component edge) or
    /// [`RedirectRef::Unresolved`] (the live layer could not resolve the
    /// reference URI; it forms no edge but fails this project's component
    /// closed and poisons SHARED snapshot-wide). A reference under
    /// `disableSourceOfProjectReferenceRedirect: true` decouples the
    /// Programs and must NOT be passed here.
    pub fn insert_project(
        &mut self,
        id: ProjectIdentity,
        eligibility: ProjectEligibility,
        redirect_on_refs: Vec<RedirectRef>,
    ) {
        self.nodes.insert(
            id,
            ProjectNode {
                redirect_on_refs,
                eligibility,
            },
        );
    }

    /// The eligibility of a project, or `None` for a project absent from
    /// the graph (an absent member — fail-closed in
    /// [`select_component_mode`]).
    #[must_use]
    pub fn eligibility(&self, id: &ProjectIdentity) -> Option<ProjectEligibility> {
        self.nodes.get(id).map(|node| node.eligibility)
    }

    /// Whether `id` declares any [`RedirectRef::Unresolved`] redirect-ON
    /// reference — an incompleteness signal that fails the declaring
    /// project's whole component closed in [`select_component_mode`]. A
    /// project absent from the graph declares none (it fails closed for
    /// being absent instead).
    fn has_unresolved_redirect_refs(&self, id: &ProjectIdentity) -> bool {
        self.nodes.get(id).is_some_and(|node| {
            node.redirect_on_refs
                .iter()
                .any(|r| matches!(r, RedirectRef::Unresolved))
        })
    }

    /// Whether ANY inserted node declares an [`RedirectRef::Unresolved`]
    /// redirect-ON reference.
    ///
    /// A single unresolved reference anywhere means the live layer could not
    /// prove which apparently-separate components are truly independent, so NO
    /// component in this snapshot may be served SHARED (its unresolved target
    /// could be the SHARED endpoint of a real cross-project edge). Returns a
    /// deterministic bool — iteration order over the node map cannot affect
    /// the answer. Fails the WHOLE snapshot closed to
    /// OWNED/[`OwnedReason::UnresolvedRedirectInSnapshot`] in
    /// [`select_component_mode`], one precedence level below the member-local
    /// [`OwnedReason::IncompleteComponent`] rule.
    #[must_use]
    pub fn any_unresolved_redirect_refs(&self) -> bool {
        self.nodes.values().any(|node| {
            node.redirect_on_refs
                .iter()
                .any(|r| matches!(r, RedirectRef::Unresolved))
        })
    }

    /// The UNDIRECTED connected component of `root` over redirect-ON
    /// reference edges — the mode-selection unit.
    ///
    /// An edge `A → B` connects the component in BOTH directions, so
    /// rooting at ANY member yields the SAME component (entry-independent —
    /// the strongest no-split-brain form; directed reachability would be
    /// entry-dependent). Cycle-safe via the member set itself; members are
    /// held in canonical byte order, so the result — and every decision
    /// derived from it — is deterministic and reproducible. Resolved
    /// referenced identities absent from the graph are included as (absent)
    /// members; a root absent from the graph still anchors its component
    /// through the reverse edges of the nodes that reference it.
    #[must_use]
    pub fn connected_component(&self, root: &ProjectIdentity) -> ReferenceComponent {
        // Symmetric adjacency over every RESOLVED edge endpoint, present or
        // absent. Unresolved references carry no identity, so they form no
        // edge (they fail closed in `select_component_mode` — member-local for
        // their declaring component, snapshot-wide for every other — not here).
        let mut adjacency: FxHashMap<ProjectIdentity, Vec<ProjectIdentity>> = FxHashMap::default();
        for (id, node) in &self.nodes {
            for referenced in &node.redirect_on_refs {
                if let RedirectRef::Resolved(referenced) = referenced {
                    adjacency.entry(*id).or_default().push(*referenced);
                    adjacency.entry(*referenced).or_default().push(*id);
                }
            }
        }

        let mut members = BTreeSet::new();
        members.insert(*root);
        let mut frontier = VecDeque::from([*root]);
        while let Some(current) = frontier.pop_front() {
            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if members.insert(*neighbor) {
                        frontier.push_back(*neighbor);
                    }
                }
            }
        }
        ReferenceComponent { members }
    }
}

/// A redirect-ON reference connected component — THE mode-selection unit.
///
/// Only mintable via [`RedirectReferenceGraph::connected_component`], so a
/// decision's member set is always a whole component, never a hand-picked
/// subset. Members are held in canonical byte order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceComponent {
    members: BTreeSet<ProjectIdentity>,
}

impl ReferenceComponent {
    /// The members in canonical (byte) order.
    pub fn members(&self) -> impl Iterator<Item = ProjectIdentity> + '_ {
        self.members.iter().copied()
    }

    /// Whether `id` is a member of this component.
    #[must_use]
    pub fn contains(&self, id: &ProjectIdentity) -> bool {
        self.members.contains(id)
    }

    /// The number of members (always at least one — the root).
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Never true for a component produced by
    /// [`RedirectReferenceGraph::connected_component`] (the root is always
    /// a member); provided for API completeness.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Opaque identity facts of one engine session, supplied by the caller
/// (for the OWNED session: the spawned engine; for the SHARED session: the
/// attached editor engine).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineSessionFacts {
    /// The engine version observed in-band from the session itself.
    pub observed_version: Arc<str>,
    /// The negotiated wire/protocol pin of the session.
    pub wire_pin: u64,
    /// The attach-session generation (bumps on editor engine restart or
    /// reconnect; the OWNED session uses its own spawn generation).
    pub editor_session_generation: u64,
}

/// Provenance-typed OWNED-session facts: a DISTINCT newtype from
/// [`SharedSessionFacts`] whose inner [`EngineSessionFacts`] is SEALED
/// (private) and constructed only through [`OwnedSessionFacts::new`].
///
/// Two guarantees hold at compile time. An owned-typed value is NOT
/// assignable to a SHARED slot (and vice versa), so an accidental slot swap
/// is a COMPILE error. And because the inner field is private, the sibling
/// SHARED newtype's facts cannot be moved into this one by a bare field
/// re-wrap — `OwnedSessionFacts(shared.0)` does not compile; the only way in
/// is the typed constructor over a caller-supplied [`EngineSessionFacts`].
/// Supplying independently-constructed OWNED facts through
/// [`OwnedSessionFacts::new`] is legitimate — a caller-declared OWNED
/// session, not laundering. Mirrors how [`EligibilityFailure`] ⊂
/// [`OwnedReason`] keeps a derived reason out of the eligibility-INPUT
/// position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OwnedSessionFacts(EngineSessionFacts);

impl OwnedSessionFacts {
    /// Wrap `facts` as the OWNED-session provenance type.
    #[must_use]
    pub fn new(facts: EngineSessionFacts) -> Self {
        Self(facts)
    }

    /// Borrow the sealed inner session facts.
    #[must_use]
    pub fn facts(&self) -> &EngineSessionFacts {
        &self.0
    }
}

/// Provenance-typed SHARED-session facts: a DISTINCT newtype from
/// [`OwnedSessionFacts`] whose inner [`EngineSessionFacts`] is SEALED
/// (private) and constructed only through [`SharedSessionFacts::new`].
///
/// A shared-typed value is NOT assignable to the OWNED slot (and vice
/// versa), so an accidental slot swap is a COMPILE error; and because the
/// inner field is private, the OWNED newtype's facts cannot be moved into
/// this one by a bare field re-wrap — `SharedSessionFacts(owned.0)` does not
/// compile. Placing facts here goes through the typed constructor over a
/// caller-supplied [`EngineSessionFacts`]; supplying independently-
/// constructed SHARED facts that way is a legitimate SHARED session, not
/// laundering.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SharedSessionFacts(EngineSessionFacts);

impl SharedSessionFacts {
    /// Wrap `facts` as the SHARED-session provenance type.
    #[must_use]
    pub fn new(facts: EngineSessionFacts) -> Self {
        Self(facts)
    }

    /// Borrow the sealed inner session facts.
    #[must_use]
    pub fn facts(&self) -> &EngineSessionFacts {
        &self.0
    }
}

/// The candidate engine sessions a selection chooses between. OWNED is
/// always available (the universal baseline); the SHARED editor-attach
/// session is present only when a live attach exists, so it is optional —
/// its absence fails a would-be-SHARED decision closed to OWNED rather than
/// fabricating SHARED facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineSessionCandidates {
    /// The OWNED spawned-engine session — always available, the universal
    /// baseline. Provenance-typed [`OwnedSessionFacts`]; a SHARED-typed value
    /// is not assignable here, so a slot swap is a compile error.
    pub owned: OwnedSessionFacts,
    /// The SHARED editor-attach session, or `None` when no live attach is
    /// available. Provenance-typed [`SharedSessionFacts`]; an OWNED-typed
    /// value is not assignable here and the OWNED newtype's sealed facts
    /// cannot be moved into this slot by a bare field re-wrap, so both are
    /// compile errors. `None` is not a placeholder: a decision that would
    /// otherwise select SHARED fails closed to
    /// OWNED/[`OwnedReason::SharedSessionUnavailable`] instead of inventing
    /// SHARED facts.
    pub shared: Option<SharedSessionFacts>,
}

/// Engine-and-mode identity. The `mode` axis is a first-class identity
/// dimension: an OWNED identity and a SHARED identity over the same
/// project + version are NEVER equal, so state keyed on this identity can
/// never launder one engine's facts into the other's. Carried on every
/// [`ComponentModeDecision`]; the warm-state cache keyed on it is owned by
/// the live editor-attach integration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineIdentity {
    /// The serving mode — the dimension that separates OWNED and SHARED
    /// identities over identical session facts.
    pub mode: ServeMode,
    /// The engine version observed in-band from the serving session.
    pub observed_version: Arc<str>,
    /// The negotiated wire/protocol pin of the serving session.
    pub wire_pin: u64,
    /// The serving session's generation.
    pub editor_session_generation: u64,
}

impl EngineIdentity {
    /// The identity of `session` serving under `mode`.
    #[must_use]
    pub fn for_mode(mode: ServeMode, session: &EngineSessionFacts) -> Self {
        Self {
            mode,
            observed_version: Arc::clone(&session.observed_version),
            wire_pin: session.wire_pin,
            editor_session_generation: session.editor_session_generation,
        }
    }
}

/// ONE mode decision covering a FULL component. There is no narrower
/// decision carrier — per-file / per-single-project mode answers are not
/// representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentModeDecision {
    mode: ServeMode,
    members: ReferenceComponent,
    owned_reason: Option<OwnedReason>,
    engine: EngineIdentity,
}

impl ComponentModeDecision {
    /// The ONE mode every member of [`Self::members`] is served with.
    #[must_use]
    pub fn mode(&self) -> ServeMode {
        self.mode
    }

    /// The full component the decision covers.
    #[must_use]
    pub fn members(&self) -> &ReferenceComponent {
        &self.members
    }

    /// Why the component is OWNED (`None` for a SHARED decision).
    #[must_use]
    pub fn owned_reason(&self) -> Option<OwnedReason> {
        self.owned_reason
    }

    /// The mode-keyed identity of the serving engine session.
    #[must_use]
    pub fn engine(&self) -> &EngineIdentity {
        &self.engine
    }
}

/// Decide the ONE serve mode for the whole component of `root`.
///
/// The component is computed INTERNALLY, from the SAME `graph` the
/// eligibilities live on ([`RedirectReferenceGraph::connected_component`]),
/// so the decision always covers the current, whole component BY
/// CONSTRUCTION — a mismatched or stale member set is not passable.
///
/// SHARED only when EVERY member is [`ProjectEligibility::Eligible`]. The
/// decision applies four fail-closed levels in STRICT precedence:
///
/// 1. **Member-local incompleteness** — if ANY member of THIS component is
///    absent from the graph or declares a [`RedirectRef::Unresolved`]
///    reference, the WHOLE component is OWNED with
///    [`OwnedReason::IncompleteComponent`], regardless of any other member's
///    eligibility (a missing dependency means the eligibility picture cannot
///    be trusted). Runs FIRST, so the declaring component of an unresolved
///    reference always reports THIS reason.
/// 2. **Snapshot-wide unresolved poison** — else if ANY node ANYWHERE in the
///    graph (including OUTSIDE this component) declares a
///    [`RedirectRef::Unresolved`] reference
///    ([`RedirectReferenceGraph::any_unresolved_redirect_refs`]), the whole
///    component is OWNED with [`OwnedReason::UnresolvedRedirectInSnapshot`]:
///    the identity-less target could be the SHARED endpoint of a real
///    cross-project edge with this component, so no snapshot carrying an
///    unresolved reference may be served SHARED.
/// 3. **Per-member eligibility** — else the first member (in canonical member
///    order) that is `Owned(failure)` makes the WHOLE component OWNED with
///    that failure's mapped reason.
/// 4. **SHARED session presence** — else every member is eligible and the
///    snapshot is clean, so SHARED is served only if a SHARED session is
///    present ([`EngineSessionCandidates::shared`] is `Some`); a `None`
///    SHARED session fails the component closed to OWNED with
///    [`OwnedReason::SharedSessionUnavailable`].
///
/// SHARED is never assumed for an unknown member, a dropped dependency, or a
/// snapshot with any unresolved reference. The decision covers the FULL
/// member set; there is no per-member mode output.
#[must_use]
pub fn select_component_mode(
    graph: &RedirectReferenceGraph,
    root: &ProjectIdentity,
    engines: &EngineSessionCandidates,
) -> ComponentModeDecision {
    let component = graph.connected_component(root);

    // Pass 1 — incompleteness is authoritative and takes precedence over any
    // per-member eligibility failure: an absent member (never inserted) or an
    // unresolved redirect-ON reference means the component graph itself is
    // not fully resolved, so it fails closed to OWNED/IncompleteComponent
    // regardless of member order or other members' reasons.
    for member in component.members() {
        if graph.eligibility(&member).is_none() || graph.has_unresolved_redirect_refs(&member) {
            return ComponentModeDecision {
                mode: ServeMode::Owned,
                members: component.clone(),
                owned_reason: Some(OwnedReason::IncompleteComponent),
                engine: EngineIdentity::for_mode(ServeMode::Owned, &engines.owned.0),
            };
        }
    }

    // Snapshot-wide poison — one precedence level below the member-local rule
    // above and above per-member eligibility: an `Unresolved` redirect-ON ref
    // declared ANYWHERE in the snapshot (including a node OUTSIDE this queried
    // component) means the live layer could not prove this component is
    // independent of the unresolved (identity-less) target. Fail the WHOLE
    // snapshot's SHARED closed to OWNED with the DISTINCT
    // `UnresolvedRedirectInSnapshot` reason rather than risk serving one
    // endpoint of a real cross-project edge SHARED while the other is OWNED.
    // The member-local pass above ran FIRST, so a member of THIS component
    // declaring Unresolved already returned `IncompleteComponent`; only an
    // unresolved ref OUTSIDE the queried component reaches here.
    if graph.any_unresolved_redirect_refs() {
        return ComponentModeDecision {
            mode: ServeMode::Owned,
            members: component.clone(),
            owned_reason: Some(OwnedReason::UnresolvedRedirectInSnapshot),
            engine: EngineIdentity::for_mode(ServeMode::Owned, &engines.owned.0),
        };
    }

    // Pass 2 — the component is complete; the first ineligible member (in
    // canonical order) makes the whole component OWNED with its mapped reason.
    for member in component.members() {
        if let Some(ProjectEligibility::Owned(failure)) = graph.eligibility(&member) {
            return ComponentModeDecision {
                mode: ServeMode::Owned,
                members: component.clone(),
                owned_reason: Some(OwnedReason::from(failure)),
                engine: EngineIdentity::for_mode(ServeMode::Owned, &engines.owned.0),
            };
        }
    }
    // Every member is eligible. Serve SHARED only if a real SHARED session
    // exists; otherwise fail closed to the OWNED baseline rather than
    // fabricating a SHARED identity from the OWNED session's facts.
    match &engines.shared {
        Some(shared) => ComponentModeDecision {
            mode: ServeMode::Shared,
            owned_reason: None,
            engine: EngineIdentity::for_mode(ServeMode::Shared, &shared.0),
            members: component,
        },
        None => ComponentModeDecision {
            mode: ServeMode::Owned,
            owned_reason: Some(OwnedReason::SharedSessionUnavailable),
            engine: EngineIdentity::for_mode(ServeMode::Owned, &engines.owned.0),
            members: component,
        },
    }
}

/// Fail a `prior` decision's component over to OWNED as a UNIT, mid-flight.
///
/// The failover binds to the PRIOR [`ComponentModeDecision`] and reuses its
/// EXACT member set ([`ComponentModeDecision::members`]), so the returned
/// decision covers the FULL component the prior selection produced — never
/// a subset. A closed redirect or a dropped member moves the entire component:
/// the `cause` is a [`FailoverCause`] — [`FailoverCause::RedirectClosed`] for a
/// closed/failed redirect, [`FailoverCause::ComponentMemberOwned`] when a
/// sibling member forced the move. A SELECTION-time reason (an eligibility
/// failure, [`OwnedReason::UnresolvedRedirectInSnapshot`],
/// [`OwnedReason::IncompleteComponent`], [`OwnedReason::SharedSessionUnavailable`])
/// is not a valid failover cause and is unrepresentable here. `owned_session`
/// is the provenance-typed [`OwnedSessionFacts`] that takes over — always
/// available, the universal baseline.
#[must_use]
pub fn failover_component_to_owned(
    prior: &ComponentModeDecision,
    cause: FailoverCause,
    owned_session: &OwnedSessionFacts,
) -> ComponentModeDecision {
    ComponentModeDecision {
        mode: ServeMode::Owned,
        members: prior.members().clone(),
        owned_reason: Some(OwnedReason::from(cause)),
        engine: EngineIdentity::for_mode(ServeMode::Owned, &owned_session.0),
    }
}

/// The editor-binding identity witness: does the project the editor bound
/// the carrier to match the project Verter resolved? Canonical-identity
/// equality; a mismatch feeds
/// [`ProjectEligibility::Owned`]`(`[`EligibilityFailure::EditorBindingMismatch`]`)`
/// in the caller's eligibility computation.
#[must_use]
pub fn editor_binding_matches(expected: &ProjectIdentity, editor_bound: &ProjectIdentity) -> bool {
    expected == editor_bound
}

#[cfg(test)]
#[path = "mode_tests.rs"]
mod tests;
