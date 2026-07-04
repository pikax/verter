//! Behavioral unit tests for the engine-mode selection substrate.
//!
//! Every test is discriminating against a concrete broken invariant:
//! a split-brain answer (one redirect-ON edge served by two engines), a
//! partial failover (a member subset moving to OWNED), directed
//! reachability (entry-dependent components), or a mode-free engine
//! identity (OWNED facts laundered as SHARED facts).

use std::sync::Arc;

use super::*;
use crate::file_artifact_store::ProjectIdentity;

/// Distinct canonical project identity from a single byte (byte-orderable:
/// `pid(1) < pid(2) < pid(3)`).
fn pid(b: u8) -> ProjectIdentity {
    ProjectIdentity([b; 16])
}

fn facts(version: &str, pin: u64, generation: u64) -> EngineSessionFacts {
    EngineSessionFacts {
        observed_version: Arc::<str>::from(version),
        wire_pin: pin,
        editor_session_generation: generation,
    }
}

/// Candidate engine sessions with DISTINCT owned/shared facts, so a test can
/// tell which engine session a decision's identity came from. Facts are wrapped
/// in the provenance-typed newtypes ([`OwnedSessionFacts`] /
/// [`SharedSessionFacts`]) the slots require.
fn candidates() -> EngineSessionCandidates {
    EngineSessionCandidates {
        owned: OwnedSessionFacts::new(facts("7.0.1", 1, 0)),
        shared: Some(SharedSessionFacts::new(facts("7.0.1", 7, 3))),
    }
}

/// Build a graph from `(project, eligibility, redirect_on_refs)` rows.
fn graph(
    rows: &[(ProjectIdentity, ProjectEligibility, &[ProjectIdentity])],
) -> RedirectReferenceGraph {
    let mut g = RedirectReferenceGraph::new();
    for (id, eligibility, refs) in rows {
        let refs = refs.iter().map(|r| RedirectRef::Resolved(*r)).collect();
        g.insert_project(*id, *eligibility, refs);
    }
    g
}

fn members_of(component: &ReferenceComponent) -> Vec<ProjectIdentity> {
    component.members().collect()
}

// ── OWNED is the universal default ──

/// Each absent SHARED precondition, alone, keeps the component OWNED with
/// that precise reason — OWNED is the default, not a special case.
#[test]
fn owned_is_the_default_for_each_absent_precondition() {
    let failures = [
        EligibilityFailure::VersionGateNotGreen,
        EligibilityFailure::AttachNotLive,
        EligibilityFailure::ProjectNotBound,
        EligibilityFailure::ProxyUnavailable,
        EligibilityFailure::EditorBindingMismatch,
    ];
    for failure in failures {
        let a = pid(1);
        let g = graph(&[(a, ProjectEligibility::Owned(failure), &[])]);
        let decision = select_component_mode(&g, &a, &candidates());
        assert_eq!(decision.mode(), ServeMode::Owned, "failure {failure:?}");
        // The per-project INPUT failure maps to the DECISION-output reason.
        assert_eq!(decision.owned_reason(), Some(OwnedReason::from(failure)));
        assert_eq!(members_of(decision.members()), vec![a]);
        assert_eq!(decision.engine().mode, ServeMode::Owned);
    }
}

/// SHARED requires EVERY member eligible; a single owned member two hops
/// from the root flips the WHOLE component (the member scan is complete,
/// not root-local).
#[test]
fn shared_only_when_every_member_is_eligible() {
    let (a, b, c) = (pid(1), pid(2), pid(3));

    // All eligible -> SHARED over the full component.
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[c]),
        (c, ProjectEligibility::Eligible, &[]),
    ]);
    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(decision.mode(), ServeMode::Shared);
    assert_eq!(decision.owned_reason(), None);
    assert_eq!(members_of(decision.members()), vec![a, b, c]);
    assert_eq!(decision.engine().mode, ServeMode::Shared);
    // The SHARED identity carries the SHARED session facts.
    assert_eq!(decision.engine().wire_pin, 7);

    // The far member (two redirect hops from the root) turns OWNED -> the
    // WHOLE component is OWNED.
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[c]),
        (
            c,
            ProjectEligibility::Owned(EligibilityFailure::AttachNotLive),
            &[],
        ),
    ]);
    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(decision.mode(), ServeMode::Owned);
    assert_eq!(decision.owned_reason(), Some(OwnedReason::AttachNotLive));
    assert_eq!(members_of(decision.members()), vec![a, b, c]);
}

