//! The graph's own storage — its arena and its signature epochs — is pinned
//! against the store's account while it lives and released with it.

use std::sync::Arc;

use verter_session_query::retention::allocation::arc_block_bytes;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};

use super::graph_fixture::GraphFixture;
use super::SemanticGraphStore;
use crate::semantic_query::{LiteralValue, SemanticNodeData};

fn account() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits::defaults())
}

#[test]
fn graph_storage_is_pinned_while_its_nodes_live_and_released_with_them() {
    const NODES: usize = 600;
    let account = account();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
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

/// The reverse index keeps its own copy of each registered canonical's
/// text: that copy is charged to the index while it holds it and given back
/// when the canonical's last holder drains.
#[test]
fn a_reverse_index_charges_its_canonical_text_until_it_drains() {
    use crate::semantic_query::{PrimitiveKind, RelateMemoKey, RelationContext, RelationOutcome};
    use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};

    const CANONICAL_LEN: usize = 10_000;
    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let canonical = format!("/ws/{}.ts", "r".repeat(CANONICAL_LEN));
    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.clone().into(),
        hash: [3; 16],
    }]));
    let roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(canonical.as_str())]);
    let source = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    store.insert_relation_payload_for_tests(
        RelateMemoKey::assignable(source, target, RelationContext::default()),
        carrier,
        roots,
        store.relation_payload_for_tests(RelationOutcome::Assignable),
        0,
    );
    assert_eq!(store.canonical_to_entries_count(&canonical), 1);
    let charged = store.canonical_to_entries.storage_charge_bytes();
    assert!(
        charged >= arc_block_bytes(canonical.len()),
        "the index's copy of the oversized canonical is charged: {charged}"
    );

    assert_eq!(store.invalidate_canonical(&canonical), 1);
    assert!(
        store.canonical_to_entries.storage_charge_bytes() < arc_block_bytes(canonical.len()),
        "the drained canonical's text is no longer charged"
    );
    store.invalidate_all();
    assert_eq!(
        store.canonical_to_entries.storage_charge_bytes(),
        0,
        "an emptied index holds no storage and no charge"
    );
}

/// A payload's own heap — a literal's text, a template's quasi array and
/// names — is charged with its graph storage, and an allocation two live
/// payloads share is charged once; every byte is released with the last
/// payload reaching it.
#[test]
fn graph_storage_charges_payload_heap_once_per_shared_allocation() {
    use crate::semantic_query::PayloadArray;

    const TEXT: usize = 1 << 20;
    let account = account();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let pinned = || account.snapshot().pinned_bytes;
    harness.run(|store| {
        // The template's quasi list is an owned allocation: every payload
        // naming it shares its one charge.
        let quasis = verter_type_expr::OwnedSlice::from(vec![verter_type_expr::OwnedStr::from(
            "q".repeat(TEXT),
        )]);
        let small = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "small".into(),
        )));
        let before = pinned();
        let _literal = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "l".repeat(TEXT),
        )));
        assert!(
            pinned() - before >= TEXT,
            "a literal's text is charged with its payload: {} < {TEXT}",
            pinned() - before
        );

        let before = pinned();
        let _first = store.intern_node(SemanticNodeData::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: PayloadArray::from(Vec::new()),
        });
        assert!(
            pinned() - before >= TEXT,
            "the quasi array and its text are charged with the first payload reaching them"
        );
        let before = pinned();
        let _second = store.intern_node(SemanticNodeData::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: PayloadArray::from(vec![small]),
        });
        drop(quasis);
        assert!(
            pinned() - before < TEXT / 2,
            "a second payload sharing the quasis is not charged for them again: {}",
            pinned() - before
        );
    });
    assert_eq!(harness.live_nodes(), 0);
    assert!(
        pinned() < TEXT,
        "the released payloads' heap is no longer charged: {}",
        pinned()
    );
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A payload a reader still holds when its last owner destroys its slot
/// stays allocated, so it stays charged with graph storage; the reader's
/// drop releases it in the same step, with no later arena operation, and
/// the store's teardown restores the baseline.
#[test]
fn a_released_payload_a_reader_still_holds_stays_charged_until_the_reader_lets_go() {
    use crate::semantic_query::NodeScopeId;
    const TEXT: usize = 1 << 20;
    const FILE: &str = "/held.ts";
    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let scope = |canonical: &str| NodeScopeId::File {
        canonical_id: verter_type_expr::OwnedStr::from(canonical),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [0; 16],
        local_scope: None,
    };
    let owner = store.enter_node_root_scope();
    let node = store.intern_node_with_scope(
        SemanticNodeData::Literal(LiteralValue::String("h".repeat(TEXT))),
        scope(FILE),
    );
    let held = store
        .arena
        .core
        .live_payload(node)
        .expect("a live node reads");
    let charged = store.graph_storage_charge_for_tests();
    assert!(charged >= TEXT);
    drop(owner);
    assert!(!store.arena.is_live(node));
    assert!(
        store.graph_storage_charge_for_tests() >= TEXT,
        "the held payload is still allocated, so still charged"
    );
    drop(held);
    assert!(
        store.graph_storage_charge_for_tests() < TEXT,
        "the reader's drop releases the payload's charge at once"
    );
    drop(store);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A store no reader outlives drops its graph whole: the caches' roots are
/// released with the arena rather than node by node, and the teardown still
/// restores the baseline.
#[test]
fn a_store_teardown_no_reader_outlives_releases_no_slot_one_by_one() {
    const NODES: usize = 64;
    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    {
        let _scope = store.enter_node_root_scope();
        for index in 0..NODES {
            let node = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                "cached-{index}"
            ))));
            // The cache entry is the node's only owner once the scope ends.
            store.substitute_memo_publish(node, node, node, node);
        }
    }
    assert!(account.snapshot().total_bytes() > 0);
    let before = super::arena::slot_releases_for_tests();
    drop(store);
    assert_eq!(super::arena::slot_releases_for_tests() - before, 0);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A reader holding a root past the store's teardown keeps what it reaches
