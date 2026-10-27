//! The request completion tier keeps results on the process retention
//! account: charged while kept, released with the tier, refused whole under
//! pressure, and shared by every dispatcher of one request on one graph and
//! view.

use std::sync::Arc;

use verter_session_query::facts::fact_cache::ReadSetSignature;
use verter_session_query::facts::store_view::StoreViewCompatToken;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};

use super::{CompletedResult, RequestCompletionTier, RequestCompletions, ValueRead};
use crate::project_semantic_dispatch::WorkspaceEdit;
use crate::semantic_query::{
    LiteralValue, ReadReceipt, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue,
};
use crate::semantic_query_memo::SemanticGraphStore;

fn edit(content: u64) -> WorkspaceEdit {
    WorkspaceEdit {
        content,
        source_env: None,
        workspace_shape: 0,
    }
}

fn completed() -> ReadSetSignature {
    ReadSetSignature::new(
        verter_session_query::facts::policy::FactPolicy::production().set_of(Vec::new()),
    )
}

fn store(limits: RetentionLimits) -> (Arc<SemanticRetentionAccount>, SemanticGraphStore) {
    let account = SemanticRetentionAccount::new(limits);
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    (account, store)
}

fn literal(store: &SemanticGraphStore, value: &str) -> SemanticNodeId {
    store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        value.to_string(),
    )))
}

/// A distinct key and a kept read of `node`.
fn entry(store: &SemanticGraphStore, node: SemanticNodeId) -> (SemanticQueryKey, ValueRead) {
    let key = SemanticQueryKey::Conditional {
        check: node,
        extends: node,
        true_branch: node,
        false_branch: node,
        distributive: false,
        pending: None,
    };
    let read = store
        .completed_member_read(SemanticQueryValue::TypeNode(node), ReadReceipt::Unpriced)
        .expect("a live node");
    (key, read)
}

/// A kept result is charged on the account for as long as the tier keeps
/// it, serves only under the edit clocks it was computed at, and is
/// released with the tier.
#[test]
fn a_kept_result_is_charged_while_kept_and_released_with_its_tier() {
    let (account, store) = store(RetentionLimits::defaults());
    let _scope = store.enter_node_root_scope();
    let (key, read) = entry(&store, literal(&store, "kept"));
    let before = account.snapshot().retained_bytes;
    let tier = RequestCompletionTier::default();
    tier.keep(&store, &key, &read, completed(), edit(1));
    assert_eq!(
        account.snapshot().retained_bytes - before,
        CompletedResult::footprint(&read),
        "the kept result is charged"
    );
    assert!(tier.completed(&key, edit(1), |_| true).is_some());
    assert!(
        tier.completed(&key, edit(2), |_| true).is_none(),
        "an edit since the result was computed is never served"
    );
    assert!(
        tier.completed(&key, edit(1), |_| false).is_none(),
        "a value the graph no longer serves is never served"
    );
    drop(tier);
    assert_eq!(
        account.snapshot().retained_bytes,
        before,
        "the tier's teardown releases what it kept"
    );
}

/// An account that declines the reservation keeps nothing and charges
/// nothing: the result's delivery stays its only owner.
#[test]
fn a_tier_under_pressure_keeps_nothing() {
    let (account, store) = store(RetentionLimits {
        max_entry_bytes: 0,
        ..RetentionLimits::defaults()
    });
    let _scope = store.enter_node_root_scope();
    let (key, read) = entry(&store, literal(&store, "refused"));
    let before = account.snapshot();
    let tier = RequestCompletionTier::default();
    tier.keep(&store, &key, &read, completed(), edit(1));
    assert_eq!(tier.len(), 0, "a refused result is not kept");
    assert!(tier.completed(&key, edit(1), |_| true).is_none());
    let after = account.snapshot();
    assert_eq!(after.retained_bytes, before.retained_bytes);
    assert_eq!(
        after.refusals_oversized,
        before.refusals_oversized + 1,
        "the reservation was refused, not skipped"
    );
}

/// A component is kept whole, under one charge for all of its members, or
/// not at all: an account with room for one member's footprint but not the
/// component's keeps none of them.
#[test]
fn a_component_is_kept_whole_or_not_at_all() {
    let (account, store) = store(RetentionLimits::defaults());
    let _scope = store.enter_node_root_scope();
    let members: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|value| entry(&store, literal(&store, value)))
        .collect();
    let bytes: usize = members
        .iter()
        .map(|(_, read)| CompletedResult::footprint(read))
        .sum();
    let before = account.snapshot();
    let tier = RequestCompletionTier::default();
    assert!(tier.keep_component(&store, members.clone(), &completed(), edit(1)));
    assert_eq!(tier.len(), 3);
    let after = account.snapshot();
    assert_eq!(after.retained_bytes - before.retained_bytes, bytes);
    assert_eq!(
        after.admissions,
        before.admissions + 1,
        "one charge covers the whole component"
    );
    drop(tier);
    assert_eq!(account.snapshot().retained_bytes, before.retained_bytes);

    let (account, store) = store_with_one_entry_of_room(&members[0].1);
    let _scope = store.enter_node_root_scope();
    let members: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|value| entry(&store, literal(&store, value)))
        .collect();
    let before = account.snapshot().retained_bytes;
    let tier = RequestCompletionTier::default();
    assert!(!tier.keep_component(&store, members.clone(), &completed(), edit(1)));
    assert_eq!(tier.len(), 0, "no member of a refused component is kept");
    for (key, _) in &members {
        assert!(tier.completed(key, edit(1), |_| true).is_none());
    }
    assert_eq!(account.snapshot().retained_bytes, before);
}

fn store_with_one_entry_of_room(
    read: &ValueRead,
) -> (Arc<SemanticRetentionAccount>, SemanticGraphStore) {
    store(RetentionLimits {
        max_entry_bytes: CompletedResult::footprint(read) + 1,
        ..RetentionLimits::defaults()
    })
}

/// One request keeps one tier per graph and store view: every dispatcher of
/// the request on the same graph and view gets the same tier, another view
/// or another graph its own.
#[test]
fn a_request_shares_one_tier_per_graph_and_view() {
    let completions = RequestCompletions::default();
    let graph = Arc::new(SemanticGraphStore::default());
    let other_graph = Arc::new(SemanticGraphStore::default());
    let view = StoreViewCompatToken {
        epoch: 1,
        session: None,
        validity_fingerprint: 7,
    };
    let session_view = StoreViewCompatToken {
        session: Some(3),
        ..view
    };
    let tier = completions.tier_for(&graph, view);
    assert!(Arc::ptr_eq(&tier, &completions.tier_for(&graph, view)));
    assert!(!Arc::ptr_eq(
        &tier,
        &completions.tier_for(&graph, session_view)
    ));
    assert!(!Arc::ptr_eq(
        &tier,
        &completions.tier_for(&other_graph, view)
    ));
}
