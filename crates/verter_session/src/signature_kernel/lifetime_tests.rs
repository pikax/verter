use super::lifetime::{SignatureStore, StoreError};
use super::read_view::SemanticReadView;
use super::records::{BinderDeclaration, BinderSpace, SignatureSetRef, SpellingId};
use super::substitution::{CallSubstitution, SubstTerm, MAX_SUBSTITUTION_CHAIN_DEPTH};
use super::test_support::intern_one_call;
use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

#[test]
fn stale_epoch_handle_is_rejected() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(candidate) = set else {
        panic!("one");
    };
    let old = SemanticReadView::pin(&store);
    let new_epoch = store.replace_epoch().unwrap();
    assert_ne!(new_epoch, old.epoch());
    let fresh = SemanticReadView::pin(&store);
    assert_eq!(
        fresh.descriptor(candidate.signature).unwrap_err(),
        super::read_view::ReadError::StaleHandle
    );
    assert!(old.descriptor(candidate.signature).is_ok());
    let _ = StoreError::StaleHandle;
}

#[test]
fn old_pinned_reader_finishes_against_its_epoch() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let old = SemanticReadView::pin(&store);
    store.replace_epoch().unwrap();
    match old.read_set(set).unwrap() {
        super::read_view::BorrowedSet::One { .. } => {}
        _ => panic!("expected One, got a different borrowed set"),
    }
}

#[test]
fn unissued_handle_is_rejected() {
    let store = SignatureStore::new();
    let view = SemanticReadView::pin(&store);
    let bogus = super::records::SignatureDescriptorId::from_raw(0);
    assert_eq!(
        view.descriptor(bogus).unwrap_err(),
        super::read_view::ReadError::StaleHandle
    );
}

#[test]
fn live_readers_are_roots_until_drop() {
    let store = SignatureStore::new();
    assert_eq!(store.live_reader_count(), 0);
    let view = SemanticReadView::pin(&store);
    assert!(store.live_reader_count() >= 1);
    drop(view);
    assert_eq!(store.live_reader_count(), 0);
}

#[test]
fn retained_results_outlive_epoch_replacement_until_drained() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let subst = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let recipe = intern_recipe_of(&store, c.signature);
    let result = super::records::AppliedResult::context_free(
        c.signature,
        subst,
        recipe,
        super::test_support::intern_test_context([1; 16]),
    );
    store.retain_result(result).unwrap();
    assert_eq!(store.retained_len(), 1);
    store.replace_epoch().unwrap();
    assert_eq!(store.retained_len(), 1);
    assert!(store.retained_descriptor(0).is_ok());
    store.drain_retained();
    assert_eq!(store.retained_len(), 0);
    assert_eq!(
        store.retained_descriptor(0).unwrap_err(),
        StoreError::Missing
    );
}

fn intern_recipe_of(
    store: &SignatureStore,
    _descriptor: super::records::SignatureDescriptorId,
) -> super::records::SignatureResultRecipeId {
    let locator = store.intern_body_locator(99, None).expect("locator");
    store
        .intern_recipe(
            super::records::SignatureResultRecipe::Body {
                return_obligation_key: super::records::ReturnObligationKey {
                    body_locator: locator,
                    evaluation: crate::semantic_query::ResultEvaluationContextId::from_raw(0),
                },
            },
            None,
        )
        .expect("recipe")
}

#[test]
fn binder_tokens_are_logical_and_reject_overflow() {
    let a = SignatureStore::binder_token(1, 0).unwrap();
    let b = SignatureStore::binder_token(1, 256).unwrap();
    let c = SignatureStore::binder_token(2, 0).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(
        SignatureStore::binder_token(1 << 31, 0).unwrap_err(),
        StoreError::InvalidBinderToken
    );
    let store_fwd = SignatureStore::new();
    let store_rev = SignatureStore::new();
    let s_fwd_1 = store_fwd
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let _s_fwd_2 = store_fwd
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let _s_rev_2 = store_rev
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let s_rev_1 = store_rev
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    assert_ne!(s_fwd_1.index(), s_rev_1.index());
    assert_eq!(
        store_fwd.binder_token_for(s_fwd_1, 0).unwrap(),
        store_rev.binder_token_for(s_rev_1, 0).unwrap()
    );
}