// ── No split-brain across a cross-project redirect-ON edge ──

/// A—B with B owned: the ONE decision covers {A, B} and is OWNED. The
/// split-brain answer (A SHARED while B OWNED) is not producible — the
/// selection has no per-member output, and selecting from either side
/// yields the identical decision.
#[test]
fn cross_project_owned_member_forces_whole_component_owned() {
    let (a, b) = (pid(1), pid(2));
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (
            b,
            ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable),
            &[],
        ),
    ]);

    let from_a = select_component_mode(&g, &a, &candidates());
    assert_eq!(from_a.mode(), ServeMode::Owned);
    assert_eq!(from_a.owned_reason(), Some(OwnedReason::ProxyUnavailable));
    assert_eq!(members_of(from_a.members()), vec![a, b]);
    assert!(from_a.members().contains(&a) && from_a.members().contains(&b));

    // Entry-independent decision: asking from B produces the same answer
    // over the same member set.
    let from_b = select_component_mode(&g, &b, &candidates());
    assert_eq!(from_a, from_b);
}

// ── Entry independence (undirected component, not directed reachability) ──

/// For the directed chain A -> B -> C, the component rooted at ANY member is
/// the same {A, B, C}. Directed reachability would answer {C} from C and
/// {B, C} from B — entry-dependent, the split-brain hole.
#[test]
fn connected_component_is_entry_independent() {
    let (a, b, c) = (pid(1), pid(2), pid(3));
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[c]),
        (c, ProjectEligibility::Eligible, &[]),
    ]);
    let from_a = g.connected_component(&a);
    let from_b = g.connected_component(&b);
    let from_c = g.connected_component(&c);
    assert_eq!(from_a, from_b);
    assert_eq!(from_b, from_c);
    assert_eq!(members_of(&from_a), vec![a, b, c]);
}

// ── Fail-closed on unresolved members ──

/// A referenced project ABSENT from the graph is still a component member,
/// and its unknown eligibility fails the WHOLE component closed to OWNED
/// (`IncompleteComponent`) — never an assumed-SHARED answer.
#[test]
fn absent_referenced_member_fails_closed_to_owned() {
    let (a, x) = (pid(1), pid(9));
    let g = graph(&[(a, ProjectEligibility::Eligible, &[x])]);

    let component = g.connected_component(&a);
    assert_eq!(members_of(&component), vec![a, x]);

    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(decision.mode(), ServeMode::Owned);
    assert_eq!(
        decision.owned_reason(),
        Some(OwnedReason::IncompleteComponent)
    );
    assert_eq!(members_of(decision.members()), vec![a, x]);

    // Entry independence includes the absent member: rooting at the
    // unresolved id finds the same component through the reverse edge.
    assert_eq!(g.connected_component(&x), component);
}

/// A root that is itself absent from the graph and unreferenced is a
/// one-member component with unknown eligibility -> OWNED, fail-closed.
#[test]
fn unknown_root_is_owned_incomplete() {
    let g = graph(&[(pid(1), ProjectEligibility::Eligible, &[])]);
    let unknown = pid(8);
    let component = g.connected_component(&unknown);
    assert_eq!(members_of(&component), vec![unknown]);
    let decision = select_component_mode(&g, &unknown, &candidates());
    assert_eq!(decision.mode(), ServeMode::Owned);
    assert_eq!(
        decision.owned_reason(),
        Some(OwnedReason::IncompleteComponent)
    );
}

/// A node marked `Eligible` but declaring a redirect-ON reference the live
/// layer could NOT resolve fails the WHOLE component closed to OWNED with
/// `IncompleteComponent`. Silently dropping the unresolved edge — the
/// fail-OPEN bug — would let this eligible-but-incomplete node go SHARED.
#[test]
fn unresolved_redirect_ref_fails_closed_to_owned() {
    let a = pid(1);
    let mut g = RedirectReferenceGraph::new();
    // `a` is itself eligible, but declares one redirect-ON reference that
    // did not resolve to a canonical identity.
    g.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Unresolved],
    );

    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(
        decision.mode(),
        ServeMode::Owned,
        "an eligible node with an unresolved redirect ref must fail closed"
    );
    assert_eq!(
        decision.owned_reason(),
        Some(OwnedReason::IncompleteComponent)
    );
    assert_eq!(decision.engine().mode, ServeMode::Owned);

    // The unresolved ref (not the eligibility) is what fails it: the same
    // node with a RESOLVED reference to an eligible sibling goes SHARED.
    let b = pid(2);
    let mut resolved = RedirectReferenceGraph::new();
    resolved.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Resolved(b)],
    );
    resolved.insert_project(b, ProjectEligibility::Eligible, vec![]);
    let ok = select_component_mode(&resolved, &a, &candidates());
    assert_eq!(
        ok.mode(),
        ServeMode::Shared,
        "resolved-only refs over eligible members stay SHARED"
    );
}

