//! Guard: `shared_mode_failover_is_per_reference_closure`.
//!
//! The engine-mode selection unit for external-TS carrier requests is the
//! whole redirect-ON project-reference connected component, and failover is
//! per that unit — never per file, never per single project. No
//! cross-project redirect-ON edge is ever split across two engines (no
//! half-SHARED/half-OWNED answer) — on BOTH the declaring side (a resolved
//! or unresolved reference in the queried component) AND the target side (an
//! unresolved reference anywhere else in the snapshot). Seven behavioral
//! facts, exercised against the REAL `verter_session::external_ts::mode`
//! substrate:
//!
//!   1. `select_component_mode` returns SHARED only when EVERY component
//!      member is `Eligible`; any `Owned` or graph-absent member turns the
//!      WHOLE component OWNED (absent ⇒ `IncompleteComponent`, fail-closed).
//!   2. A cross-project redirect-ON component A—B with B owned decides
//!      OWNED over the full member set {A, B} from EITHER entry — the
//!      split-brain answer (A SHARED, B OWNED) is not producible.
//!   3. `failover_component_to_owned` covers the FULL component — a member
//!      subset never fails over alone.
//!   4. `connected_component` is entry-independent: for the directed
//!      reference chain A → B → C, rooting at any member yields the same
//!      {A, B, C} (undirected traversal, not directed reachability).
//!   5. A reference under `disableSourceOfProjectReferenceRedirect: true`
//!      is not an edge of the graph, so it does not merge two projects
//!      into one mode unit.
//!   6. An OWNED and a SHARED `EngineIdentity` over IDENTICAL session
//!      facts are never equal — the mode dimension is first-class identity.
//!   7. An `Unresolved` redirect-ON reference declared by ANY node in the
//!      snapshot — including one OUTSIDE the queried component — fails SHARED
//!      closed SNAPSHOT-WIDE with the distinct
//!      `OwnedReason::UnresolvedRedirectInSnapshot`. This closes the
//!      TARGET-SIDE split: an identity-less (unresolved) target could be the
//!      SHARED endpoint of a real cross-project edge, so no separate,
//!      independently-eligible component may be served SHARED while it stands
//!      (the member-local `IncompleteComponent` still outranks it for the
//!      declaring component).
//!
//! Plus one STRUCTURAL fact: `mode.rs` exposes NO per-file /
//! per-single-project mode-selection API (`select_file_mode`,
//! `select_project_mode`, `mode_for_file`, …) — the component is the only
//! selection unit; a narrower API is the split-brain escape hatch.
//!
//! The self-test proves every predicate DISCRIMINATES by running the same
//! predicates against deliberately broken plants: a per-project selector
//! (split-brain), a subset failover, a directed-reachability component
//! walker, a whole-graph merge, a mode-free engine identity, a local-only
//! unresolved check (the target-side split), and a source snippet exposing
//! `pub fn select_file_mode`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use verter_session::external_ts::{
    failover_component_to_owned, select_component_mode, EligibilityFailure, EngineIdentity,
    EngineSessionCandidates, EngineSessionFacts, FailoverCause, OwnedReason, OwnedSessionFacts,
    ProjectEligibility, RedirectRef, RedirectReferenceGraph, ServeMode, SharedSessionFacts,
};
use verter_session::file_artifact_store::ProjectIdentity;

fn pid(b: u8) -> ProjectIdentity {
    ProjectIdentity([b; 16])
}

fn session(pin: u64) -> EngineSessionFacts {
    EngineSessionFacts {
        observed_version: Arc::<str>::from("7.0.1"),
        wire_pin: pin,
        editor_session_generation: 1,
    }
}

fn candidates() -> EngineSessionCandidates {
    EngineSessionCandidates {
        owned: OwnedSessionFacts::new(session(1)),
        shared: Some(SharedSessionFacts::new(session(2))),
    }
}

/// `(project, eligibility, redirect_on_refs)` rows — the one construction
/// shared by the real graph and the self-test plants, so both judge the
/// same input.
type GraphRows = Vec<(ProjectIdentity, ProjectEligibility, Vec<ProjectIdentity>)>;

fn build_graph(rows: &GraphRows) -> RedirectReferenceGraph {
    let mut graph = RedirectReferenceGraph::new();
    for (id, eligibility, refs) in rows {
        let refs = refs.iter().map(|r| RedirectRef::Resolved(*r)).collect();
        graph.insert_project(*id, *eligibility, refs);
    }
    graph
}