#[test]
fn intern_rejects_stale_embedded_handles_across_epoch_replacement() {
    let store = SignatureStore::new();
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    store.replace_epoch().unwrap();
    let layout = store
        .intern_layout(
            super::records::ParameterLayout {
                parameters: Box::from([]),
                rest: None,
            },
            None,
        )
        .unwrap();
    let err = store
        .intern_shape(
            super::records::SignatureInputShape {
                kind: super::records::SignatureKind::Call,
                binder_declarations: space,
                this_parameter: None,
                parameter_layout: layout,
                declared_minimum: 0,
                signature_semantic_flags: super::records::SignatureSemanticFlags::NONE,
            },
            None,
        )
        .unwrap_err();
    assert_eq!(err, StoreError::StaleHandle);
}

#[test]
fn lookup_result_does_not_publish_on_miss() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let subst = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let recipe = intern_recipe_of(&store, c.signature);
    let result = super::records::AppliedResult::context_free(
        c.signature,
        subst,
        recipe,
        super::test_support::intern_test_context([1; 16]),
    );
    assert_eq!(store.lookup_result(&result).unwrap(), None);
    let id = store.publish_result(result.clone(), None).unwrap();
    assert_eq!(store.lookup_result(&result).unwrap(), Some(id));
}

#[test]
fn publish_result_rejects_type_tokens_from_a_retired_epoch() {
    let store = SignatureStore::new();
    let stale_token = store
        .intern_type_token(crate::semantic_query::SemanticNodeId(77), None)
        .unwrap();
    store.replace_epoch().unwrap();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(candidate) = set else {
        panic!("one");
    };
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let substitution = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let recipe = intern_recipe_of(&store, candidate.signature);
    let result = super::records::AppliedResult {
        descriptor: candidate.signature,
        substitution,
        recipe,
        evaluation: crate::semantic_query::CONTEXT_FREE_EVALUATION,
        semantic_context: super::test_support::intern_test_context([2; 16]),
        evidence: crate::semantic_query::CONTEXT_FREE_EVIDENCE,
        return_type: Some(stale_token),
        effects: None,
    };
    assert_eq!(
        store.publish_result(result, None),
        Err(StoreError::StaleHandle)
    );
}

#[test]
fn concurrent_replace_epoch_publishes_in_order() {
    use std::sync::Arc;
    let store = Arc::new(SignatureStore::new());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let store = Arc::clone(&store);
            scope.spawn(move || store.replace_epoch().unwrap());
        }
    });
    assert_eq!(store.epoch().as_u32(), 9);
}

#[test]
fn compose_rejects_unrelated_codomain() {
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
    let other = store
        .intern_binder_space(
            BinderSpace {
                key: 3,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let first = store
        .intern_substitution(
            CallSubstitution::map(
                decl,
                CanonicalTypeSubstitution::new(vec![(
                    SignatureStore::binder_token(1, 0).unwrap(),
                    SignatureStore::binder_token(1, 1).unwrap(),
                )]),
            ),
            None,
        )
        .unwrap();
    let second = store
        .intern_substitution(
            CallSubstitution::map(
                other,
                CanonicalTypeSubstitution::new(vec![(
                    SignatureStore::binder_token(3, 0).unwrap(),
                    crate::semantic_query::SemanticNodeId(9),
                )]),
            ),
            None,
        )
        .unwrap();
    assert_eq!(
        store.compose_after(first, second, None).unwrap_err(),
        StoreError::WrongBinderSpace
    );
}

#[test]
fn intern_binder_space_rejects_stale_or_unissued_spelling() {
    let store = SignatureStore::new();
    let spelling = store.intern_spelling("T", None).unwrap();
    store.replace_epoch().unwrap();
    let stale = store
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([BinderDeclaration {
                    spelling,
                    constraint: None,
                    default: None,
                }]),
            },
            None,
        )
        .unwrap_err();
    assert_eq!(stale, StoreError::StaleHandle);
    let unissued = store
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([BinderDeclaration {
                    spelling: SpellingId::from_raw(0),
                    constraint: None,
                    default: None,
                }]),
            },
            None,
        )
        .unwrap_err();
    assert_eq!(unissued, StoreError::StaleHandle);
}

