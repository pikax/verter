//! `AttemptOutput`'s own unit tests — the accumulator is inert (not yet
//! wired into `AttemptOutcome`/`ResolverObservation`), so these test the
//! accumulator API directly: record/read/merge, and that a fresh
//! accumulator is empty (the "discarded on `NeedInputs`/`Terminal`"
//! semantics the eventual driver relies on — a fresh accumulator IS the
//! discarded state, trivially).

use super::{AmbientDependency, AttemptOutput, ConsumedResolutionObservationKey};
use std::sync::Arc;

fn fact(byte: u8) -> crate::facts::version::FactVersionRef {
    crate::facts::version::FactVersionRef::FileWholeHash {
        canonical_id: format!("/ws/{byte}.ts"),
        hash: [byte; 16],
    }
}

#[test]
fn new_accumulator_is_empty() {
    let output = AttemptOutput::new();
    assert!(output.is_empty());
    assert_eq!(output.observed_facts(), &[]);
    assert_eq!(output.ambient_dependencies(), &[]);
    assert_eq!(output.consumed_resolution_observations(), &[]);
}

#[test]
fn default_accumulator_is_empty() {
    // Discriminates: `Default` must produce the SAME empty state as
    // `new()`, not a differently-initialized accumulator -- both are
    // supposed to be interchangeable "fresh, one per attempt" starting
    // points.
    assert_eq!(AttemptOutput::default(), AttemptOutput::new());
}

#[test]
fn record_fact_accumulates_in_order() {
    let mut output = AttemptOutput::new();
    output.record_fact(fact(1)).expect("within default budget");
    output.record_fact(fact(2)).expect("within default budget");

    assert_eq!(output.observed_facts(), &[fact(1), fact(2)]);
    assert!(!output.is_empty());
}

#[test]
fn record_ambient_dependency_accumulates_in_order() {
    let mut output = AttemptOutput::new();
    output
        .record_ambient_dependency(Arc::from("/ws/a.ts"), Arc::from("virtual:/a"))
        .expect("within default budget");
    output
        .record_ambient_dependency(Arc::from("/ws/b.ts"), Arc::from("virtual:/b"))
        .expect("within default budget");

    assert_eq!(
        output.ambient_dependencies(),
        &[
            AmbientDependency {
                consumer_canonical: Arc::from("/ws/a.ts"),
                virtual_id: Arc::from("virtual:/a"),
            },
            AmbientDependency {
                consumer_canonical: Arc::from("/ws/b.ts"),
                virtual_id: Arc::from("virtual:/b"),
            },
        ]
    );
}

#[test]
fn record_consumed_resolution_observation_accumulates_in_order() {
    let mut output = AttemptOutput::new();
    output
        .record_consumed_resolution_observation(ConsumedResolutionObservationKey::PathProbe {
            path: Arc::from("/ws/a.ts"),
        })
        .expect("within default budget");
    output
        .record_consumed_resolution_observation(ConsumedResolutionObservationKey::PackageManifest {
            directory: Arc::from("/ws/node_modules/pkg"),
        })
        .expect("within default budget");

    assert_eq!(
        output.consumed_resolution_observations(),
        &[
            ConsumedResolutionObservationKey::PathProbe {
                path: Arc::from("/ws/a.ts"),
            },
            ConsumedResolutionObservationKey::PackageManifest {
                directory: Arc::from("/ws/node_modules/pkg"),
            },
        ]
    );
}

/// `RecoveryScope` is a distinct consumed-observation kind (mirroring
/// `verter_workspace::resolution_currency::
/// ResolutionFactKey::RecoveryScope`), not folded into or confused with
/// `PathProbe` -- an accumulator that collapsed the two would lose the
/// ancestor-directory recovery fact `resolution_witness_contract_tests.rs`
/// proves is independently required.
#[test]
fn record_consumed_resolution_observation_covers_recovery_scope() {
    let mut output = AttemptOutput::new();
    output
        .record_consumed_resolution_observation(ConsumedResolutionObservationKey::RealPath {
            path: Arc::from("/ws/mod.tsx"),
        })
        .expect("within default budget");
    output
        .record_consumed_resolution_observation(ConsumedResolutionObservationKey::RecoveryScope {
            canonical_prefix: Arc::from("/ws"),
        })
        .expect("within default budget");

    assert_eq!(
        output.consumed_resolution_observations(),
        &[
            ConsumedResolutionObservationKey::RealPath {
                path: Arc::from("/ws/mod.tsx"),
            },
            ConsumedResolutionObservationKey::RecoveryScope {
                canonical_prefix: Arc::from("/ws"),
            },
        ]
    );
}

