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
        assert_eq!(
            pinned,
            store.graph_storage_charge_for_tests() + store.project_dep_name_pinned_bytes()
        );
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
            store.graph_storage_charge_for_tests() + store.project_dep_name_pinned_bytes(),
            "what stays pinned is the dedup tables' retained capacity and the              store's name"
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
    let kernel_pinned = || {
        account.snapshot().pinned_bytes
            - store.graph_storage_charge_for_tests()
            - store.project_dep_name_pinned_bytes()
    };
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
    let carrier = ReadSetSignature::new(
        verter_session_query::facts::policy::FactPolicy::production().set_of(vec![
            FactVersionRef::FileWholeHash {
                canonical_id: canonical.clone().into(),
                hash: [3; 16],
            },
        ]),
    );
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
/// one `"<project>"` allocation, minted with the store and pinned once as
/// its own storage: many entries add no charge, a cache hold moves the one
/// charge into retention and back, and the charge lives exactly as long as
/// the allocation — past the store while an entry still names it.
#[test]
fn the_project_dependency_name_is_one_allocation_charged_once_for_its_life() {
    use crate::semantic_query::DepVersion;
    use verter_session_query::retention::owned::{ChargeCtx, OwnedStr};
    use verter_session_query::retention::ChargeClass;

    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let charge = OwnedStr::charge_for_len("<project>".len());
    let minted = account.snapshot();
    assert!(
        minted.pinned_bytes >= charge,
        "the store pins its name when it is built: {minted:?}"
    );
    assert_eq!(
        minted.pinned_bytes,
        store.store_storage_pins_for_tests(),
        "the name is counted as the store's own storage"
    );
    let entries: Vec<_> = (0..100u64)
        .map(|generation| store.project_dep_entry(DepVersion::ProjectGeneration(generation)))
        .collect();
    assert!(
        entries
            .iter()
            .all(|entry| OwnedStr::ptr_eq(&entry.0, &entries[0].0) && entry.0 == "<project>"),
        "every entry names the store's one allocation"
    );
    assert_eq!(
        account.snapshot(),
        minted,
        "a hundred entries take no charge and no admission"
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

    let survivor = entries[7].clone();
    drop(entries);
    drop(store);
    assert_eq!(
        account.snapshot().total_bytes(),
        charge,
        "an entry outliving the store keeps the name charged, once"
    );
    drop(survivor);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// The store's name is a liveness obligation charged when the store is
/// built, so an account with no headroom left serves entries without a
/// reservation to refuse, and the pin keeps consuming headroom a later
/// refusable reservation is then refused for.
#[test]
fn project_dependency_entries_need_no_admission_at_a_full_account() {
    use crate::semantic_query::DepVersion;
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
    let entries: Vec<_> = (0..10u64)
        .map(|generation| store.project_dep_entry(DepVersion::ProjectGeneration(generation)))
        .collect();
    assert_eq!(
        account.snapshot(),
        full,
        "entries at a full account take nothing and are refused nothing"
    );
    assert!(
        matches!(
            account.reserve(ChargeClass::Retained, 16),
            RetentionAdmission::Refused(_)
        ),
        "the pinned name consumes headroom a cache cannot then take"
    );
    drop(filler);
    drop(entries);
    drop(store);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A delivery's short root sets ride inline in its owner and a longer one
/// names its shared block, so its metadata charge is the owner's block plus
/// that block's cell, each taken once at mint:
/// cloning the delivery adds no charge and no count, the delivery keeps its
/// nodes live across requests (construction scopes) and a cache
/// invalidation, and its last holder's drop releases both the charge and
/// the nodes.
#[test]
fn a_delivery_charges_its_root_sets_once_and_holds_its_nodes_across_requests() {
    use super::delivery_lease::SemanticDeliveryLease;
    use verter_session_query::retention::allocation::heap_block_bytes;

    const NODES: usize = 6;
    let account = account();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    // Pinned bytes beyond the graph's own storage: what callers hold.
    let caller_pinned = || {
        account.snapshot().pinned_bytes
            - harness.run(|store| store.graph_storage_charge_for_tests())
    };
    let before = caller_pinned();
    let (lease, ids) = harness.run(|store| {
        let ids: Vec<_> = (0..NODES)
            .map(|index| {
                store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                    "delivered-{index}"
                ))))
            })
            .collect();
        let roots = store.root_nodes(ids.iter().copied()).expect("live nodes");
        let lease = SemanticDeliveryLease::from_roots(roots, store.retention_account());
        (lease, ids)
    });
    assert_eq!(
        harness.live_nodes(),
        NODES,
        "the delivery outlives its request"
    );
    let metadata = lease.own_charge_bytes_for_tests();
    assert!(
        metadata
            > heap_block_bytes(
                NODES * std::mem::size_of::<crate::semantic_query::SemanticNodeId>()
            ),
        "the owner's block and its spilled operand list are charged: {metadata}"
    );
    assert_eq!(
        caller_pinned() - before,
        metadata,
        "the delivery's metadata is charged once"
    );

    let clones: Vec<_> = (0..50).map(|_| lease.clone()).collect();
    assert_eq!(
        caller_pinned() - before,
        metadata,
        "clones share the one owner and its one charge"
    );
    assert!(clones
        .iter()
        .all(|clone| clone.own_charge_bytes_for_tests() == metadata));

    harness.run(|store| store.invalidate_all());
    harness.run(|_| {
        for id in &ids {
            assert!(
                clones[0].node_ref(*id).expect("delivered").read().is_ok(),
                "a later request reads every delivered node"
            );
        }
    });
    assert_eq!(harness.live_nodes(), NODES);

    drop(lease);
    assert_eq!(
        harness.live_nodes(),
        NODES,
        "a clone still holds the delivery"
    );
    drop(clones);
    assert_eq!(
        harness.live_nodes(),
        0,
        "the last holder releases the nodes"
    );
    assert_eq!(
        caller_pinned(),
        before,
        "the delivery's metadata charge leaves with its last holder"
    );
}