#[test]
fn intern_substitution_recomputes_compose_depth_and_spaces() {
    let store = SignatureStore::new();
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let stale_space = space;
    store.replace_epoch().unwrap();
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let first = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(SemanticNodeId(1), SemanticNodeId(2))]),
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
    let composed = store
        .intern_substitution(
            CallSubstitution::Compose {
                domain: stale_space,
                codomain: stale_space,
                first,
                second,
                depth: 0,
            },
            None,
        )
        .unwrap();
    match store.substitution(composed).unwrap() {
        CallSubstitution::Compose {
            domain,
            codomain,
            depth,
            ..
        } => {
            assert_eq!(domain, space);
            assert_eq!(codomain, space);
            assert_eq!(depth, 1);
        }
        other => panic!("expected Compose, got {other:?}"),
    }
}

#[test]
fn intern_hand_built_compose_chain_flattens_past_the_bound() {
    let store = SignatureStore::new();
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let mut current = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                CanonicalTypeSubstitution::new(vec![(SemanticNodeId(1), SemanticNodeId(2))]),
            ),
            None,
        )
        .unwrap();
    // bounded-loop: hand-built Compose claiming depth 0, one step past the bound.
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
        current = store
            .intern_substitution(
                CallSubstitution::Compose {
                    domain: space,
                    codomain: space,
                    first: current,
                    second: step,
                    depth: 0,
                },
                None,
            )
            .unwrap();
    }
    match store.substitution(current).unwrap() {
        CallSubstitution::Map { .. } => {}
        other => panic!("expected flattened Map, got {other:?}"),
    }
}

#[test]
fn context_distinct_results_do_not_alias() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let subst = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let recipe = intern_recipe_of(&store, c.signature);
    let first_ctx = super::test_support::intern_test_context([0; 16]);
    let other_ctx = super::test_support::intern_test_context([1; 16]);
    assert_ne!(first_ctx, other_ctx);
    let a = super::records::AppliedResult::context_free(c.signature, subst, recipe, first_ctx);
    let b = super::records::AppliedResult::context_free(c.signature, subst, recipe, other_ctx);
    let id_a = store.publish_result(a.clone(), None).unwrap();
    let id_b = store.publish_result(b.clone(), None).unwrap();
    assert_ne!(id_a, id_b);
    assert_eq!(store.lookup_result(&a).unwrap(), Some(id_a));
    assert_eq!(store.lookup_result(&b).unwrap(), Some(id_b));
}

#[test]
fn live_reader_count_includes_pinned_retired_epoch() {
    let store = SignatureStore::new();
    let view = SemanticReadView::pin(&store);
    assert!(store.live_reader_count() >= 1);
    store.replace_epoch().unwrap();
    assert!(
        store.live_reader_count() >= 1,
        "retired pinned reader vanished from live_reader_count"
    );
    drop(view);
    assert_eq!(store.live_reader_count(), 0);
}

#[test]
fn pinned_view_reads_substitution_after_epoch_replacement() {
    let store = SignatureStore::new();
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let subst = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let view = SemanticReadView::pin(&store);
    let descriptor = view.descriptor(c.signature).unwrap();
    let template = view.template(descriptor.template).unwrap();
    let _ = view.shape(template.input_shape).unwrap();
    let _ = view.recipe(template.result_recipe).unwrap();
    store.replace_epoch().unwrap();
    assert!(view.substitution(subst).is_ok());
    assert!(view.template(descriptor.template).is_ok());
    assert_eq!(
        store.substitution(subst).unwrap_err(),
        StoreError::StaleHandle
    );
    let applied = view
        .apply(subst, &SubstTerm::Binder(SemanticNodeId(1)))
        .unwrap();
    assert_eq!(applied, SubstTerm::Binder(SemanticNodeId(1)));
}