#[test]
fn merge_appends_every_category_and_preserves_order() {
    let mut parent = AttemptOutput::new();
    parent.record_fact(fact(1)).expect("within default budget");
    parent
        .record_ambient_dependency(Arc::from("/ws/parent.ts"), Arc::from("virtual:/parent"))
        .expect("within default budget");

    let mut child = AttemptOutput::new();
    child.record_fact(fact(2)).expect("within default budget");
    child
        .record_consumed_resolution_observation(ConsumedResolutionObservationKey::RealPath {
            path: Arc::from("/ws/child.ts"),
        })
        .expect("within default budget");

    parent.merge(child).expect("within default budget");

    // Discriminates: a merge that dropped either side's contributions, or
    // that reordered facts across the parent/child boundary, would
    // silently lose or misattribute output a sub-attempt (e.g. one
    // project-reference-walk node) produced.
    assert_eq!(parent.observed_facts(), &[fact(1), fact(2)]);
    assert_eq!(
        parent.ambient_dependencies(),
        &[AmbientDependency {
            consumer_canonical: Arc::from("/ws/parent.ts"),
            virtual_id: Arc::from("virtual:/parent"),
        }]
    );
    assert_eq!(
        parent.consumed_resolution_observations(),
        &[ConsumedResolutionObservationKey::RealPath {
            path: Arc::from("/ws/child.ts"),
        }]
    );
    assert!(!parent.is_empty());
}

#[test]
fn merging_two_empty_accumulators_stays_empty() {
    let mut a = AttemptOutput::new();
    let b = AttemptOutput::new();
    a.merge(b).expect("empty merge cannot fail");
    assert!(a.is_empty());
}

#[test]
fn completed_witness_retention_is_inclusive_tagged_and_operation_wide() {
    let budgets = crate::resolver_core::InputResolutionBudgets::try_tightened_with_retention(
        8, 8, 128, 4, 2, 2, 2,
    )
    .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    retention.scope(|| {
        let mut first = AttemptOutput::new();
        first.record_fact(fact(1)).expect("first distinct witness");
        first.record_fact(fact(1)).expect("duplicate charges zero");
        let mut nested = AttemptOutput::new();
        nested
            .record_ambient_dependency(Arc::from("/ws/a.ts"), Arc::from("virtual:/a"))
            .expect("inclusive maximum is admitted");
        assert_eq!(retention.retained_for_test().2, 2);

        let failure = nested
            .record_consumed_resolution_observation(ConsumedResolutionObservationKey::PathProbe {
                path: Arc::from("/ws/a.ts"),
            })
            .expect_err("the next tagged witness breaches the whole-operation maximum");
        assert_eq!(
            failure,
            crate::resolver_core::AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                retained: 2,
                prospective: 3,
                maximum: 2,
            }
        );
        assert_eq!(nested.consumed_resolution_observations(), &[]);
        drop(first);
        nested
            .record_consumed_resolution_observation(ConsumedResolutionObservationKey::PathProbe {
                path: Arc::from("/ws/a.ts"),
            })
            .expect("discarding retained state releases its charge");
    });
    assert_eq!(retention.retained_for_test().2, 0);

    let fresh = crate::resolver_core::InputResolutionRetention::new(budgets);
    fresh.scope(|| {
        let mut output = AttemptOutput::new();
        output
            .record_fact(fact(9))
            .expect("independent operation starts clean");
    });
}

#[test]
fn completed_witness_retention_checked_overflow_is_typed_and_recoverable() {
    let budgets = crate::resolver_core::InputResolutionBudgets::try_tightened_with_retention(
        8, 8, 128, 4, 2, 2, 2,
    )
    .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    retention.force_completed_witness_retained_for_test(Some(u32::MAX));
    retention.scope(|| {
        let mut output = AttemptOutput::new();
        assert_eq!(
            output
                .record_fact(fact(1))
                .expect_err("checked prospective arithmetic must reject overflow"),
            crate::resolver_core::AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                retained: u32::MAX,
                prospective: u32::MAX,
                maximum: 2,
            }
        );
        assert!(output.is_empty(), "the rejected witness cannot be retained");
    });

    retention.force_completed_witness_retained_for_test(None);
    retention.scope(|| {
        let mut clean = AttemptOutput::new();
        clean
            .record_fact(fact(2))
            .expect("the clean independent operation admits its first witness");
    });

    // Mutation recipe: replace `checked_add(1)` in
    // `retain_completed_witness` with wrapping arithmetic. The exact terminal
    // assertion turns RED; restoring it returns GREEN.
}

#[test]
fn alias_geometry_retention_uses_checked_inclusive_live_charges() {
    let budgets = crate::resolver_core::InputResolutionBudgets::try_tightened_with_retention(
        8, 8, 128, 4, 2, 2, 2,
    )
    .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    let first = retention
        .retain_alias_geometry()
        .expect("first live bundle");
    let second = retention
        .retain_alias_geometry()
        .expect("inclusive maximum");
    assert_eq!(retention.retained_for_test(), (2, 2, 0));
    assert_eq!(
        retention
            .retain_alias_geometry()
            .expect_err("max plus one is terminal"),
        crate::resolver_core::AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
            retained: 2,
            prospective: 3,
            maximum: 2,
        }
    );
    drop(second);
    drop(first);
    retention.force_alias_retained_for_test(u32::MAX);
    assert_eq!(
        retention
            .retain_alias_geometry()
            .expect_err("checked arithmetic overflow is terminal"),
        crate::resolver_core::AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
            retained: u32::MAX,
            prospective: u32::MAX,
            maximum: 2,
        }
    );
    retention.force_alias_retained_for_test(0);
}
