//! The family memo's retired-kernel-epoch predicates, over real kernel
//! handles.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{SemanticContextId, SignatureKind, CONTEXT_FREE_EVALUATION};
use crate::signature_kernel::test_support::intern_one_call;
use crate::signature_kernel::{
    BinderSpace, CallSubstitution, ReadSignatureResultKey, ResultDemand, SignatureSetRef,
    SignatureSetValue,
};

fn set_value(set: SignatureSetRef) -> QueryResult<SemanticQueryValue> {
    QueryResult::Value(SemanticQueryValue::SignatureSet(SignatureSetValue {
        set,
        nodes: Arc::from([]),
    }))
}

/// A `ReadSignatureResult` family names the epoch of its descriptor and
/// call substitution; a `SignaturesOfType` family names none (its VALUE
/// does), so only the former is retired by its key.
#[test]
fn a_read_signature_result_key_is_retired_with_its_handles_epoch() {
    let graph = SemanticGraphStore::new();
    let store = graph.signature_store();
    let SignatureSetRef::One(candidate) = intern_one_call(store) else {
        panic!("the fixture interns one candidate");
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
    let call = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let result_family = FamilyKey::ReadSignatureResult {
        key: ReadSignatureResultKey {
            descriptor: candidate.signature,
            call_substitution: call,
            projection: ResultDemand::Return,
            evaluation: CONTEXT_FREE_EVALUATION,
            semantic_context: SemanticContextId::production(),
        },
    };
    let set_family = FamilyKey::SignaturesOfType {
        subject: SemanticNodeId(1),
        kind: SignatureKind::Call,
        context: SemanticContextId::production(),
    };
    assert!(!result_family.names_retired_kernel_epoch(store.epoch()));

    let current = store.replace_epoch().unwrap();
    assert!(result_family.names_retired_kernel_epoch(current));
    assert!(!set_family.names_retired_kernel_epoch(current));
}

/// A value names a retired epoch exactly when it carries a kernel handle of
/// another epoch: an empty set and a non-kernel value never do.
#[test]
fn only_a_kernel_handle_of_another_epoch_names_a_retired_epoch() {
    let graph = SemanticGraphStore::new();
    let store = graph.signature_store();
    let live = intern_one_call(store);
    let empty = set_value(SignatureSetRef::Empty);
    let node = QueryResult::Value(SemanticQueryValue::TypeNode(SemanticNodeId(1)));
    assert!(!graph.names_retired_kernel_epoch(&set_value(live)));

    store.replace_epoch().unwrap();
    assert!(graph.names_retired_kernel_epoch(&set_value(live)));
    assert!(!graph.names_retired_kernel_epoch(&empty));
    assert!(!graph.names_retired_kernel_epoch(&node));
    let fresh = intern_one_call(store);
    assert!(!graph.names_retired_kernel_epoch(&set_value(fresh)));
}

/// The cap-driven replacement fires only past the cap, once per crossing,
/// and leaves an empty current epoch behind.
#[test]
fn the_record_cap_replaces_the_epoch_only_past_the_cap() {
    let graph = SemanticGraphStore::new();
    let store = graph.signature_store();
    let _ = intern_one_call(store);
    let held = store.interned_len();
    assert!(held > 0, "the fixture interns records");
    let before = store.epoch();

    assert_eq!(graph.compact_signature_store_over(held), None);
    assert_eq!(store.epoch(), before, "at the cap the epoch stays");

    let replaced = graph.compact_signature_store_over(held - 1);
    assert!(replaced.is_some_and(|epoch| epoch != before));
    assert_eq!(
        store.interned_len(),
        0,
        "the replacement epoch starts empty"
    );
    assert_eq!(
        graph.compact_signature_store_over(0),
        None,
        "an empty epoch is never over the cap"
    );
}
