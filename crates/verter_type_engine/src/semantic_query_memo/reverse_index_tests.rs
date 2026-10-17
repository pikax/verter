use std::sync::Arc;

use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

use super::{ReceiptReverseIndex, ReverseIndexOccupancy};

type Index = ReceiptReverseIndex<u32, ()>;

fn file(canonical: &str) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_owned(),
        hash: [7; 16],
    }
}

fn canonical(i: usize) -> String {
    format!("/w/chain{i}.ts")
}

/// `n` results, each reading its own file and consuming the one before.
fn chain(n: usize) -> Vec<ResultReceipt> {
    let mut receipts: Vec<ResultReceipt> = Vec::with_capacity(n);
    for i in 0..n {
        let mut facts = vec![file(&canonical(i))];
        if let Some(below) = receipts.last() {
            facts.push(FactVersionRef::Receipt(below.clone()));
        }
        receipts.push(ResultReceipt::new(facts));
    }
    receipts
}

fn carrier(receipt: &ResultReceipt) -> Arc<[FactVersionRef]> {
    Arc::from(vec![FactVersionRef::Receipt(receipt.clone())])
}

fn reached(index: &Index, canonical: &str) -> Vec<u32> {
    let mut keys: Vec<u32> = index
        .reached_by(canonical)
        .into_iter()
        .map(|(key, ())| key)
        .collect();
    keys.sort_unstable();
    keys
}

/// Every result of a chain cached: each file, each consumed edge, each
/// carrier and each candidate is registered once — `4n - 1` edges, where a
/// per-candidate canonical closure would register `n(n + 1) / 2`.
#[test]
fn a_fully_cached_chain_registers_each_edge_once() {
    for n in [32usize, 64] {
        let receipts = chain(n);
        let index = Index::default();
        for (i, receipt) in receipts.iter().enumerate() {
            index.register(i as u32, (), &carrier(receipt), []);
        }
        let occupancy = index.occupancy();
        assert_eq!(
            occupancy,
            ReverseIndexOccupancy {
                canonicals: n,
                evidence: 2 * n,
                candidates: n,
                edges: 4 * n - 1,
            }
        );
        assert_eq!(index.edge_ops().linked, (4 * n - 1) as u64);
        // The base file reaches every candidate; the top file only its own.
        assert_eq!(reached(&index, &canonical(0)).len(), n);
        assert_eq!(reached(&index, &canonical(n - 1)), vec![(n - 1) as u32]);
    }
}

/// Only the chain's top result cached: every uncached intermediate is
/// still indexed, once, so editing the base file still reaches the top.
#[test]
fn uncached_intermediates_are_indexed_once() {
    let receipts = chain(48);
    let index = Index::default();
    index.register(7, (), &carrier(receipts.last().unwrap()), []);
    let occupancy = index.occupancy();
    assert_eq!(occupancy.evidence, 49);
    assert_eq!(occupancy.edges, 48 + 47 + 2);
    for i in 0..48 {
        assert_eq!(reached(&index, &canonical(i)), vec![7]);
    }
    assert!(reached(&index, "/w/unrelated.ts").is_empty());
}

/// Registration work grows linearly with the chain, cached or not.
#[test]
fn chain_registration_work_grows_linearly() {
    let linked = |n: usize| {
        let receipts = chain(n);
        let index = Index::default();
        for (i, receipt) in receipts.iter().enumerate() {
            index.register(i as u32, (), &carrier(receipt), []);
        }
        index.edge_ops().linked
    };
    let (small, large) = (linked(64), linked(256));
    assert!(
        large <= small * 4 + 4,
        "4x the chain must cost at most 4x the registrations: {small} -> {large}"
    );
}

