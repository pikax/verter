//! The graph's own storage — its arena and its signature epochs — is pinned
//! against the store's account while it lives and released with it.

use std::sync::Arc;

use verter_session_query::retention::allocation::arc_block_bytes;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};

use super::ownership_harness::CountedOwnershipHarness;
use super::SemanticGraphStore;
use crate::semantic_query::{LiteralValue, SemanticNodeData};

fn account() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits::defaults())
}

#[test]
fn graph_storage_is_pinned_while_its_nodes_live_and_released_with_them() {
    const NODES: usize = 600;
    let account = account();
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
    let payload = arc_block_bytes(std::mem::size_of::<SemanticNodeData>());
    harness.run(|store| {
        for index in 0..NODES {
            let _ = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                "node-{index}"
            ))));
        }
        let pinned = account.snapshot().pinned_bytes;
        assert!(
            pinned >= NODES * payload,
            "every live payload block is charged: {pinned} < {NODES} x {payload}"
        );
        assert_eq!(pinned, store.graph_storage_charge_for_tests());
    });
    assert_eq!(harness.live_nodes(), 0);
    let after = account.snapshot().pinned_bytes;
    harness.run(|store| {
        assert_eq!(
            store.arena.core.slot_storage_charge_bytes(),
            0,
            "released payloads and their emptied chunks are no longer charged"
        );
        assert_eq!(
            after,
            store.graph_storage_charge_for_tests(),
            "what stays pinned is the dedup tables' retained capacity"
        );
    });
    assert!(after < NODES * payload);
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the store's teardown releases its graph storage"
    );
}

#[test]
fn a_retired_signature_epoch_releases_its_storage_with_its_last_holder() {
    const SPELLINGS: usize = 2_000;
    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let kernel = store.signature_store();
    for index in 0..SPELLINGS {
        kernel
            .intern_spelling(&format!("{}-{index}", "s".repeat(64)), None)
            .expect("interned");
    }
    let charged = kernel.current_epoch_storage_charge();
    assert!(
        charged >= SPELLINGS * 64,
        "every interned spelling's text is charged: {charged}"
    );
    let kernel_pinned = || account.snapshot().pinned_bytes - store.graph_storage_charge_for_tests();
    assert_eq!(kernel_pinned(), charged);

    let reader = kernel.lease_epoch(kernel.epoch()).expect("current epoch");
    kernel.replace_epoch().expect("replaced");
    assert_eq!(kernel.current_epoch_storage_charge(), 0);
    assert_eq!(
        kernel_pinned(),
        charged,
        "a retired epoch a reader still holds stays charged"
    );
    drop(reader);
    assert_eq!(
        kernel_pinned(),
        0,
        "the retired epoch's storage is released with its last holder"
    );
}