/// Every kind of holder of a root set too long to keep inline charges the
/// set's shared block exactly once — the bytes the block's cell and list
/// really occupy — and a clone of the holder, or of the set, adds no charge
/// and no count: a delivery, a keyed root attachment, a kept union view and
/// a construction scope capturing a cache hit.
#[test]
fn every_holder_of_a_long_root_set_charges_its_block_once_and_clones_add_nothing() {
    use super::delivery_lease::SemanticDeliveryLease;
    use super::node_roots::{root_set_charge_bytes_for_tests as block_bytes, IdentityNodeRoots};
    use crate::semantic_query::composite::{CompositeList, UnionKind};
    use crate::semantic_query::semantic_context::SemanticContext;
    use crate::semantic_query::stable_key::semantic_union_members;
    use verter_session_query::retention::allocation::AliasSet;
    use verter_session_query::retention::owned::ChargeCtx;

    const OPERANDS: usize = 6;
    let account = account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let scope = store.enter_node_root_scope();
    let members: Vec<_> = (0..OPERANDS - 1)
        .map(|index| {
            store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                "member-{index}"
            ))))
        })
        .collect();
    let union = store.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::authored_shell(Arc::from(members.clone())),
    ));
    let mut ids = members.clone();
    ids.push(union);
    // The nodes outlive their construction scope; with no scope open, a
    // delivery's clones are captured by no construction owner.
    let _kept = store.root_nodes(ids.iter().copied()).expect("live");
    drop(scope);
    // Everything the account holds beyond the store's own storage.
    let charged = || account.snapshot().total_bytes() - store.store_storage_pins_for_tests();
    let first = ids[0];
    let counts = || store.node_owner_count_for_tests(first);
    assert!(block_bytes(OPERANDS) > 0 && block_bytes(4) == 0);

    // A delivery: its metadata is its owner's block plus the shared block.
    let base = charged();
    let short = SemanticDeliveryLease::from_roots(
        store.root_nodes([first]).expect("live"),
        store.retention_account(),
    );
    let owner_block = charged() - base;
    assert_eq!(owner_block, short.own_charge_bytes_for_tests());
    drop(short);
    let before = counts();
    let lease = SemanticDeliveryLease::from_roots(
        store.root_nodes(ids.iter().copied()).expect("live"),
        store.retention_account(),
    );
    assert_eq!(
        charged() - base,
        owner_block + block_bytes(OPERANDS),
        "a delivery charges its owner's block and its set's shared block, once"
    );
    assert_eq!(counts(), before + 1);
    let lease_clones: Vec<_> = (0..8).map(|_| lease.clone()).collect();
    assert_eq!(charged() - base, owner_block + block_bytes(OPERANDS));
    assert_eq!(counts(), before + 1, "a delivery's clones share its counts");
    drop(lease_clones);
    drop(lease);
    assert_eq!(charged(), base);
    assert_eq!(counts(), before);

    // A keyed attachment: its one cell, which owns the long list's block.
    let attachment_bytes = super::node_roots::attachment_charge_bytes(OPERANDS);
    let attachment = IdentityNodeRoots::in_graph(&store, ids.iter().copied());
    let pin = |attachment: &IdentityNodeRoots| {
        let mut aliases = AliasSet::new();
        assert_eq!(attachment.owned_heap_bytes(&mut aliases), 0);
        aliases.take_handles().pin(ChargeCtx::new(&account));
    };
    pin(&attachment);
    assert_eq!(
        charged() - base,
        attachment_bytes,
        "an attachment charges its one cell, owning its list, once"
    );
    let attachment_clones: Vec<_> = (0..8).map(|_| attachment.clone()).collect();
    for clone in &attachment_clones {
        pin(clone);
    }
    assert_eq!(charged() - base, attachment_bytes);
    assert_eq!(counts(), before + 1, "a key's clones share its counts");
    drop(attachment_clones);
    drop(attachment);
    assert_eq!(charged(), base);
    assert_eq!(counts(), before);

    // A kept union view: its entry, the shared block of the set rooting the
    // union and every member, and the entry's hold on that block.
    let view_bytes = |view: &[crate::semantic_query::SemanticNodeId]| {
        super::union_views::retained_view_bytes(view)
            + verter_session_query::retention::owned::HeldHandles::backing_bytes_for(1)
            + block_bytes(OPERANDS)
    };
    let _reading = store.enter_node_root_scope();
    let ctx = SemanticContext::production();
    let view = semantic_union_members(&store, union, &ctx);
    assert_eq!(view.len(), OPERANDS - 1);
    assert_eq!(
        charged() - base,
        view_bytes(&view),
        "a kept view charges its entry and its set's shared block, once"
    );
    // A hit hands the open scope a share of the view's set: no charge, no
    // count, however often it is read.
    let kept = counts();
    for _ in 0..8 {
        assert_eq!(semantic_union_members(&store, union, &ctx), view);
    }
    assert_eq!(charged() - base, view_bytes(&view));
    assert_eq!(counts(), kept, "scope captures share the view's counts");
}