/// A mode answer as judged by the predicates: the ONE mode and the member
/// set it applies to (canonical order).
type ModeAnswer = (ServeMode, Vec<ProjectIdentity>);

/// The REAL selection surface: `select_component_mode` computes the
/// component of `root` internally from the graph, then returns the one
/// component-wide decision.
fn real_answer(rows: &GraphRows, root: ProjectIdentity) -> ModeAnswer {
    let graph = build_graph(rows);
    let decision = select_component_mode(&graph, &root, &candidates());
    (decision.mode(), decision.members().members().collect())
}

fn real_component(rows: &GraphRows, root: ProjectIdentity) -> Vec<ProjectIdentity> {
    build_graph(rows)
        .connected_component(&root)
        .members()
        .collect()
}

// ── The shared graph fixtures ──

/// Cross-project redirect-ON edge A → B, A eligible, B owned.
fn rows_a_eligible_b_owned(a: ProjectIdentity, b: ProjectIdentity) -> GraphRows {
    vec![
        (a, ProjectEligibility::Eligible, vec![b]),
        (
            b,
            ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable),
            vec![],
        ),
    ]
}

/// Directed redirect-ON chain A → B → C, all eligible.
fn rows_chain_all_eligible(
    a: ProjectIdentity,
    b: ProjectIdentity,
    c: ProjectIdentity,
) -> GraphRows {
    vec![
        (a, ProjectEligibility::Eligible, vec![b]),
        (b, ProjectEligibility::Eligible, vec![c]),
        (c, ProjectEligibility::Eligible, vec![]),
    ]
}

// ── Fact predicates (shared by the real-impl assertions and the plants) ──

/// Facts 1 + 2 — over A—B with B owned, the answer from EITHER entry is
/// OWNED covering the full {A, B}; no entry ever gets a SHARED or
/// partial-member answer.
fn no_split_brain(
    answer_for: impl Fn(ProjectIdentity) -> ModeAnswer,
    a: ProjectIdentity,
    b: ProjectIdentity,
) -> bool {
    let whole = vec![a, b];
    let (mode_a, members_a) = answer_for(a);
    let (mode_b, members_b) = answer_for(b);
    mode_a == ServeMode::Owned
        && mode_b == ServeMode::Owned
        && members_a == whole
        && members_b == whole
}

/// Fact 3 — the failover member set is exactly the full component.
fn whole_unit_failover(
    failover_members: &[ProjectIdentity],
    component_members: &[ProjectIdentity],
) -> bool {
    failover_members == component_members
}

/// Fact 4 — the component of every member of the A → B → C chain is the
/// same full {A, B, C}.
fn entry_independent(
    component_of: impl Fn(ProjectIdentity) -> Vec<ProjectIdentity>,
    a: ProjectIdentity,
    b: ProjectIdentity,
    c: ProjectIdentity,
) -> bool {
    let whole = vec![a, b, c];
    component_of(a) == whole && component_of(b) == whole && component_of(c) == whole
}

/// Structural fact — the substrate source exposes no per-file /
/// per-single-project mode-selection API. Returns the offending needles.
fn per_file_api_violations(mode_source: &str) -> Vec<&'static str> {
    const FORBIDDEN: &[&str] = &[
        "fn select_file_mode",
        "fn select_project_mode",
        "fn select_uri_mode",
        "fn select_path_mode",
        "fn mode_for_file",
        "fn mode_for_project",
        "fn mode_for_uri",
        "fn mode_for_path",
        "fn file_mode",
        "fn project_mode",
    ];
    FORBIDDEN
        .iter()
        .filter(|needle| mode_source.contains(**needle))
        .copied()
        .collect()
}

fn mode_rs_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/external_ts/mode.rs");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ── The guard ──