/// Incompleteness OUTRANKS a per-member eligibility failure in the reported
/// reason: a component whose graph is not fully resolved is served OWNED with
/// `IncompleteComponent` even when an earlier (canonical-order) member is
/// `Owned(_)` for another reason. Reporting the earlier owned reason (the
/// first-match-wins behavior) would MASK the incompleteness — the missing
/// dependency means the eligibility picture cannot be trusted, so it is
/// authoritative. The mode is OWNED either way; this pins the reason.
#[test]
fn incompleteness_outranks_eligibility_failure_in_the_reason() {
    let (a, b) = (pid(1), pid(2)); // a < b in canonical member order
    let mut g = RedirectReferenceGraph::new();
    // `a` (first in order) is Owned for an ordinary eligibility reason…
    g.insert_project(
        a,
        ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable),
        vec![RedirectRef::Resolved(b)],
    );
    // …while `b` (later in order) declares an unresolved redirect ref, so the
    // component graph is incomplete.
    g.insert_project(
        b,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Unresolved],
    );

    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(
        decision.mode(),
        ServeMode::Owned,
        "either reason serves OWNED"
    );
    assert_eq!(
        decision.owned_reason(),
        Some(OwnedReason::IncompleteComponent),
        "incompleteness outranks the earlier member's ProxyUnavailable reason"
    );
    assert_eq!(
        members_of(decision.members()),
        vec![a, b],
        "the OWNED decision still covers the whole component"
    );
}

/// TARGET-SIDE split-brain: an `Unresolved` redirect-ON reference declared by a
/// node OUTSIDE the queried component poisons SHARED SNAPSHOT-WIDE. `A` holds an
/// unresolved reference (its URI did not resolve, so it forms NO edge — its own
/// component is `{A}`, protected by the member-local rule). `B` is a
/// SEPARATELY-loaded, independently-eligible node with NO edge to `A`. If `A`
/// and `B` are secretly the two endpoints of the SAME real redirect-ON edge the
/// live layer could not yet prove independent, serving `B` SHARED while `A` is
/// OWNED is exactly the split-brain this layer exists to prevent — and it would
/// falsify the guard's absolute "no cross-project redirect-ON edge is ever split
/// across two engines". So a single unresolved reference ANYWHERE fails the
/// WHOLE snapshot's SHARED closed with the DISTINCT
/// `UnresolvedRedirectInSnapshot` reason, while the member-local
/// `IncompleteComponent` still outranks it for the declaring component.
#[test]
fn unresolved_redirect_anywhere_poisons_shared_snapshot_wide() {
    let (a, b, c) = (pid(1), pid(2), pid(3));
    let mut g = RedirectReferenceGraph::new();
    // `a` is eligible but declares an unresolved redirect-ON ref — it forms no
    // edge, so `a`'s own component is `{a}`.
    g.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Unresolved],
    );
    // `b` is a SEPARATE, independently-eligible node with NO edge to `a`.
    g.insert_project(b, ProjectEligibility::Eligible, vec![]);

    // TARGET SIDE: querying `b` (a separate component `{b}`, member-local-clean,
    // eligible, shared session present) must NOT go SHARED — the unresolved ref
    // living in `a`'s component poisons the whole snapshot.
    let from_b = select_component_mode(&g, &b, &candidates());
    assert_eq!(
        from_b.mode(),
        ServeMode::Owned,
        "an unresolved redirect-ON ref anywhere must fail SHARED closed snapshot-wide, \
         even for a separate independently-eligible component"
    );
    assert_eq!(
        from_b.owned_reason(),
        Some(OwnedReason::UnresolvedRedirectInSnapshot),
        "the snapshot-wide poison has its own DISTINCT reason, not IncompleteComponent"
    );
    assert_eq!(
        members_of(from_b.members()),
        vec![b],
        "the decision still covers exactly B's own component"
    );
    assert_eq!(from_b.engine().mode, ServeMode::Owned);
    // The serving identity carries the OWNED facts, never a laundered SHARED one.
    assert_eq!(from_b.engine().wire_pin, 1);

    // DECLARING SIDE protection intact: querying `a` (a member of THIS component
    // declares Unresolved) still reports the member-local `IncompleteComponent`,
    // which OUTRANKS the snapshot-wide reason.
    let from_a = select_component_mode(&g, &a, &candidates());
    assert_eq!(from_a.mode(), ServeMode::Owned);
    assert_eq!(
        from_a.owned_reason(),
        Some(OwnedReason::IncompleteComponent),
        "a member of the queried component declaring Unresolved reports the \
         member-local IncompleteComponent, not the snapshot-wide reason"
    );
    assert_eq!(members_of(from_a.members()), vec![a]);

    // The POISON is the cause (not something else): with `a`'s ref RESOLVED to an
    // eligible `c`, the snapshot carries NO unresolved ref, so `b` goes SHARED.
    let mut resolved = RedirectReferenceGraph::new();
    resolved.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Resolved(c)],
    );
    resolved.insert_project(c, ProjectEligibility::Eligible, vec![]);
    resolved.insert_project(b, ProjectEligibility::Eligible, vec![]);
    let from_b_clean = select_component_mode(&resolved, &b, &candidates());
    assert_eq!(
        from_b_clean.mode(),
        ServeMode::Shared,
        "with no unresolved ref anywhere, the separate eligible component B goes SHARED"
    );
    assert_eq!(from_b_clean.owned_reason(), None);
}