/// A diamond's shared base is registered once and walked once.
#[test]
fn a_diamond_registers_its_shared_base_once() {
    let base = ResultReceipt::new(vec![file("/w/base.ts")]);
    let left = ResultReceipt::new(vec![
        file("/w/left.ts"),
        FactVersionRef::Receipt(base.clone()),
    ]);
    let right = ResultReceipt::new(vec![
        file("/w/right.ts"),
        FactVersionRef::Receipt(base.clone()),
    ]);
    let top = ResultReceipt::new(vec![
        file("/w/top.ts"),
        FactVersionRef::Receipt(left.clone()),
        FactVersionRef::Receipt(right.clone()),
    ]);
    let index = Index::default();
    index.register(1, (), &carrier(&top), []);
    index.register(2, (), &carrier(&left), []);
    let occupancy = index.occupancy();
    // Four files; four receipts and one carrier per candidate; the four
    // consumed edges; and each candidate's carrier and candidate edges.
    assert_eq!(occupancy.canonicals, 4);
    assert_eq!(occupancy.evidence, 6);
    assert_eq!(occupancy.edges, 4 + 4 + 2 + 2);
    assert_eq!(reached(&index, "/w/base.ts"), vec![1, 2]);
    assert_eq!(reached(&index, "/w/right.ts"), vec![1]);
    let walked = index.edge_ops().walked;
    index.reached_by("/w/base.ts");
    assert_eq!(
        index.edge_ops().walked - walked,
        8,
        "base, left, right, top, both carriers and both candidates, each once"
    );
}

/// Releasing one candidate keeps evidence a sibling still reaches; the
/// last release drains every node, edge and charged byte.
#[test]
fn shared_evidence_survives_its_sibling_and_drains_with_the_last_candidate() {
    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let receipts = chain(16);
    let index = Index::charged(&account);
    index.register(1, (), &carrier(&receipts[15]), ["/w/fence.ts"]);
    index.register(2, (), &carrier(&receipts[9]), []);
    assert!(index.storage_charge_bytes() > 0);

    assert_eq!(index.unregister(&1), Some(()));
    assert_eq!(reached(&index, &canonical(0)), vec![2]);
    assert!(reached(&index, &canonical(15)).is_empty());
    assert!(reached(&index, "/w/fence.ts").is_empty());
    assert_eq!(index.occupancy().evidence, 11);

    assert_eq!(index.unregister(&2), Some(()));
    assert_eq!(index.unregister(&2), None);
    assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
    assert_eq!(index.storage_charge_bytes(), 0);
    let ops = index.edge_ops();
    assert_eq!(ops.linked, ops.unlinked);
}

/// Re-registering a key replaces its edges; a payload update that keeps
/// nothing unregisters it.
#[test]
fn replacing_and_updating_a_registration_keeps_edges_exact() {
    let index: ReceiptReverseIndex<u32, Vec<u8>> = ReceiptReverseIndex::default();
    let first = ResultReceipt::new(vec![file("/w/first.ts")]);
    let second = ResultReceipt::new(vec![file("/w/second.ts")]);
    index.register(1, vec![1, 2], &carrier(&first), ["/w/fence.ts"]);
    index.register(1, vec![1, 2], &carrier(&second), []);
    assert!(index.reached_by("/w/first.ts").is_empty());
    assert!(index.reached_by("/w/fence.ts").is_empty());
    assert_eq!(index.reached_by("/w/second.ts").len(), 1);

    assert!(index.update(&1, |slots| {
        slots.retain(|slot| *slot != 1);
        !slots.is_empty()
    }));
    assert_eq!(index.reached_by("/w/second.ts")[0].1, vec![2]);
    assert!(index.update(&1, |slots| {
        slots.clear();
        false
    }));
    assert!(!index.update(&1, |_| true));
    assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
}