#[test]
fn shared_mode_failover_is_per_reference_closure() {
    let (a, b, c) = (pid(1), pid(2), pid(3));

    // Fact 1 — SHARED only when EVERY member is eligible…
    let all_eligible = rows_chain_all_eligible(a, b, c);
    let (mode, members) = real_answer(&all_eligible, a);
    assert_eq!(mode, ServeMode::Shared, "all-eligible component is SHARED");
    assert_eq!(
        members,
        vec![a, b, c],
        "the decision covers the full component"
    );
    // …any owned member ⇒ the WHOLE component OWNED…
    let far_member_owned = vec![
        (a, ProjectEligibility::Eligible, vec![b]),
        (b, ProjectEligibility::Eligible, vec![c]),
        (
            c,
            ProjectEligibility::Owned(EligibilityFailure::AttachNotLive),
            vec![],
        ),
    ];
    let (mode, members) = real_answer(&far_member_owned, a);
    assert_eq!(
        mode,
        ServeMode::Owned,
        "one owned member turns the whole component OWNED"
    );
    assert_eq!(members, vec![a, b, c]);
    // …and an ABSENT (unresolved) member fails the whole component closed.
    let unresolved_member = vec![(a, ProjectEligibility::Eligible, vec![b])];
    let graph = build_graph(&unresolved_member);
    let decision = select_component_mode(&graph, &a, &candidates());
    assert_eq!(
        decision.mode(),
        ServeMode::Owned,
        "unresolved member fails closed to OWNED"
    );
    assert_eq!(
        decision.owned_reason(),
        Some(OwnedReason::IncompleteComponent),
        "fail-closed reason is IncompleteComponent"
    );
    assert_eq!(
        decision.members().members().collect::<Vec<_>>(),
        vec![a, b],
        "the unresolved member is still part of the decided unit"
    );

    // Fact 2 — cross-project A—B with B owned: OWNED over {A, B} from
    // either entry; the split-brain answer is not producible.
    let rows = rows_a_eligible_b_owned(a, b);
    assert!(
        no_split_brain(|root| real_answer(&rows, root), a, b),
        "A—B with B owned must decide OWNED over the whole {{A, B}} from either entry"
    );

    // Fact 3 — failover binds to the PRIOR decision and moves the FULL
    // component, never a subset.
    let graph = build_graph(&rows_chain_all_eligible(a, b, c));
    let prior = select_component_mode(&graph, &a, &candidates());
    let component_members: Vec<_> = prior.members().members().collect();
    let failed_over = failover_component_to_owned(
        &prior,
        FailoverCause::RedirectClosed,
        &OwnedSessionFacts::new(session(1)),
    );
    assert_eq!(failed_over.mode(), ServeMode::Owned);
    assert_eq!(
        failed_over.owned_reason(),
        Some(OwnedReason::RedirectClosed)
    );
    let failover_members: Vec<_> = failed_over.members().members().collect();
    assert!(
        whole_unit_failover(&failover_members, &component_members),
        "failover must carry the FULL component: {failover_members:?} vs {component_members:?}"
    );

    // Fact 4 — entry independence (undirected component).
    let chain = rows_chain_all_eligible(a, b, c);
    assert!(
        entry_independent(|root| real_component(&chain, root), a, b, c),
        "the component must be identical from every entry (undirected, not directed reachability)"
    );

    // Fact 5 — a redirect-disabled reference is NOT an edge: with only
    // A → C fed as redirect-ON (A's reference to B is redirect-disabled and
    // therefore absent), B stays outside A's mode unit.
    let disabled_ref_rows = vec![
        (a, ProjectEligibility::Eligible, vec![c]),
        (
            b,
            ProjectEligibility::Owned(EligibilityFailure::ProxyUnavailable),
            vec![],
        ),
        (c, ProjectEligibility::Eligible, vec![]),
    ];
    let component_a = real_component(&disabled_ref_rows, a);
    assert_eq!(
        component_a,
        vec![a, c],
        "redirect-ON closure of A is exactly {{A, C}}"
    );
    assert!(
        !component_a.contains(&b),
        "a redirect-disabled reference must not merge B into A's mode unit"
    );
    // The decoupled units decide independently — no shared edge, no split.
    let (mode_a, _) = real_answer(&disabled_ref_rows, a);
    let (mode_b, members_b) = real_answer(&disabled_ref_rows, b);
    assert_eq!(mode_a, ServeMode::Shared);
    assert_eq!((mode_b, members_b), (ServeMode::Owned, vec![b]));

    // Fact 6 — engine identity is mode-keyed: identical session facts,
    // different mode ⇒ never equal.
    let same = session(9);
    assert_ne!(
        EngineIdentity::for_mode(ServeMode::Owned, &same),
        EngineIdentity::for_mode(ServeMode::Shared, &same),
        "OWNED and SHARED identities over identical facts must be distinct"
    );

    // Fact 7 — an unresolved redirect-ON ref ANYWHERE poisons SHARED
    // snapshot-wide (the target-side split). `a` declares an unresolved ref
    // (its own component {a} is protected member-local); `b` is a SEPARATE,
    // independently-eligible node with NO edge to `a`. Querying `b` must fail
    // closed to OWNED/UnresolvedRedirectInSnapshot — never SHARED.
    let mut poisoned = RedirectReferenceGraph::new();
    poisoned.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Unresolved],
    );
    poisoned.insert_project(b, ProjectEligibility::Eligible, vec![]);
    let target_side = select_component_mode(&poisoned, &b, &candidates());
    assert_eq!(
        target_side.mode(),
        ServeMode::Owned,
        "a separate eligible component must fail closed while an unresolved ref stands anywhere"
    );
    assert_eq!(
        target_side.owned_reason(),
        Some(OwnedReason::UnresolvedRedirectInSnapshot),
        "the target-side poison has its own distinct reason"
    );
    assert_eq!(
        target_side.members().members().collect::<Vec<_>>(),
        vec![b],
        "the decision still covers exactly B's own component"
    );
    // Declaring side keeps the member-local reason (outranks the snapshot-wide).
    let declaring_side = select_component_mode(&poisoned, &a, &candidates());
    assert_eq!(declaring_side.mode(), ServeMode::Owned);
    assert_eq!(
        declaring_side.owned_reason(),
        Some(OwnedReason::IncompleteComponent),
        "the member-local IncompleteComponent outranks the snapshot-wide reason"
    );

    // Structural fact — no per-file / per-single-project mode API exists
    // in the substrate, and the component-unit entry points do.
    let source = mode_rs_source();
    let violations = per_file_api_violations(&source);
    assert!(
        violations.is_empty(),
        "mode.rs must expose NO per-file/per-single-project mode-selection API \
         (the component is the only unit); found: {violations:?}"
    );
    assert!(
        source.contains("pub fn select_component_mode("),
        "the component-wide selection entry point must exist"
    );
    assert!(
        source.contains("pub fn failover_component_to_owned("),
        "the component-wide failover entry point must exist"
    );
    assert!(
        source.contains("pub fn connected_component("),
        "the component constructor must exist"
    );
}