// ── SHARED requires a SHARED session to exist ──

/// An all-eligible component whose SHARED session is `None` (no live editor
/// attach) fails CLOSED to OWNED with `SharedSessionUnavailable`: SHARED is
/// never served without a real SHARED session, and the OWNED session's
/// facts are never laundered into a SHARED identity.
#[test]
fn all_eligible_but_no_shared_session_fails_closed() {
    let (a, b) = (pid(1), pid(2));
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[]),
    ]);
    let no_shared = EngineSessionCandidates {
        owned: OwnedSessionFacts::new(facts("7.0.1", 1, 0)),
        shared: None,
    };
    let decision = select_component_mode(&g, &a, &no_shared);
    assert_eq!(
        decision.mode(),
        ServeMode::Owned,
        "a would-be-SHARED component with no SHARED session must fail closed"
    );
    assert_eq!(
        decision.owned_reason(),
        Some(OwnedReason::SharedSessionUnavailable)
    );
    assert_eq!(decision.engine().mode, ServeMode::Owned);
    // The serving identity carries the OWNED facts (wire_pin 1), never a
    // laundered SHARED identity.
    assert_eq!(decision.engine().wire_pin, 1);

    // The same all-eligible component WITH a SHARED session present goes
    // SHARED — proving the missing session (not the eligibility) is the
    // fail-closed cause.
    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(decision.mode(), ServeMode::Shared);
    assert_eq!(decision.engine().wire_pin, 7);
}

// ── Redirect-ON edges only ──

/// A reference under `disableSourceOfProjectReferenceRedirect: true` is NOT
/// an edge of this graph (the caller feeds redirect-ON references only), so
/// it does not merge two projects into one mode unit: A's component is
/// exactly its redirect-ON closure {A, C}; B stays its own component and
/// keeps its own independent decision.
#[test]
fn non_redirect_reference_does_not_merge_components() {
    let (a, b, c) = (pid(1), pid(2), pid(3));
    // A -> C is redirect-ON; A's reference to B is redirect-disabled and
    // therefore absent from the graph's edge set.
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[c]),
        (
            b,
            ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable),
            &[],
        ),
        (c, ProjectEligibility::Eligible, &[]),
    ]);

    let component_a = g.connected_component(&a);
    assert_eq!(members_of(&component_a), vec![a, c]);
    assert!(
        !component_a.contains(&b),
        "a redirect-disabled reference must not merge"
    );

    // The decoupled Programs decide independently: {A, C} can be SHARED
    // while {B} is OWNED — that is NOT a split edge, there is no
    // redirect-ON edge between them.
    let decision_a = select_component_mode(&g, &a, &candidates());
    assert_eq!(decision_a.mode(), ServeMode::Shared);
    let component_b = g.connected_component(&b);
    assert_eq!(members_of(&component_b), vec![b]);
    let decision_b = select_component_mode(&g, &b, &candidates());
    assert_eq!(decision_b.mode(), ServeMode::Owned);
}

