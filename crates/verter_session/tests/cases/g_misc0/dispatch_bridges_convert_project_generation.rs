//! `accumulate_dispatch_dep_signature` and `observe_fence_entry` must
//! convert `DepVersion::ProjectGeneration` into a
//! `FactVersionRef::ProjectGeneration` — the same conversion the
//! sibling bridge `dep_signature_to_fact_signature` performs.
//!
//! `ProjectGeneration` carries an authoritative validating fact: the
//! project-wide generation a sub-result depended on, validated by
//! `StoreView::validates`'s `ProjectGeneration` arm. A
//! fact-only consumer fed through either of these bridges that never
//! sees the `ProjectGeneration` fact cannot detect a project-shape
//! change (`tsconfig`, path-alias, SDK, workspace-folder edit) — it
//! would validate a stale result against a superseded project shape.
//!
//! `RouteGeneration` is the opposite: there is no
//! `FactVersionRef::RouteGeneration` variant and no authoritative
//! validating source, so it is the sole dropped variant — a defensive
//! floor. No production path constructs `DepVersion::RouteGeneration`.
//!
//! Discrimination property: against the pre-fix tree both bridges
//! DROP `ProjectGeneration` (the `accumulate_dispatch_dep_signature`
//! joint `RouteGeneration | ProjectGeneration` `continue` arm; the
//! `observe_fence_entry` `WholeHash`-only arm) — the
//! `project_generation_converts` assertions below FAIL. Post-fix both
//! bridges convert it and the assertions PASS, while `RouteGeneration`
//! stays dropped.

use std::sync::Arc;