// ── Self-test: every predicate rejects a broken plant ──

#[test]
fn shared_mode_failover_is_per_reference_closure_self_test_discriminates() {
    let (a, b, c) = (pid(1), pid(2), pid(3));

    // Plant vs facts 1/2 — a PER-PROJECT selector (the split-brain bug):
    // answers each project from its own eligibility alone, so A gets
    // SHARED while its redirect-ON sibling B is OWNED.
    let rows = rows_a_eligible_b_owned(a, b);
    let per_project_plant = |root: ProjectIdentity| -> ModeAnswer {
        let graph = build_graph(&rows);
        match graph.eligibility(&root) {
            Some(ProjectEligibility::Eligible) => (ServeMode::Shared, vec![root]),
            Some(ProjectEligibility::Owned(_)) | None => (ServeMode::Owned, vec![root]),
        }
    };
    assert!(
        !no_split_brain(per_project_plant, a, b),
        "the split-brain predicate must REJECT a per-project selector"
    );
    assert!(
        no_split_brain(|root| real_answer(&rows, root), a, b),
        "…and ACCEPT the real component-wide selection"
    );

    // Plant vs fact 3 — a PARTIAL failover that drops a member.
    let graph = build_graph(&rows_chain_all_eligible(a, b, c));
    let component_members: Vec<_> = graph.connected_component(&a).members().collect();
    let mut subset = component_members.clone();
    subset.pop();
    assert!(
        !whole_unit_failover(&subset, &component_members),
        "the whole-unit predicate must REJECT a member-subset failover"
    );
    assert!(whole_unit_failover(&component_members, &component_members));

    // Plant vs fact 4 — DIRECTED reachability: walks only the declared
    // (forward) reference direction, so the component depends on the entry.
    let chain = rows_chain_all_eligible(a, b, c);
    let directed_plant = |root: ProjectIdentity| -> Vec<ProjectIdentity> {
        let mut reached = vec![root];
        let mut frontier = vec![root];
        while let Some(current) = frontier.pop() {
            for (id, _, refs) in &chain {
                if *id == current {
                    for referenced in refs {
                        if !reached.contains(referenced) {
                            reached.push(*referenced);
                            frontier.push(*referenced);
                        }
                    }
                }
            }
        }
        reached.sort();
        reached
    };
    assert_eq!(
        directed_plant(c),
        vec![c],
        "the plant really is directed (C reaches only itself)"
    );
    assert!(
        !entry_independent(directed_plant, a, b, c),
        "the entry-independence predicate must REJECT directed reachability"
    );
    assert!(entry_independent(
        |root| real_component(&chain, root),
        a,
        b,
        c
    ));

    // Plant vs fact 5 — a whole-graph merge that unions every node in the
    // graph regardless of edges would pull B into A's unit.
    let disabled_ref_rows = vec![
        (a, ProjectEligibility::Eligible, vec![c]),
        (b, ProjectEligibility::Eligible, vec![]),
        (c, ProjectEligibility::Eligible, vec![]),
    ];
    let merge_all_plant: Vec<ProjectIdentity> = {
        let mut all: Vec<_> = disabled_ref_rows.iter().map(|(id, _, _)| *id).collect();
        all.sort();
        all
    };
    assert!(
        merge_all_plant.contains(&b),
        "the merge-everything plant wrongly merges the un-edged B"
    );
    assert!(!real_component(&disabled_ref_rows, a).contains(&b));

    // Plant vs fact 6 — a MODE-FREE identity (facts without the mode
    // dimension) makes OWNED and SHARED indistinguishable.
    let same = session(9);
    let mode_free_owned = (
        Arc::clone(&same.observed_version),
        same.wire_pin,
        same.editor_session_generation,
    );
    let mode_free_shared = (
        Arc::clone(&same.observed_version),
        same.wire_pin,
        same.editor_session_generation,
    );
    assert_eq!(
        mode_free_owned, mode_free_shared,
        "the mode-free plant identity cannot tell the engines apart"
    );
    assert_ne!(
        EngineIdentity::for_mode(ServeMode::Owned, &same),
        EngineIdentity::for_mode(ServeMode::Shared, &same),
        "…the real identity can"
    );

    // Plant vs fact 7 — a LOCAL-ONLY unresolved check (exactly the pre-fix
    // behavior: fail closed only when a member of the QUERIED component
    // declares Unresolved). Over the A=[Unresolved] / separate-B snapshot it
    // serves root B SHARED while A is OWNED — the target-side split. The real
    // snapshot-wide predicate must REJECT that (B → OWNED) while the plant
    // ACCEPTS it (B → SHARED).
    let mut poisoned = RedirectReferenceGraph::new();
    poisoned.insert_project(
        a,
        ProjectEligibility::Eligible,
        vec![RedirectRef::Unresolved],
    );
    poisoned.insert_project(b, ProjectEligibility::Eligible, vec![]);
    // The nodes that declare an Unresolved redirect-ON ref in this snapshot —
    // the plant models the pre-fix member-local check against this set only.
    let unresolved_declarers = [a];
    let local_only_unresolved_plant = |root: ProjectIdentity| -> ServeMode {
        let component = poisoned.connected_component(&root);
        // Pre-fix: fail closed only if a member of the QUERIED component
        // declares Unresolved (member-local), else decide by eligibility.
        let member_local_unresolved = component
            .members()
            .any(|m| unresolved_declarers.contains(&m));
        if member_local_unresolved {
            return ServeMode::Owned;
        }
        match poisoned.eligibility(&root) {
            Some(ProjectEligibility::Eligible) => ServeMode::Shared,
            _ => ServeMode::Owned,
        }
    };
    assert_eq!(
        local_only_unresolved_plant(b),
        ServeMode::Shared,
        "the local-only plant really splits: it serves separate B SHARED while A is OWNED"
    );
    let real_target = select_component_mode(&poisoned, &b, &candidates());
    assert_eq!(
        real_target.mode(),
        ServeMode::Owned,
        "…the real snapshot-wide predicate refuses to split — B fails closed to OWNED"
    );
    assert_eq!(
        real_target.owned_reason(),
        Some(OwnedReason::UnresolvedRedirectInSnapshot),
        "…with the distinct target-side reason"
    );
    assert_ne!(
        local_only_unresolved_plant(b),
        real_target.mode(),
        "the plant and the real selector disagree on the target side — the predicate discriminates"
    );

    // Plant vs the structural fact — a source snippet exposing a per-file
    // API is flagged; the real substrate source is clean.
    let plant_source = "impl ModeApi {\n    pub fn select_file_mode(&self, uri: &str) -> ServeMode {\n        ServeMode::Shared\n    }\n}\n";
    assert_eq!(
        per_file_api_violations(plant_source),
        vec!["fn select_file_mode"],
        "the structural scan must FLAG a per-file mode API"
    );
    let narrower_unit_plant =
        "pub fn mode_for_project(id: &ProjectIdentity) -> ServeMode { ServeMode::Owned }";
    assert_eq!(
        per_file_api_violations(narrower_unit_plant),
        vec!["fn mode_for_project"],
        "the structural scan must FLAG a per-single-project mode API"
    );
    assert!(per_file_api_violations(&mode_rs_source()).is_empty());
}
