use super::*;
use crate::facts::receipt::ResultReceipt;
use crate::facts::resolution::{
    CanonicalResolutionId, ResolutionFactKey, ResolutionFactRef, ResolutionFactVersion,
};
use crate::facts::version::{
    AggregatePopulation, AggregateStamp, CompactionDomain, DomainGenerationFact, OverlayId,
    RequestCompletion, ResolutionRootsStamp, ResolveImportsFactRef, SemanticImportsStamp,
    ViewPopulation, ViewPopulationParent,
};
use crate::resolution::resolution_world_identity::{
    ResolutionPopulation, ResolutionWorldId, SessionFingerprint,
};

const HOST: NativeAuthority = NativeAuthority::new(7);

fn whole(canonical: &str, byte: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.into(),
        hash: [byte; 16],
    }
}

fn probe(canonical: &str, population: ResolutionPopulation, version: u64) -> FactVersionRef {
    FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(ResolutionFactRef::new(
        ResolutionFactKey::PathProbe {
            canonical: CanonicalResolutionId::new(canonical),
            population,
        },
        ResolutionFactVersion::fresh(version),
    )))
}

fn observe(fact: &FactVersionRef, authority: NativeAuthority) -> NativeObservation {
    fact.native_observation(authority).expect("leaf encodes")
}

/// A recorded absent probe is observed through its producer-committed key
/// and version. Its population is part of the key, and the version is a
/// host-namespaced revision: the base and a session's probe of one path
/// never alias, and two hosts' equal counters never alias either.
#[test]
fn native_encoding_preserves_negative_probe_population() {
    let base = observe(&probe("/p/missing.ts", ResolutionPopulation::Base, 3), HOST);
    let session = observe(
        &probe(
            "/p/missing.ts",
            ResolutionPopulation::Session(SessionFingerprint::test_only(9)),
            3,
        ),
        HOST,
    );
    assert_ne!(base.key(), session.key(), "population is part of the key");

    let moved = observe(&probe("/p/missing.ts", ResolutionPopulation::Base, 4), HOST);
    assert_eq!(
        base.key(),
        moved.key(),
        "a re-probe is the same observation"
    );
    assert_ne!(
        base.revision(),
        moved.revision(),
        "the version is the revision"
    );

    let other_host = observe(
        &probe("/p/missing.ts", ResolutionPopulation::Base, 3),
        NativeAuthority::new(8),
    );
    assert_eq!(base.key(), other_host.key());
    assert_ne!(
        base.revision(),
        other_host.revision(),
        "a counter revision is namespaced by its host authority"
    );

    let other_family = observe(
        &FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(ResolutionFactRef::new(
            ResolutionFactKey::Manifest {
                canonical: CanonicalResolutionId::new("/p/missing.ts"),
                population: ResolutionPopulation::Base,
            },
            ResolutionFactVersion::fresh(3),
        ))),
        HOST,
    );
    assert_ne!(base.key(), other_family.key(), "the fact family is tagged");
}

/// Receipts are expanded to their leaves: the same evidence grouped under
/// different receipts, or not grouped at all, commits the same
/// observations, each leaf once.
#[test]
fn receipt_grouping_does_not_change_native_observations() {
    let a = whole("/a.ts", 1);
    let b = whole("/b.ts", 2);
    let c = probe("/c.ts", ResolutionPopulation::Base, 5);
    let d = FactVersionRef::ProjectGeneration { generation: 11 };

    let flat = native_observations([&a, &b, &c, &d], HOST).expect("flat");
    let inner = ResultReceipt::new(vec![d.clone(), a.clone()]);
    let grouped = [
        FactVersionRef::Receipt(ResultReceipt::new(vec![a.clone(), b.clone()])),
        FactVersionRef::Receipt(ResultReceipt::new(vec![
            c.clone(),
            FactVersionRef::Receipt(inner),
        ])),
    ];
    let nested = native_observations(grouped.iter(), HOST).expect("grouped");
    let single = FactVersionRef::Receipt(ResultReceipt::new(vec![a, b, c, d]));
    let whole_receipt = native_observations([&single], HOST).expect("single");

    assert_eq!(flat.len(), 4, "every distinct leaf once");
    assert_eq!(flat, nested);
    assert_eq!(flat, whole_receipt);
}