// ── Whole-unit failover ──

/// A mid-flight failover carries the FULL component to OWNED — the members
/// of the failover decision are exactly the component's members, never a
/// subset, and the engine identity is the OWNED one.
#[test]
fn failover_transitions_the_full_component() {
    let (a, b, c) = (pid(1), pid(2), pid(3));
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[c]),
        (c, ProjectEligibility::Eligible, &[]),
    ]);
    let component = g.connected_component(&a);
    let shared = select_component_mode(&g, &a, &candidates());
    assert_eq!(shared.mode(), ServeMode::Shared);

    let owned_session = candidates().owned;
    let failed_over =
        failover_component_to_owned(&shared, FailoverCause::RedirectClosed, &owned_session);
    assert_eq!(failed_over.mode(), ServeMode::Owned);
    assert_eq!(
        failed_over.owned_reason(),
        Some(OwnedReason::RedirectClosed)
    );
    assert_eq!(
        members_of(failed_over.members()),
        members_of(&component),
        "failover must move the WHOLE component, never a subset"
    );
    assert_eq!(members_of(failed_over.members()), vec![a, b, c]);
    assert_eq!(failed_over.engine().mode, ServeMode::Owned);
    assert_ne!(failed_over.engine(), shared.engine());

    // A member dropping to OWNED mid-flight fails the siblings over with
    // the component-member cause — same whole-unit semantics.
    let sibling_forced =
        failover_component_to_owned(&shared, FailoverCause::ComponentMemberOwned, &owned_session);
    assert_eq!(sibling_forced.mode(), ServeMode::Owned);
    assert_eq!(
        sibling_forced.owned_reason(),
        Some(OwnedReason::ComponentMemberOwned)
    );
    assert_eq!(members_of(sibling_forced.members()), vec![a, b, c]);
}

// ── Cycle safety ──

/// Reference cycles (A -> B -> A, and a self-reference) terminate and yield
/// the same deduplicated component from either entry.
#[test]
fn cyclic_references_are_component_safe() {
    let (a, b) = (pid(1), pid(2));
    let g = graph(&[
        (a, ProjectEligibility::Eligible, &[b, a]),
        (b, ProjectEligibility::Eligible, &[a]),
    ]);
    let from_a = g.connected_component(&a);
    let from_b = g.connected_component(&b);
    assert_eq!(from_a, from_b);
    assert_eq!(members_of(&from_a), vec![a, b]);

    let decision = select_component_mode(&g, &a, &candidates());
    assert_eq!(decision.mode(), ServeMode::Shared);
}

// ── Canonical, deterministic member order ──

/// Component membership and order are independent of insertion order and of
/// the traversal entry: members come out in canonical byte order.
#[test]
fn component_member_order_is_canonical_and_insertion_independent() {
    let (a, b, c) = (pid(1), pid(2), pid(3));
    let forward = graph(&[
        (a, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[c]),
        (c, ProjectEligibility::Eligible, &[]),
    ]);
    // Same edges, reversed insertion order and reversed edge direction
    // (undirected: direction must not matter).
    let reversed = graph(&[
        (c, ProjectEligibility::Eligible, &[b]),
        (b, ProjectEligibility::Eligible, &[a]),
        (a, ProjectEligibility::Eligible, &[]),
    ]);
    let lhs = forward.connected_component(&b);
    let rhs = reversed.connected_component(&c);
    assert_eq!(lhs, rhs);
    assert_eq!(members_of(&lhs), vec![a, b, c]);
    assert_eq!(members_of(&rhs), vec![a, b, c]);
}

// ── Engine identity is mode-keyed ──

