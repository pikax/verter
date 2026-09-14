//! The dispatch-signature bridge `dep_signature_to_fact_signature` must
//! convert `DepVersion::ProjectGeneration` into a
//! `FactVersionRef::ProjectGeneration`.
//!
//! `ProjectGeneration` carries an authoritative validating fact: the
//! project-wide generation a sub-result depended on, validated by
//! `StoreView::validates`'s `ProjectGeneration` arm. A
//! fact-only consumer fed through this bridge that never
//! sees the `ProjectGeneration` fact cannot detect a project-shape
//! change (`tsconfig`, path-alias, SDK, workspace-folder edit) — it
//! would validate a stale result against a superseded project shape.
//!
//! `RouteGeneration` is the opposite: there is no
//! `FactVersionRef::RouteGeneration` variant and no authoritative
//! validating source, so it is the sole dropped variant — a defensive
//! floor. No production path constructs `DepVersion::RouteGeneration`.
//!
//! The assertions discriminate `ProjectGeneration` conversion from the
//! deliberately unsupported `RouteGeneration` mapping.

use std::sync::Arc;

use verter_session::for_tests::dispatch_dep_signature_facts_for_tests;
use verter_session::resolver_core::FactVersionRef;
use verter_session::semantic_query::{DepSignature, DepVersion};

fn make_dep_sig(entries: Vec<(&str, DepVersion)>) -> DepSignature {
    Arc::from(
        entries
            .into_iter()
            .map(|(canon, ver)| (Arc::from(canon), ver))
            .collect::<Vec<_>>(),
    )
}

// ----------------------------------------------------------------------------
// Dispatch signature bridge
// ----------------------------------------------------------------------------

#[test]
fn dispatch_bridge_converts_project_generation() {
    // A `ProjectGeneration` dep MUST land in the accumulator as a
    // `FactVersionRef::ProjectGeneration` carrying the same
    // generation. Pre-fix the joint `RouteGeneration | ProjectGeneration`
    // arm drops it and the accumulator is empty.
    let sig = make_dep_sig(vec![("x.ts", DepVersion::ProjectGeneration(77))]);

    let result = dispatch_dep_signature_facts_for_tests(&sig);

    assert_eq!(
        result.len(),
        1,
        "ProjectGeneration must convert to one FactVersionRef in the \
         dispatch signature bridge, not be dropped"
    );
    assert_eq!(
        result[0],
        FactVersionRef::ProjectGeneration { generation: 77 },
        "ProjectGeneration(77) must convert to \
         FactVersionRef::ProjectGeneration with generation 77"
    );
}

#[test]
fn dispatch_bridge_drops_route_generation() {
    // `RouteGeneration` has no `FactVersionRef` peer — the dispatch
    // bridge drops it (defensive floor). The sibling `WholeHash`
    // entry still converts so the call is observably non-empty.
    let sig = make_dep_sig(vec![
        ("a.ts", DepVersion::RouteGeneration(9)),
        ("b.ts", DepVersion::WholeHash([3u8; 16])),
    ]);

    let result = dispatch_dep_signature_facts_for_tests(&sig);

    assert_eq!(
        result.len(),
        1,
        "RouteGeneration must be dropped by the dispatch bridge; \
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
fn dispatch_bridge_mixed_signature_converts_project_drops_route() {
    // A realistic dispatch dep-signature: a WholeHash for the keyed
    // canonical plus a ProjectGeneration plus a RouteGeneration. The
    // bridge keeps the WholeHash + ProjectGeneration and drops
    // only the RouteGeneration.
    let sig = make_dep_sig(vec![
        ("scope.ts", DepVersion::WholeHash([5u8; 16])),
        ("scope.ts", DepVersion::ProjectGeneration(404)),
        ("route.ts", DepVersion::RouteGeneration(1)),
    ]);

    let result = dispatch_dep_signature_facts_for_tests(&sig);

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