/// Concurrent registrations and releases over shared evidence leave every
/// surviving candidate reachable and drain to empty with the last release.
#[test]
fn concurrent_registration_and_release_preserve_and_drain() {
    let receipts = Arc::new(chain(24));
    let index = Arc::new(Index::default());
    let threads: Vec<_> = (0..4u32)
        .map(|worker| {
            let (receipts, index) = (Arc::clone(&receipts), Arc::clone(&index));
            std::thread::spawn(move || {
                for round in 0..50u32 {
                    let key = worker * 1000 + round;
                    let receipt = &receipts[(key as usize * 7) % receipts.len()];
                    index.register(key, (), &carrier(receipt), []);
                    if round % 2 == 1 {
                        assert_eq!(index.unregister(&key), Some(()));
                    }
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let survivors: Vec<u32> = (0..4u32)
        .flat_map(|worker| {
            (0..50u32)
                .filter(|r| r % 2 == 0)
                .map(move |r| worker * 1000 + r)
        })
        .collect();
    assert_eq!(reached(&index, &canonical(0)), survivors);
    for key in &survivors {
        assert_eq!(index.unregister(key), Some(()));
    }
    assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
}

/// A candidate whose carrier names no canonical and with no extra
/// canonical registers nothing.
#[test]
fn a_candidate_no_canonical_reaches_registers_nothing() {
    let index = Index::default();
    index.register(1, (), &Arc::from(Vec::new()), []);
    index.register(
        2,
        (),
        &Arc::from(vec![FactVersionRef::ProjectGeneration { generation: 3 }]),
        [],
    );
    let occupancy = index.occupancy();
    assert_eq!(occupancy.canonicals, 0);
    assert_eq!(occupancy.candidates, 1);
    index.clear();
    assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
}

/// Memo candidates over every fourth result of a chain: the chain's
/// uncached results are indexed once, an edit reaches exactly the
/// candidates above it, and the last eviction drains the index and its
/// charge.
#[test]
fn memo_candidates_evict_through_uncached_receipts_and_drain_the_index() {
    use crate::semantic_query::{
        LiteralValue, RelateMemoKey, RelationContext, RelationOutcome, SemanticNodeData,
    };
    use verter_session_query::facts::fact_cache::ReadSetSignature;
    use verter_session_query::retention::StoreAccount;

    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let store = super::super::SemanticGraphStore::with_account(StoreAccount::new(account));
    let receipts = chain(32);
    let cached: Vec<usize> = (0..32).filter(|i| i % 4 == 3).collect();
    for &i in &cached {
        let source = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
            "s{i}"
        ))));
        let target = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
            "t{i}"
        ))));
        store.insert_relation_payload_for_tests(
            RelateMemoKey::assignable(source, target, RelationContext::default()),
            ReadSetSignature::new(carrier(&receipts[i])),
            Arc::from(Vec::new()),
            store.relation_payload_for_tests(RelationOutcome::Assignable),
            0,
        );
    }
    let occupancy = store.reverse_index_occupancy();
    assert_eq!(
        occupancy,
        ReverseIndexOccupancy {
            canonicals: 32,
            evidence: 32 + cached.len(),
            candidates: cached.len(),
            edges: 32 + 31 + 2 * cached.len(),
        }
    );
    assert_eq!(
        store.canonical_to_entries_count(&canonical(0)),
        cached.len()
    );
    assert_eq!(store.canonical_to_entries_count(&canonical(29)), 1);

    // Results 23, 27 and 31 consumed result 20.
    assert_eq!(store.invalidate_canonical(&canonical(20)), 3);
    assert_eq!(store.canonical_to_entries_count(&canonical(0)), 5);
    assert_eq!(store.reverse_index_occupancy().evidence, 20 + 5);
    assert_eq!(store.invalidate_canonical(&canonical(20)), 0);

    assert_eq!(store.invalidate_canonical(&canonical(0)), 5);
    assert_eq!(
        store.reverse_index_occupancy(),
        ReverseIndexOccupancy::default()
    );
    assert_eq!(store.canonical_to_entries.storage_charge_bytes(), 0);
    let ops = store.reverse_index_edge_ops();
    assert_eq!(ops.linked, ops.unlinked);
}
