use std::mem::size_of;

use crate::semantic_query::{
    CanonicalTypeSubstitution, QueryOutcome, Ready, ResultEvaluationContextId,
};

use super::lifetime::SignatureStore;
use super::records::{
    ParameterOptionality, ReturnObligationKey, SignatureDescriptor, SignatureInputShape,
    SignatureKind, SignatureResultRecipe, SignatureSemanticFlags, SignatureSetRef,
    SignatureTemplate, TypeToken, LAYOUT_QUERY_OUTCOME_SET, LAYOUT_READY_SET,
    LAYOUT_SIGNATURE_CANDIDATE, LAYOUT_SIGNATURE_SET_REF,
};
use super::test_support::intern_one_call;

#[test]
fn layouts_are_the_measured_64_bit_sizes() {
    assert_eq!(
        size_of::<super::records::SignatureCandidate>(),
        LAYOUT_SIGNATURE_CANDIDATE
    );
    assert_eq!(size_of::<SignatureSetRef>(), LAYOUT_SIGNATURE_SET_REF);
    assert_eq!(size_of::<Ready<SignatureSetRef>>(), LAYOUT_READY_SET);
    assert_eq!(
        size_of::<QueryOutcome<SignatureSetRef>>(),
        LAYOUT_QUERY_OUTCOME_SET
    );
    let family = crate::semantic_query_memo::family_key_size_for_tests();
    let memo = crate::semantic_query_memo::memo_entry_size_for_tests();
    assert_eq!(family, 136, "hot family key");
    assert!(
        memo >= 64,
        "persisted memo record must occupy a measured envelope, got {memo}"
    );
    eprintln!(
        "signature_kernel layouts: candidate={} set_ref={} ready={} query_outcome={} family_key={} memo_entry={}",
        LAYOUT_SIGNATURE_CANDIDATE,
        LAYOUT_SIGNATURE_SET_REF,
        LAYOUT_READY_SET,
        LAYOUT_QUERY_OUTCOME_SET,
        family,
        memo
    );
}

#[test]
fn same_template_and_normalized_environment_intern_to_one_descriptor() {
    let store = SignatureStore::new();
    let a = intern_one_call(&store);
    let b = intern_one_call(&store);
    let SignatureSetRef::One(ca) = a else {
        panic!("expected One");
    };
    let SignatureSetRef::One(cb) = b else {
        panic!("expected One");
    };
    assert_eq!(ca.signature, cb.signature);
}

#[test]
fn body_locator_difference_does_not_share_a_recipe_while_sharing_the_input_shape() {
    let store = SignatureStore::new();
    let space = store
        .intern_binder_space(
            super::records::BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let layout = store
        .intern_layout(
            super::records::ParameterLayout {
                parameters: Box::from([]),
                rest: None,
            },
            None,
        )
        .unwrap();
    let shape = SignatureInputShape {
        kind: SignatureKind::Call,
        binder_declarations: space,
        this_parameter: None,
        parameter_layout: layout,
        declared_minimum: 0,
        signature_semantic_flags: SignatureSemanticFlags::NONE,
    };
    let shape_id = store.intern_shape(shape, None).unwrap();
    let loc_a = store.intern_body_locator(10, None).unwrap();
    let loc_b = store.intern_body_locator(11, None).unwrap();
    let recipe_a = store
        .intern_recipe(
            SignatureResultRecipe::Body {
                return_obligation_key: ReturnObligationKey {
                    body_locator: loc_a,
                    evaluation: ResultEvaluationContextId::from_raw(0),
                },
            },
            None,
        )
        .unwrap();
    let recipe_b = store
        .intern_recipe(
            SignatureResultRecipe::Body {
                return_obligation_key: ReturnObligationKey {
                    body_locator: loc_b,
                    evaluation: ResultEvaluationContextId::from_raw(0),
                },
            },
            None,
        )
        .unwrap();
    assert_ne!(recipe_a, recipe_b);
    let ta = store
        .intern_template(
            SignatureTemplate {
                input_shape: shape_id,
                result_recipe: recipe_a,
            },
            None,
        )
        .unwrap();
    let tb = store
        .intern_template(
            SignatureTemplate {
                input_shape: shape_id,
                result_recipe: recipe_b,
            },
            None,
        )
        .unwrap();
    assert_ne!(ta, tb);
    let env = store
        .intern_environment(CanonicalTypeSubstitution::empty(), None)
        .unwrap();
    let da = store
        .intern_descriptor(
            SignatureDescriptor {
                template: ta,
                declaration_environment: env,
                residual_binders: space,
            },
            None,
        )
        .unwrap();
    let db = store
        .intern_descriptor(
            SignatureDescriptor {
                template: tb,
                declaration_environment: env,
                residual_binders: space,
            },
            None,
        )
        .unwrap();
    assert_ne!(da, db);
    let view = super::read_view::SemanticReadView::pin(&store);
    assert_eq!(view.descriptor(da).unwrap().template, ta);
    assert_eq!(view.descriptor(db).unwrap().template, tb);
    assert_eq!(
        view.descriptor(da).unwrap().declaration_environment,
        view.descriptor(db).unwrap().declaration_environment
    );
}

#[test]
fn optionality_is_explicit_not_a_missing_span() {
    let required = ParameterOptionality::required();
    let optional = ParameterOptionality::optional_with_undefined();
    assert!(!required.declared_optional && !required.includes_undefined);
    assert!(optional.declared_optional && optional.includes_undefined);
    assert_ne!(required, optional);
    let _ = TypeToken::from_raw(1);
}

#[test]
fn empty_one_many_are_distinct_set_refs() {
    let store = SignatureStore::new();
    let one = intern_one_call(&store);
    let many = store
        .set_ref_many(
            Box::from([match one {
                SignatureSetRef::One(c) => c,
                _ => panic!("one"),
            }]),
            None,
        )
        .unwrap();
    assert_ne!(one, SignatureSetRef::Empty);
    assert_ne!(one, many);
    assert_ne!(many, SignatureSetRef::Empty);
}