/// A terminal domain aggregate keeps its complete population and stamp:
/// the domain and population name the observation, the whole stamp is its
/// revision.
#[test]
fn native_encoding_preserves_aggregate_stamps() {
    let aggregate = |population: ViewPopulation, stamp: AggregateStamp| {
        observe(
            &FactVersionRef::DomainGeneration(DomainGenerationFact {
                domain: CompactionDomain::SemanticImports,
                population: AggregatePopulation::View(population),
                stamp,
            }),
            HOST,
        )
    };
    let stamp = |session: Option<u64>, workspace_shape: u64| {
        AggregateStamp::SemanticImports(SemanticImportsStamp {
            semantic_imports: 1,
            content: 2,
            source_env: 3,
            resolution: ResolutionRootsStamp {
                base: ResolutionWorldId::test_only(4),
                session: session.map(ResolutionWorldId::test_only),
            },
            workspace_shape,
        })
    };
    let base = aggregate(ViewPopulation::Base, stamp(None, 5));
    let session_root = aggregate(ViewPopulation::Base, stamp(Some(6), 5));
    let shape = aggregate(ViewPopulation::Base, stamp(None, 6));
    assert_eq!(base.key(), session_root.key());
    assert_ne!(
        base.revision(),
        session_root.revision(),
        "every root counts"
    );
    assert_ne!(base.revision(), shape.revision(), "every stamp axis counts");

    let overlay = OverlayId::fresh();
    let completion = |revision: u64| {
        ViewPopulation::RequestCompletion(RequestCompletion {
            parent: ViewPopulationParent::Base,
            overlay_id: overlay,
            revision,
        })
    };
    let first = aggregate(completion(1), stamp(None, 5));
    let second = aggregate(completion(2), stamp(None, 5));
    assert_ne!(base.key(), first.key(), "the overlay population is the key");
    assert_ne!(first.key(), second.key(), "the overlay revision is the key");

    let resolution_domain = observe(
        &FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Resolution,
            population: AggregatePopulation::Resolution(ResolutionPopulation::Base),
            stamp: AggregateStamp::Generation(5),
        }),
        HOST,
    );
    let content_domain = observe(
        &FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Content,
            population: AggregatePopulation::View(ViewPopulation::Base),
            stamp: AggregateStamp::Generation(5),
        }),
        HOST,
    );
    assert_ne!(resolution_domain.key(), content_domain.key());
}

/// A terminal witness scoped to one request's completion overlay is never
/// durable evidence; the same witness in a durable population is.
#[test]
fn request_completion_witnesses_are_recognised() {
    let completion = ViewPopulation::RequestCompletion(RequestCompletion {
        parent: ViewPopulationParent::Base,
        overlay_id: OverlayId::fresh(),
        revision: 1,
    });
    let aggregate = |population| {
        FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Content,
            population: AggregatePopulation::View(population),
            stamp: AggregateStamp::Generation(3),
        })
    };
    let world = |population| {
        FactVersionRef::StrictSelfRootWorld(crate::facts::version::StrictSelfRootWorld {
            authority_id: 1,
            authority_generation: 2,
            source_epoch: 3,
            artifact_epoch: 4,
            population,
        })
    };
    assert!(aggregate(completion).is_request_completion_witness());
    assert!(world(completion).is_request_completion_witness());
    assert!(!aggregate(ViewPopulation::Base).is_request_completion_witness());
    assert!(!world(ViewPopulation::Base).is_request_completion_witness());
    assert!(!whole("/a.ts", 1).is_request_completion_witness());
}
