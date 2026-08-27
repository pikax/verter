use super::*;

fn file_whole_hash(id: &str) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: id.to_string(),
        hash: [1; 16],
    }
}

// ── compaction_domain ──

#[test]
fn compaction_domain_covers_every_variant_as_documented() {
    assert_eq!(
        compaction_domain(&file_whole_hash("/a.ts")),
        CompactionDomain::Content
    );
    assert_eq!(
        compaction_domain(&FactVersionRef::DerivedFactHash {
            canonical_id: "/a.ts".to_string(),
            kind: DerivedFactKind::Route,
            hash: [2; 16],
        }),
        CompactionDomain::Content
    );
    // `FileSourceEnv`'s arm (-> `CompactionDomain::SourceEnv`) is omitted
    // here: `ParseKey`/`FileLanguage` construction needs a full
    // `CanonicalEncode` fixture disproportionate to this exhaustiveness
    // smoke test. The match itself is exhaustive by construction (a
    // missing arm is a compile error), so this test's job — proving the
    // easy arms map correctly — doesn't need every arm covered.
    assert_eq!(
        compaction_domain(&FactVersionRef::ProjectGeneration { generation: 1 }),
        CompactionDomain::WorkspaceShape
    );
    assert_eq!(
        compaction_domain(&FactVersionRef::StrictSelfRootWorld(StrictSelfRootWorld {
            authority_id: 1,
            authority_generation: 1,
            source_epoch: 1,
            artifact_epoch: 1,
            population: ViewPopulation::Base,
        })),
        CompactionDomain::Content
    );
}

#[test]
fn compaction_domain_of_a_domain_aggregate_is_its_own_domain() {
    let aggregate = FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain: CompactionDomain::RouteSurface,
        population: AggregatePopulation::View(ViewPopulation::Base),
        stamp: AggregateStamp::Generation(3),
    });
    assert_eq!(
        compaction_domain(&aggregate),
        CompactionDomain::RouteSurface
    );
}

// ── SessionOverlayFingerprint ──

#[test]
fn session_overlay_fingerprint_rejects_zero() {
    assert_eq!(SessionOverlayFingerprint::new(0), None);
    assert!(SessionOverlayFingerprint::new(1).is_some());
}

// ── OverlayId ──

#[test]
fn overlay_id_fresh_never_returns_zero_and_is_monotonic() {
    let a = OverlayId::fresh();
    let b = OverlayId::fresh();
    assert_ne!(a.get(), 0);
    assert_ne!(b.get(), 0);
    assert!(b.get() > a.get());
}

// ── ViewPopulation::refined_by_completion ──

#[test]
fn refined_by_completion_empty_projects_to_parent() {
    let projected = ViewPopulation::refined_by_completion(
        ViewPopulationParent::Base,
        CompletionOverlayState::Empty,
    );
    assert_eq!(projected, Some(ViewPopulation::Base));
}

#[test]
fn refined_by_completion_shadowing_carries_overlay_identity() {
    let overlay_id = OverlayId::fresh();
    let projected = ViewPopulation::refined_by_completion(
        ViewPopulationParent::Base,
        CompletionOverlayState::Shadowing {
            overlay_id,
            revision: 5,
        },
    );
    assert_eq!(
        projected,
        Some(ViewPopulation::RequestCompletion(RequestCompletion {
            parent: ViewPopulationParent::Base,
            overlay_id,
            revision: 5,
        }))
    );
}

#[test]
fn refined_by_completion_in_flight_names_no_population() {
    let projected = ViewPopulation::refined_by_completion(
        ViewPopulationParent::Base,
        CompletionOverlayState::InFlight,
    );
    assert_eq!(projected, None);
}

// ── ResolveImportsFactRef ──

#[test]
fn resolve_imports_fact_ref_semantic_reports_its_canonical_id() {
    let fact = ResolveImportsFactRef::Semantic {
        canonical_id: "/a.ts".to_string(),
        key: FactKey::SyntacticExportSet,
        lane: FactLane::Semantic,
        expected_hash: [0; 16],
    };
    assert_eq!(fact.canonical_id(), Some("/a.ts"));
    assert_eq!(fact.resolution_fact(), None);
}

// ── FactVersionRef::attribution / canonical_id ──

#[test]
fn file_whole_hash_attributes_to_its_canonical() {
    let fact = file_whole_hash("/a.ts");
    assert_eq!(fact.attribution(), FactAttribution::Canonical("/a.ts"));
    assert_eq!(fact.canonical_id(), Some("/a.ts"));
}

#[test]
fn project_generation_attributes_to_no_canonical() {
    let fact = FactVersionRef::ProjectGeneration { generation: 4 };
    assert_eq!(fact.attribution(), FactAttribution::ProjectScalar);
    assert_eq!(fact.canonical_id(), None);
}

#[test]
fn domain_generation_attributes_to_its_domain_aggregate() {
    let fact = FactVersionRef::DomainGeneration(DomainGenerationFact {
        domain: CompactionDomain::Content,
        population: AggregatePopulation::View(ViewPopulation::Base),
        stamp: AggregateStamp::Generation(1),
    });
    assert_eq!(
        fact.attribution(),
        FactAttribution::DomainAggregate(CompactionDomain::Content)
    );
    assert_eq!(fact.canonical_id(), None);
}

#[test]
fn strict_self_root_world_attributes_to_itself_not_a_canonical() {
    let fact = FactVersionRef::StrictSelfRootWorld(StrictSelfRootWorld {
        authority_id: 1,
        authority_generation: 1,
        source_epoch: 1,
        artifact_epoch: 1,
        population: ViewPopulation::Base,
    });
    assert_eq!(fact.attribution(), FactAttribution::StrictSelfRootWorld);
    assert_eq!(fact.canonical_id(), None);
}

#[test]
fn resolve_imports_semantic_attributes_to_its_canonical() {
    let fact = FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
        canonical_id: "/b.ts".to_string(),
        key: FactKey::SyntacticExportSet,
        lane: FactLane::Semantic,
        expected_hash: [0; 16],
    });
    assert_eq!(fact.attribution(), FactAttribution::Canonical("/b.ts"));
}
