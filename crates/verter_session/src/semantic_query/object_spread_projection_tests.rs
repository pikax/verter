use std::collections::HashSet;
use std::sync::Arc;

use static_assertions::assert_not_impl_any;

use super::object_spread_projection::{
    test_support, ClosedKeyLookup, CompleteObjectDomain, ExactOptionalPropertyPolicy, IndexDomain,
    ObjectProjectionSelector, ObjectSignatureKind, OpenSafeKeyEvidence, PositiveKeyPresence,
    ProjectionEvidence,
};
use super::{
    ProjectionMode, ProjectionReductionContext, PropertyKey, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue, SemanticQueryValueTag, SubstitutionCanonicalHash,
};

fn key(name: &str) -> PropertyKey {
    PropertyKey::identifier(Arc::<str>::from(name))
}

#[test]
fn every_projection_selector_is_distinct_query_identity() {
    let selectors = [
        ObjectProjectionSelector::Key(key("x")),
        ObjectProjectionSelector::RelationShape(Arc::from([key("x"), key("y")])),
        ObjectProjectionSelector::Surface,
        ObjectProjectionSelector::IndexDomain(IndexDomain::String),
        ObjectProjectionSelector::Signature(ObjectSignatureKind::Call),
        ObjectProjectionSelector::EnumerableValueEnvelope(IndexDomain::Number),
        ObjectProjectionSelector::ExcessEligibility,
    ];
    assert_eq!(
        selectors.iter().cloned().collect::<HashSet<_>>().len(),
        selectors.len()
    );

    let context = test_support::context(
        ProjectionReductionContext::published(ProjectionMode::Shallow),
        [1; 16],
        [2; 16],
        [3; 16],
        [4; 16],
        SubstitutionCanonicalHash::distinct_for_test(1),
        ExactOptionalPropertyPolicy::Enabled,
    );
    let keys = selectors.map(|selector| SemanticQueryKey::ProjectObjectSpread {
        program: SemanticNodeId(7),
        selector,
        context,
    });
    assert_eq!(
        keys.iter().cloned().collect::<HashSet<_>>().len(),
        keys.len()
    );
}

#[test]
fn context_retains_every_env_substitution_reduction_and_optional_axis() {
    let base = test_support::context(
        ProjectionReductionContext::published(ProjectionMode::Shallow),
        [1; 16],
        [2; 16],
        [3; 16],
        [4; 16],
        SubstitutionCanonicalHash::distinct_for_test(1),
        ExactOptionalPropertyPolicy::Enabled,
    );
    let mutations = [
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [9; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [9; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [9; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [3; 16],
            [9; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(9),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Disabled,
        ),
    ];
    assert!(mutations.into_iter().all(|mutation| mutation != base));
}

#[test]
fn finite_union_keeps_branch_surfaces_correlated() {
    let formula = test_support::closed_formula([
        test_support::closed_alternative([test_support::positive_key(
            key("left"),
            PositiveKeyPresence::Required,
            ProjectionEvidence::Proven(SemanticNodeId(11)),
        )]),
        test_support::closed_alternative([test_support::positive_key(
            key("right"),
            PositiveKeyPresence::Optional,
            ProjectionEvidence::Proven(SemanticNodeId(22)),
        )]),
    ]);

    assert_eq!(formula.alternatives().len(), 2);
    let complete = formula.closed().expect("every branch is closed");
    let alternatives: Vec<_> = complete.alternatives().collect();
    assert!(matches!(
        alternatives[0].lookup(&key("left")),
        Some(ClosedKeyLookup::Present(fact))
            if *fact.value() == ProjectionEvidence::Proven(SemanticNodeId(11))
    ));
    assert!(matches!(
        alternatives[0].lookup(&key("right")),
        Some(ClosedKeyLookup::AbsentProven)
    ));
    assert!(matches!(
        alternatives[1].lookup(&key("right")),
        Some(ClosedKeyLookup::Present(fact))
            if fact.presence() == PositiveKeyPresence::Optional
    ));
    assert!(
        complete.keyof().is_some_and(|keyof| keyof.is_empty()),
        "`keyof (left-arm | right-arm)` is the exact common-key intersection"
    );
    let surfaces: Vec<_> = complete.surfaces().collect();
    assert_eq!(surfaces.len(), 2);
    assert_eq!(
        surfaces[0].expect("whole-domain surface").members()[0].key(),
        &key("left")
    );
    assert_eq!(
        surfaces[1].expect("whole-domain surface").members()[0].key(),
        &key("right")
    );
}

#[test]
fn open_alternative_exposes_only_positive_or_indeterminate_key_evidence() {
    let residual = SemanticNodeId(99);
    let second_residual = SemanticNodeId(100);
    let formula = test_support::open_formula(
        [test_support::positive_key(
            key("known"),
            PositiveKeyPresence::Required,
            ProjectionEvidence::Indeterminate,
        )],
        [residual, second_residual],
    );
    assert_eq!(
        formula.alternatives().len(),
        1,
        "generic/non-enumerable operands remain residual evidence, never relation arms"
    );
    let alternative = &formula.alternatives()[0];

    assert!(matches!(
        alternative.selected_key(&key("known")),
        OpenSafeKeyEvidence::Positive(fact)
            if fact.presence() == PositiveKeyPresence::Required
    ));
    assert!(matches!(
        alternative.selected_key(&key("possible")),
        OpenSafeKeyEvidence::UnknownOnOpenDomain { residual_operands }
            if residual_operands == [residual, second_residual]
    ));
    assert!(alternative.closed().is_none());
    assert!(formula.closed().is_none());
}

#[test]
fn open_alternative_reports_indeterminate_possible_writes_without_absence() {
    let formula = test_support::open_formula_with_possible_writes(
        [],
        [SemanticNodeId(101)],
        [key("possible")],
    );
    assert!(matches!(
        formula.alternatives()[0].selected_key(&key("possible")),
        OpenSafeKeyEvidence::IndeterminatePossibleWrite
    ));
}

#[test]
fn mixed_formula_has_only_branch_local_closed_witnesses() {
    let formula = test_support::formula([
        test_support::closed_alternative([test_support::positive_key(
            key("closed"),
            PositiveKeyPresence::Required,
            ProjectionEvidence::Proven(SemanticNodeId(102)),
        )]),
        test_support::open_alternative([], [SemanticNodeId(103)]),
    ]);
    assert!(formula.alternatives()[0].closed().is_some());
    assert!(formula.alternatives()[1].closed().is_none());
    assert!(formula.closed().is_none());
}

#[test]
fn exact_operations_are_witness_only() {
    use super::object_spread_projection::{
        ObjectProjectionAlternative, ObjectProjectionFormula, PositiveAlternativeEvidence,
    };

    assert_not_impl_any!(ObjectProjectionFormula: CompleteObjectDomain);
    assert_not_impl_any!(ObjectProjectionAlternative: CompleteObjectDomain);
    assert_not_impl_any!(PositiveAlternativeEvidence<'static>: CompleteObjectDomain);
}

#[test]
fn object_projection_has_a_dedicated_value_domain() {
    let value = SemanticQueryValue::ObjectProjection(test_support::closed_formula([
        test_support::closed_alternative([]),
    ]));
    assert_eq!(value.tag(), SemanticQueryValueTag::ObjectProjection);
}