use verter_session::for_tests::{
    accumulate_dispatch_dep_signature_for_tests, observe_fence_entry_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::semantic_query::{DepSignature, DepVersion};
use verter_session::{HostConfig, VerterHost};

fn make_dep_sig(entries: Vec<(&str, DepVersion)>) -> DepSignature {
    Arc::from(
        entries
            .into_iter()
            .map(|(canon, ver)| (Arc::from(canon), ver))
            .collect::<Vec<_>>(),
    )
}

// ----------------------------------------------------------------------------
// accumulate_dispatch_dep_signature
// ----------------------------------------------------------------------------

#[test]
fn accumulate_dispatch_converts_project_generation() {
    // A `ProjectGeneration` dep MUST land in the accumulator as a
    // `FactVersionRef::ProjectGeneration` carrying the same
    // generation. Pre-fix the joint `RouteGeneration | ProjectGeneration`
    // arm drops it and the accumulator is empty.
    let sig = make_dep_sig(vec![("x.ts", DepVersion::ProjectGeneration(77))]);

    let result = accumulate_dispatch_dep_signature_for_tests(&sig);

    assert_eq!(
        result.len(),
        1,
        "ProjectGeneration must convert to one FactVersionRef in the \
         dispatch accumulator, not be dropped"
    );
    assert_eq!(
        result[0],
        FactVersionRef::ProjectGeneration { generation: 77 },
        "ProjectGeneration(77) must convert to \
         FactVersionRef::ProjectGeneration with generation 77"
    );
}

#[test]
fn accumulate_dispatch_drops_route_generation() {
    // `RouteGeneration` has no `FactVersionRef` peer — the dispatch
    // accumulator drops it (defensive floor). The sibling `WholeHash`
    // entry still converts so the call is observably non-empty.
    let sig = make_dep_sig(vec![
        ("a.ts", DepVersion::RouteGeneration(9)),
        ("b.ts", DepVersion::WholeHash([3u8; 16])),
    ]);

    let result = accumulate_dispatch_dep_signature_for_tests(&sig);

    assert_eq!(
        result.len(),
        1,
        "RouteGeneration must be dropped by the dispatch accumulator; \
         only the WholeHash entry survives"
    );
    assert!(
        !result
            .iter()
            .any(|f| matches!(f, FactVersionRef::ProjectGeneration { .. })),
        "no ProjectGeneration fact may appear for a RouteGeneration dep"
    );
    match &result[0] {
        FactVersionRef::FileWholeHash { canonical_id, .. } => {
            assert_eq!(
                canonical_id, "b.ts",
                "WholeHash entry for b.ts must survive"
            );
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn accumulate_dispatch_mixed_signature_converts_project_drops_route() {
    // A realistic dispatch dep-signature: a WholeHash for the keyed
    // canonical plus a ProjectGeneration plus a RouteGeneration. The
    // accumulator keeps the WholeHash + ProjectGeneration and drops
    // only the RouteGeneration.
    let sig = make_dep_sig(vec![
        ("scope.ts", DepVersion::WholeHash([5u8; 16])),
        ("scope.ts", DepVersion::ProjectGeneration(404)),
        ("route.ts", DepVersion::RouteGeneration(1)),
    ]);

    let result = accumulate_dispatch_dep_signature_for_tests(&sig);

    assert_eq!(
        result.len(),
        2,
        "WholeHash + ProjectGeneration survive; RouteGeneration drops"
    );
    assert!(
        result.contains(&FactVersionRef::ProjectGeneration { generation: 404 }),
        "the ProjectGeneration(404) dep must convert"
    );
    assert!(
        result.contains(&FactVersionRef::FileWholeHash {
            canonical_id: "scope.ts".to_string(),
            hash: [5u8; 16],
        }),
        "the WholeHash dep must convert"
    );
    assert!(
        !result.iter().any(
            |f| matches!(f, FactVersionRef::ProjectGeneration { generation } if *generation == 1)
        ),
        "the RouteGeneration(1) must not leak in as a ProjectGeneration"
    );
}

// ----------------------------------------------------------------------------
// observe_fence_entry
// ----------------------------------------------------------------------------

fn finalised_facts(result: FactReadSetFinalise) -> Vec<FactVersionRef> {
    match result {
        FactReadSetFinalise::Ok(facts) => facts.to_vec(),
        FactReadSetFinalise::NonCacheable(_) => panic!("fixture unexpectedly non-cacheable"),
        FactReadSetFinalise::Overflow => panic!("tracer overflowed in a tiny-signature test"),
    }
}

#[test]
fn observe_fence_entry_converts_project_generation() {
    // `observe_fence_entry` fans observations onto the active tracer.
    // A `ProjectGeneration` dep MUST be observed as a
    // `FactVersionRef::ProjectGeneration`. Pre-fix the `WholeHash`-only
    // arm emits nothing for it and the finalized set is empty.
    let host = VerterHost::new_standalone(HostConfig::default());
    let sig = make_dep_sig(vec![("x.ts", DepVersion::ProjectGeneration(123))]);

    let facts = finalised_facts(observe_fence_entry_for_tests(&host, &sig));

    assert_eq!(
        facts.len(),
        1,
        "observe_fence_entry must emit one FactVersionRef for a \
         ProjectGeneration dep, not drop it"
    );
    assert_eq!(
        facts[0],
        FactVersionRef::ProjectGeneration { generation: 123 },
        "ProjectGeneration(123) must be observed as \
         FactVersionRef::ProjectGeneration with generation 123"
    );
}

#[test]
fn observe_fence_entry_drops_route_generation() {
    // `RouteGeneration` has no `FactVersionRef` peer — `observe_fence_entry`
    // emits nothing for it. The sibling `WholeHash` entry is still
    // observed so the finalized set is observably non-empty.
    let host = VerterHost::new_standalone(HostConfig::default());
    let sig = make_dep_sig(vec![
        ("a.ts", DepVersion::RouteGeneration(42)),
        ("b.ts", DepVersion::WholeHash([7u8; 16])),
    ]);

    let facts = finalised_facts(observe_fence_entry_for_tests(&host, &sig));

    assert_eq!(
        facts.len(),
        1,
        "RouteGeneration must be dropped by observe_fence_entry; only \
         the WholeHash observation survives"
    );
    assert!(
        !facts
            .iter()
            .any(|f| matches!(f, FactVersionRef::ProjectGeneration { .. })),
        "no ProjectGeneration fact may be observed for a RouteGeneration dep"
    );
    match &facts[0] {
        FactVersionRef::FileWholeHash { canonical_id, hash } => {
            assert_eq!(canonical_id, "b.ts");
            assert_eq!(*hash, [7u8; 16]);
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn observe_fence_entry_mixed_signature_converts_project_drops_route() {
    // WholeHash + ProjectGeneration + RouteGeneration: the first two
    // are observed onto the tracer, the RouteGeneration is dropped.
    let host = VerterHost::new_standalone(HostConfig::default());
    let sig = make_dep_sig(vec![
        ("scope.ts", DepVersion::WholeHash([1u8; 16])),
        ("scope.ts", DepVersion::ProjectGeneration(900)),
        ("route.ts", DepVersion::RouteGeneration(3)),
    ]);

    let facts = finalised_facts(observe_fence_entry_for_tests(&host, &sig));

    assert!(
        facts.contains(&FactVersionRef::ProjectGeneration { generation: 900 }),
        "observe_fence_entry must observe the ProjectGeneration(900) dep"
    );
    assert!(
        facts.contains(&FactVersionRef::FileWholeHash {
            canonical_id: "scope.ts".to_string(),
            hash: [1u8; 16],
        }),
        "observe_fence_entry must observe the WholeHash dep"
    );
    assert_eq!(
        facts.len(),
        2,
        "WholeHash + ProjectGeneration observed; RouteGeneration dropped"
    );
}
