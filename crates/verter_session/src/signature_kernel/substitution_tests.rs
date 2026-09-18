use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

use super::lifetime::SignatureStore;
use super::records::BinderSpace;
use super::substitution::{
    apply_canonical, compose_canonical, CallSubstitution, SubstError, SubstTerm,
    MAX_SUBSTITUTION_CHAIN_DEPTH,
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
    let locator = store.intern_body_locator(7, None).unwrap();
    let recipe = store
        .intern_recipe(
            super::records::SignatureResultRecipe::Body {
                return_obligation_key: super::records::ReturnObligationKey {
                    body_locator: locator,
                    evaluation: crate::semantic_query::ResultEvaluationContextId::from_raw(0),
                },
            },
            None,
        )
        .unwrap();
    let result = super::records::AppliedResult::context_free(c.signature, subst, recipe);
    let id = store.publish_result(result.clone(), None).unwrap();
    let applies_after_publish = store.apply_count();
    let walks_after_publish = store.descriptor_chain_walks();
    assert_eq!(store.lookup_result(&result).unwrap(), Some(id));
    assert_eq!(store.lookup_result(&result).unwrap(), Some(id));
    assert_eq!(
        store.apply_count(),
        applies_after_publish,
        "warm result lookup must not re-apply"
    );
    assert_eq!(
        store.descriptor_chain_walks(),
        walks_after_publish,
        "warm result lookup must not walk a compose/descriptor chain"
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
    let t_a = store.binder_token_for(space_a, 0).unwrap();
    let t_b = store.binder_token_for(space_b, 0).unwrap();
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
    let x = SemanticNodeId(1);
    let mut current = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(x, SemanticNodeId(2))]),
            ),
            None,
        )
        .unwrap();
    // bounded-loop: one compose per depth step through the flatten bound.
    for i in 1..=MAX_SUBSTITUTION_CHAIN_DEPTH {
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
    match store.substitution(current).unwrap() {
        CallSubstitution::Map { .. } => {}
        other => panic!("expected flattened Map, got {other:?}"),
    }
    let via_flat = store.apply(current, &SubstTerm::Binder(x)).unwrap();
    assert_eq!(
        via_flat,
        SubstTerm::Binder(SemanticNodeId(u64::from(MAX_SUBSTITUTION_CHAIN_DEPTH) + 2))
    );
}

#[test]
fn right_nested_compose_also_flattens() {
    let store = SignatureStore::new();
    let space = space(&store);
    let mut current = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(SemanticNodeId(20), SemanticNodeId(21))]),
            ),
            None,
        )
        .unwrap();
    // bounded-loop: right-nested compose past the flatten bound.
    for i in 0..MAX_SUBSTITUTION_CHAIN_DEPTH {
        let step = store
            .intern_substitution(
                CallSubstitution::map(
                    space,
                    CanonicalTypeSubstitution::new(vec![(
                        SemanticNodeId(u64::from(i) + 30),
                        SemanticNodeId(u64::from(i) + 31),
                    )]),
                ),
                None,
            )
            .unwrap();
        current = store.compose_after(step, current, None).unwrap();
    }
    match store.substitution(current).unwrap() {
        CallSubstitution::Map { .. } => {}
        other => panic!("expected flattened Map for right-nested chain, got {other:?}"),
    }
}

#[test]
fn compose_after_cross_binder_space_follows_application_law() {
    let store = SignatureStore::new();
    let decl = store
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let residual = store
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let x = SignatureStore::binder_token(1, 0).unwrap();
    let u = SignatureStore::binder_token(2, 0).unwrap();
    let string = SemanticNodeId(99);
    let descriptor_map = store
        .intern_substitution(
            CallSubstitution::map_across(
                decl,
                residual,
                CanonicalTypeSubstitution::new(vec![(x, u)]),
            ),
            None,
        )
        .unwrap();
    let call_map = store
        .intern_substitution(
            CallSubstitution::map_across(
                residual,
                residual,
                CanonicalTypeSubstitution::new(vec![(u, string)]),
            ),
            None,
        )
        .unwrap();
    let composed = store.compose_after(descriptor_map, call_map, None).unwrap();
    let via_compose = store.apply(composed, &SubstTerm::Binder(x)).unwrap();
    let mid = store.apply(descriptor_map, &SubstTerm::Binder(x)).unwrap();
    let via_parts = store.apply(call_map, &mid).unwrap();
    assert_eq!(via_compose, via_parts);
    assert_eq!(via_compose, SubstTerm::Binder(string));
}

#[test]
fn constructed_term_obeys_composition_law() {
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
    let term = SubstTerm::Constructed {
        ctor: 7,
        args: Box::from([SubstTerm::Binder(x)]),
    };
    let via_compose = store.apply(composed, &term).unwrap();
    let mid = store.apply(first, &term).unwrap();
    let via_parts = store.apply(second, &mid).unwrap();
    assert_eq!(via_compose, via_parts);
    assert_eq!(
        via_compose,
        SubstTerm::Constructed {
            ctor: 7,
            args: Box::from([SubstTerm::Binder(string)]),
        }
    );
}

#[test]
fn apply_term_on_compose_is_unresolved() {
    let err = CallSubstitution::Compose {
        domain: super::records::BinderSpaceId::from_raw(1),
        codomain: super::records::BinderSpaceId::from_raw(1),
        first: super::records::CallSubstitutionId::from_raw(1),
        second: super::records::CallSubstitutionId::from_raw(2),
        depth: 1,
    }
    .apply_term(&SubstTerm::Binder(SemanticNodeId(1)))
    .unwrap_err();
    assert_eq!(err, SubstError::UnresolvedCompose);
}

#[test]
fn compose_apply_increments_chain_walk_counter() {
    let store = SignatureStore::new();
    let space = space(&store);
    let x = SemanticNodeId(1);
    let first = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(x, SemanticNodeId(2))]),
            ),
            None,
        )
        .unwrap();
    let second = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(SemanticNodeId(2), SemanticNodeId(3))]),
            ),
            None,
        )
        .unwrap();
    let composed = store.compose_after(first, second, None).unwrap();
    let before = store.descriptor_chain_walks();
    let _ = store.apply(composed, &SubstTerm::Binder(x)).unwrap();
    assert!(
        store.descriptor_chain_walks() > before,
        "applying a compose node must count a chain walk"
    );
}
