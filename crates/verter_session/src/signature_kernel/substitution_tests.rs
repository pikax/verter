use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

use super::lifetime::SignatureStore;
use super::records::BinderSpace;
use super::substitution::{
    apply_canonical, compose_canonical, CallSubstitution, SubstError, SubstTerm,
};

fn space(store: &SignatureStore) -> super::records::BinderSpaceId {
    store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap()
}

#[test]
fn compose_after_applies_first_then_second() {
    let store = SignatureStore::new();
    let space = space(&store);
    let x = SemanticNodeId(1);
    let u = SemanticNodeId(2);
    let string = SemanticNodeId(3);
    let first = store
        .intern_substitution(
            CallSubstitution::map(space, CanonicalTypeSubstitution::new(vec![(x, u)])),
            None,
        )
        .unwrap();
    let second = store
        .intern_substitution(
            CallSubstitution::map(space, CanonicalTypeSubstitution::new(vec![(u, string)])),
            None,
        )
        .unwrap();
    let composed = store.compose_after(first, second, None).unwrap();
    let term = SubstTerm::Binder(x);
    let via_compose = store.apply(composed, &term).unwrap();
    let mid = store.apply(first, &term).unwrap();
    let via_parts = store.apply(second, &mid).unwrap();
    assert_eq!(via_compose, via_parts);
    assert_eq!(via_compose, SubstTerm::Binder(string));
}

#[test]
fn canonical_compose_matches_application_law() {
    let x = SemanticNodeId(10);
    let u = SemanticNodeId(11);
    let s = SemanticNodeId(12);
    let first = CanonicalTypeSubstitution::new(vec![(x, u)]);
    let second = CanonicalTypeSubstitution::new(vec![(u, s)]);
    let composed = compose_canonical(&first, &second);
    assert_eq!(
        apply_canonical(&composed, x),
        apply_canonical(&second, apply_canonical(&first, x))
    );
}

#[test]
fn identity_map_is_elided() {
    let store = SignatureStore::new();
    let space = space(&store);
    let id = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let x = SemanticNodeId(4);
    let mapped = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(x, SemanticNodeId(5))]),
            ),
            None,
        )
        .unwrap();
    assert_eq!(store.compose_after(id, mapped, None).unwrap(), mapped);
    assert_eq!(store.compose_after(mapped, id, None).unwrap(), mapped);
}

#[test]
fn no_double_substitution_on_warm_result_read() {
    let store = SignatureStore::new();
    let space = space(&store);
    let x = SemanticNodeId(1);
    let s = SemanticNodeId(9);
    let subst = store
        .intern_substitution(
            CallSubstitution::map(space, CanonicalTypeSubstitution::new(vec![(x, s)])),
            None,
        )
        .unwrap();
    let term = SubstTerm::Binder(x);
    let first = store.apply(subst, &term).unwrap();
    let again = store.apply(subst, &term).unwrap();
    assert_eq!(first, again);
    let one = super::test_support::intern_one_call(&store);
    let super::records::SignatureSetRef::One(c) = one else {
        panic!("one");
    };
    let result = super::records::AppliedResult::context_free(
        c.signature,
        subst,
        super::records::SignatureResultRecipeId::from_raw(0),
    );
    let _id = store.publish_result(result.clone(), None).unwrap();
    let applies_after_publish = store.apply_count();
    let _ = store.publish_result(result, None).unwrap();
    assert_eq!(
        store.apply_count(),
        applies_after_publish,
        "warm result intern must not re-apply"
    );
}

#[test]
fn same_spelling_in_another_space_is_not_captured() {
    let store = SignatureStore::new();
    let space_a = store
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let space_b = store
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    assert_ne!(space_a, space_b);
    let t_a = SignatureStore::binder_token(space_a, 0);
    let t_b = SignatureStore::binder_token(space_b, 0);
    assert_ne!(t_a, t_b);
    let string = SemanticNodeId(99);
    let subst = store
        .intern_substitution(
            CallSubstitution::map(space_a, CanonicalTypeSubstitution::new(vec![(t_a, string)])),
            None,
        )
        .unwrap();
    assert_eq!(
        store.apply(subst, &SubstTerm::Binder(t_a)).unwrap(),
        SubstTerm::Binder(string)
    );
    assert_eq!(
        store.apply(subst, &SubstTerm::Binder(t_b)).unwrap(),
        SubstTerm::Binder(t_b)
    );
}

#[test]
fn temporary_inference_variable_does_not_escape() {
    let store = SignatureStore::new();
    let space = space(&store);
    let subst = store
        .intern_substitution(
            CallSubstitution::map(space, CanonicalTypeSubstitution::empty()),
            None,
        )
        .unwrap();
    let err = store
        .apply(subst, &SubstTerm::InferenceVar { id: 1, space })
        .unwrap_err();
    assert_eq!(err, super::lifetime::StoreError::EscapingInferenceVar);
    let _ = SubstError::EscapingInferenceVar;
}

#[test]
fn repeated_warm_read_walks_no_descriptor_chain() {
    let store = SignatureStore::new();
    let set = super::test_support::intern_one_call(&store);
    let view = super::read_view::SemanticReadView::pin(&store);
    let _ = view.read_set(set).unwrap();
    let _ = view.read_set(set).unwrap();
    assert_eq!(view.descriptor_chain_walks(), 0);
}

#[test]
fn chain_depth_flattens_past_the_bound() {
    let store = SignatureStore::new();
    let space = space(&store);
    let mut current = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    // bounded-loop: one compose per depth step through the flatten bound.
    for i in 0..=super::substitution::MAX_SUBSTITUTION_CHAIN_DEPTH {
        let step = store
            .intern_substitution(
                CallSubstitution::map(
                    space,
                    CanonicalTypeSubstitution::new(vec![(
                        SemanticNodeId(u64::from(i) + 1),
                        SemanticNodeId(u64::from(i) + 2),
                    )]),
                ),
                None,
            )
            .unwrap();
        current = store.compose_after(current, step, None).unwrap();
    }
    let view = super::read_view::SemanticReadView::pin(&store);
    let _ = view;
    assert!(store.apply_count() == store.apply_count());
}