/// live and charged, and only that: a node only a cache held is released
/// with the cache, and the reader's drop restores the baseline.
#[test]
fn a_store_teardown_leaves_charged_only_what_an_outliving_reader_reaches() {
    const CACHED: usize = 1 << 21;
    const HELD: usize = 1 << 20;
    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    {
        let _scope = store.enter_node_root_scope();
        let cached = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "c".repeat(CACHED),
        )));
        store.substitute_memo_publish(cached, cached, cached, cached);
    }
    let reader = store.enter_node_root_scope();
    let _held = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "h".repeat(HELD),
    )));
    assert!(account.snapshot().total_bytes() >= CACHED + HELD);
    drop(store);
    let charged = account.snapshot().total_bytes();
    assert!(
        (HELD..CACHED).contains(&charged),
        "only what the reader reaches stays charged after the teardown: {charged}"
    );
    drop(reader);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A payload a reader holds past the store's teardown stays charged — and
/// only it: the rest of the graph's storage goes with the store and the
/// last owner of its nodes, and the reader's drop restores the baseline, in
/// either release order.
#[test]
fn a_payload_a_reader_holds_past_the_store_stays_charged_until_its_own_drop() {
    const TEXT: usize = 1 << 20;
    const OTHER: usize = 1 << 21;
    for reader_first in [false, true] {
        let account = account();
        let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
        let owner = store.enter_node_root_scope();
        let node = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "h".repeat(TEXT),
        )));
        let _other = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "o".repeat(OTHER),
        )));
        let held = store
            .arena
            .core
            .live_payload(node)
            .expect("a live node reads");
        if reader_first {
            drop(held);
            assert!(store.graph_storage_charge_for_tests() >= TEXT + OTHER);
            drop(store);
            drop(owner);
            assert_eq!(account.snapshot().total_bytes(), 0);
            continue;
        }
        drop(store);
        drop(owner);
        let charged = account.snapshot().total_bytes();
        assert!(
            (TEXT..OTHER).contains(&charged),
            "only the held payload stays charged after the store's teardown: {charged}"
        );
        drop(held);
        assert_eq!(
            account.snapshot().total_bytes(),
            0,
            "the reader's drop restores the baseline"
        );
    }
}