/// OWNED and SHARED identities over the IDENTICAL session facts are never
/// equal — the mode dimension alone separates them, so warm state keyed on
/// the identity can never launder one engine's facts into the other's.
#[test]
fn engine_identity_is_mode_keyed_never_equal_across_modes() {
    let same = facts("7.0.1", 42, 5);
    let owned = EngineIdentity::for_mode(ServeMode::Owned, &same);
    let shared = EngineIdentity::for_mode(ServeMode::Shared, &same);
    assert_ne!(owned, shared);
    // Same mode + same facts IS the same identity (equality is real, the
    // inequality above is the mode dimension, not object identity).
    assert_eq!(owned, EngineIdentity::for_mode(ServeMode::Owned, &same));

    // Through the decision surface with identical owned/shared candidate
    // facts: a SHARED selection and an OWNED failover still yield distinct
    // engine identities.
    let a = pid(1);
    let g = graph(&[(a, ProjectEligibility::Eligible, &[])]);
    let same_facts_candidates = EngineSessionCandidates {
        owned: OwnedSessionFacts::new(same.clone()),
        shared: Some(SharedSessionFacts::new(same.clone())),
    };
    let selected = select_component_mode(&g, &a, &same_facts_candidates);
    assert_eq!(selected.mode(), ServeMode::Shared);
    let failed_over = failover_component_to_owned(
        &selected,
        FailoverCause::RedirectClosed,
        &OwnedSessionFacts::new(same.clone()),
    );
    assert_ne!(selected.engine(), failed_over.engine());
}

// ── Session-fact provenance is type-enforced ──

/// Provenance is TYPE-ENFORCED, not convention: the OWNED slot is
/// [`OwnedSessionFacts`] and the SHARED slot is `Option<SharedSessionFacts>`, so
/// an OWNED-typed facts value is not assignable to the SHARED slot (or vice
/// versa) — the compiler rejects it, mirroring how `EligibilityFailure ⊂
/// OwnedReason` closed the earlier laundering footgun. `failover_component_to_owned`
/// takes a [`FailoverCause`] (only a closed redirect or a sibling going OWNED),
/// so an eligibility-input reason or a selection-time reason like
/// `UnresolvedRedirectInSnapshot` is unrepresentable at that call.
#[test]
fn session_provenance_and_failover_cause_are_type_enforced() {
    // The candidate slots ARE the provenance-typed newtypes — these bindings
    // are compile-time witnesses: they fail to compile if a slot's type drifts.
    let c = candidates();
    let _owned_slot: OwnedSessionFacts = c.owned.clone();
    let _shared_slot: Option<SharedSessionFacts> = c.shared.clone();

    // The sealed inner is reachable only through the typed constructor +
    // accessor: constructing from `f` and reading `.facts()` back round-trips
    // to the same value (a real check — a wrong accessor would return a
    // different or fabricated fact set).
    let f = facts("7.0.1", 9, 4);
    assert_eq!(OwnedSessionFacts::new(f.clone()).facts(), &f);
    assert_eq!(SharedSessionFacts::new(f.clone()).facts(), &f);

    // FailoverCause maps to exactly its OWNED reason — a real mapping, not a
    // constant: a mismap (e.g. RedirectClosed -> ComponentMemberOwned) fails here.
    assert_eq!(
        OwnedReason::from(FailoverCause::RedirectClosed),
        OwnedReason::RedirectClosed
    );
    assert_eq!(
        OwnedReason::from(FailoverCause::ComponentMemberOwned),
        OwnedReason::ComponentMemberOwned
    );

    // failover takes a FailoverCause and produces the mapped OWNED reason over
    // the whole prior component; the OWNED identity reads the newtype's inner
    // facts (provenance, not a behavior change).
    let a = pid(1);
    let g = graph(&[(a, ProjectEligibility::Eligible, &[])]);
    let prior = select_component_mode(&g, &a, &candidates());
    assert_eq!(prior.mode(), ServeMode::Shared);
    let owned_session = OwnedSessionFacts::new(facts("7.0.1", 1, 0));
    let failed = failover_component_to_owned(&prior, FailoverCause::RedirectClosed, &owned_session);
    assert_eq!(failed.mode(), ServeMode::Owned);
    assert_eq!(failed.owned_reason(), Some(OwnedReason::RedirectClosed));
    assert_eq!(failed.engine().mode, ServeMode::Owned);
    assert_eq!(
        failed.engine(),
        &EngineIdentity::for_mode(ServeMode::Owned, owned_session.facts()),
        "the OWNED identity reads the inner facts of the OwnedSessionFacts newtype"
    );
}

// ── Editor-binding identity witness ──

/// The editor-binding witness is canonical-identity equality: the same
/// `ProjectIdentity` matches; a different one does not (feeding
/// `Owned(EditorBindingMismatch)` in the eligibility computation).
#[test]
fn editor_binding_matches_is_identity_equality() {
    assert!(editor_binding_matches(&pid(1), &pid(1)));
    assert!(!editor_binding_matches(&pid(1), &pid(2)));
}