/// Every project-generation dependency entry of a store names the store's
/// one `"<project>"` allocation, pinned once when the store mints it: many
/// entries add no charge, a cache hold moves the one charge into retention
/// and back, the store keeps the name charged while it holds it, and the
/// charge leaves once the store releases its idle name.
#[test]
fn the_project_dependency_name_is_one_allocation_charged_once_while_the_store_holds_it() {
    use crate::semantic_query::DepVersion;
    use verter_session_query::retention::owned::{ChargeCtx, OwnedStr};
    use verter_session_query::retention::ChargeClass;

    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let before = account.snapshot();
    let entries: Vec<_> = (0..100u64)
        .map(|generation| store.project_dep_entry(DepVersion::ProjectGeneration(generation)))
        .collect();
    assert!(
        entries
            .iter()
            .all(|entry| OwnedStr::ptr_eq(&entry.0, &entries[0].0) && entry.0 == "<project>"),
        "every entry names the store's one allocation"
    );
    let charge = OwnedStr::charge_for_len("<project>".len());
    let minted = account.snapshot();
    assert_eq!(
        minted.pinned_bytes - before.pinned_bytes,
        charge,
        "a hundred entries pin the one name once"
    );
    assert_eq!(entries[0].0.charge_class(), Some(ChargeClass::Pinned));

    let hold = entries[50]
        .0
        .hold(ChargeCtx::new(&account))
        .expect("adopted");
    let held = account.snapshot();
    assert_eq!(entries[0].0.charge_class(), Some(ChargeClass::Retained));
    assert_eq!(
        held.total_bytes(),
        minted.total_bytes(),
        "a cache hold moves the one charge, it adds none"
    );
    drop(hold);
    assert_eq!(entries[0].0.charge_class(), Some(ChargeClass::Pinned));
    assert_eq!(account.snapshot().total_bytes(), minted.total_bytes());

    drop(entries);
    assert_eq!(
        account.snapshot().pinned_bytes - before.pinned_bytes,
        charge,
        "the store keeps its name, so the name stays charged"
    );
    assert!(
        OwnedStr::ptr_eq(
            &store.project_dep_entry(DepVersion::ProjectGeneration(0)).0,
            &store.project_dep_entry(DepVersion::ProjectGeneration(1)).0,
        ),
        "a later entry names the same allocation"
    );

    let survivor = store.project_dep_entry(DepVersion::ProjectGeneration(2));
    store.release_drained_storage();
    assert_eq!(
        account.snapshot().pinned_bytes - before.pinned_bytes,
        charge,
        "a name an entry still names is not released"
    );
    drop(survivor);
    store.release_drained_storage();
    assert_eq!(
        account.snapshot().total_bytes(),
        before.total_bytes(),
        "an idle name leaves with the drained storage"
    );
    let remint = store.project_dep_entry(DepVersion::ProjectGeneration(3));
    assert_eq!(
        account.snapshot().pinned_bytes - before.pinned_bytes,
        charge,
        "the next entry mints and charges the name again, once"
    );
    drop(remint);
    drop(store);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the store's teardown releases the name with everything else"
    );
}

/// The store's name is a liveness obligation: an account with no headroom
/// left still charges it, never refuses it, and the pinned bytes consume
/// headroom a later refusable reservation is then refused for.
#[test]
fn the_project_dependency_name_is_charged_at_a_full_account() {
    use crate::semantic_query::DepVersion;
    use verter_session_query::retention::owned::OwnedStr;
    use verter_session_query::retention::{ChargeClass, RetentionAdmission};

    let limits = RetentionLimits {
        aggregate_ceiling_bytes: 64 * 1024,
        max_entry_bytes: 64 * 1024,
        pin_threshold_bytes: 64 * 1024,
    };
    let account = SemanticRetentionAccount::new(limits);
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let headroom = limits.aggregate_ceiling_bytes - account.snapshot().total_bytes();
    let RetentionAdmission::Admitted(filler) = account.reserve(ChargeClass::Retained, headroom)
    else {
        panic!("the remaining headroom is admitted");
    };
    let full = account.snapshot();
    let entry = store.project_dep_entry(DepVersion::ProjectGeneration(7));
    assert_eq!(
        account.snapshot().pinned_bytes - full.pinned_bytes,
        OwnedStr::charge_for_len("<project>".len()),
        "the name is charged although the account is full"
    );
    assert!(
        matches!(
            account.reserve(ChargeClass::Retained, 16),
            RetentionAdmission::Refused(_)
        ),
        "the pinned name consumes headroom a cache cannot then take"
    );
    drop(filler);
    drop(entry);
    drop(store);
    assert_eq!(account.snapshot().total_bytes(), 0);
}
