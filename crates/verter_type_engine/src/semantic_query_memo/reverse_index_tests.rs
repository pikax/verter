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

/// A candidate holding no fact and naming no extra canonical is not
/// registered; one whose facts name no canonical is, reached by no walk,
/// and drains with its candidate.
#[test]
fn only_a_candidate_with_no_fact_and_no_extra_goes_unregistered() {
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
    assert_eq!(index.unregister(&1), None);
    assert_eq!(index.unregister(&2), Some(()));
    assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
}

/// The bytes the index holds, recounted table by table rather than from
/// its running totals: every table's and every holder set's allocated
/// slots, and the canonicals' text — less the candidates' share when their
/// owner charges it. No table's allocation is ever counted below its live
/// capacity.
fn held_bytes<K: Eq + std::hash::Hash + Clone, P: Clone>(
    index: &ReceiptReverseIndex<K, P>,
) -> usize {
    use super::{Table, Tracked};
    use verter_session_query::retention::allocation::arc_block_bytes;
    fn slots<T: Table>(tracked: &Tracked<T>) -> usize {
        assert!(tracked.slots >= tracked.table.capacity());
        tracked.slots
    }
    let state = index.state.lock();
    let set_slots: usize = state.canonicals.table.values().map(slots).sum::<usize>()
        + state
            .evidence
            .table
            .values()
            .map(|node| slots(&node.parents))
            .sum::<usize>();
    let text: usize = state
        .canonicals
        .table
        .keys()
        .map(|canonical| arc_block_bytes(canonical.len()))
        .sum();
    let held = slots(&state.canonicals) * super::CANONICAL_BUCKET_BYTES
        + slots(&state.evidence) * super::EVIDENCE_BUCKET_BYTES
        + slots(&state.candidate_ids) * super::id_bucket_bytes::<K>()
        + slots(&state.candidates) * super::candidate_bucket_bytes::<K, P>()
        + text
        + set_slots * super::EDGE_BYTES;
    let candidates_share: usize = state
        .candidates
        .table
        .values()
        .map(|node| {
            ReceiptReverseIndex::<K, P>::candidate_bytes(
                node.extras.len(),
                usize::from(node.carrier.is_some()) + usize::from(node.fence.is_some()),
            )
        })
        .sum();
    let extras_lists: usize = state
        .candidates
        .table
        .values()
        .map(|node| node.extras.len() * std::mem::size_of::<Arc<str>>())
        .sum();
    if state.storage.candidates_charged_by_owner {
        held + extras_lists - candidates_share
    } else {
        held + extras_lists
    }
}

/// Many candidates sharing one carrier and one fence, drained in stages:
/// at every stage the account carries every bucket the index still holds,
/// spare ones a partial drain leaves behind included, never more than the
/// charge granule's rounding above it, and nothing once the last candidate
/// leaves. An index whose owner charges nothing of its candidates charges
/// their share as well.
#[test]
fn a_partially_drained_index_keeps_its_spare_capacity_charged() {
    const GRANULE: usize = super::INDEX_STORAGE_GRANULE;
    const N: u32 = 4096;
    let mut charged_at_full = Vec::new();
    for owner_charges_candidates in [true, false] {
        let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
        let index = if owner_charges_candidates {
            Index::charged(&account)
        } else {
            Index::charging_registrations(&account)
        };
        let check = |stage: &str| {
            let (charge, held) = (index.storage_charge_bytes(), held_bytes(&index));
            assert!(
                charge >= held && charge < held + 2 * GRANULE,
                "{stage}: charged {charge} for {held} held bytes"
            );
            assert_eq!(account.snapshot().pinned_bytes, charge, "{stage}");
        };
        let shared = carrier(&chain(4)[3]);
        for key in 0..N {
            index.register(key, (), &shared, ["/w/fence.ts"]);
        }
        check("full");
        charged_at_full.push(index.storage_charge_bytes());
        for key in 0..N / 2 {
            assert_eq!(index.unregister(&key), Some(()));
        }
        check("half drained");
        for key in N / 2..N - 1 {
            assert_eq!(index.unregister(&key), Some(()));
        }
        check("one left");
        assert_eq!(reached(&index, "/w/fence.ts"), vec![N - 1]);
        assert_eq!(index.unregister(&(N - 1)), Some(()));
        assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
        assert_eq!(index.storage_charge_bytes(), 0);
        assert_eq!(account.snapshot().pinned_bytes, 0);
    }
    assert!(charged_at_full[1] > charged_at_full[0]);
}

/// Removals that leave tombstones — which lower a set's `capacity()`
/// without freeing its buckets — still release exactly what was charged:
/// candidates naming one of two fences in a scrambled order, drained in
/// sequence, keep the charge at or above every allocation through the
/// drain and leave nothing charged once the last one leaves.
#[test]
fn a_drain_through_tombstones_releases_every_charged_byte() {
    const GRANULE: usize = super::INDEX_STORAGE_GRANULE;
    const N: u32 = 3500;
    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let index = Index::charging_registrations(&account);
    let empty = crate::fact_signature_helpers::empty_fact_signature();
    let mut random = 0x1234_5678_u64;
    for key in 0..N {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let fence = if random & 1 == 0 {
            "/w/shared.ts"
        } else {
            "/w/other.ts"
        };
        index.register(key, (), &empty, [fence]);
    }
    for key in 0..N {
        assert_eq!(index.unregister(&key), Some(()));
        if key % 500 == 0 {
            let (charge, held) = (index.storage_charge_bytes(), held_bytes(&index));
            assert!(
                charge >= held && charge < held + 2 * GRANULE,
                "after {key}: charged {charge} for {held} held bytes"
            );
        }
    }
    assert_eq!(index.occupancy(), ReverseIndexOccupancy::default());
    assert_eq!(index.storage_charge_bytes(), 0);
    assert_eq!(account.snapshot().pinned_bytes, 0);
}

/// There is no account-less index: a default one charges the process-local
/// account.
#[test]
fn a_default_index_charges_the_process_local_account() {
    let index = Index::default();
    index.register(1, (), &carrier(&chain(2)[1]), []);
    assert!(index.storage_charge_bytes() >= held_bytes(&index));
    assert!(held_bytes(&index) > 0);
    assert!(Arc::ptr_eq(
        &index.state.lock().storage.account,
        &SemanticRetentionAccount::process_local()
    ));
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

/// Candidates sharing one interned dispatch fence register its canonicals
/// once: the fence is one evidence node naming each candidate, so `C`
/// candidates over a `k`-canonical fence cost `k + C` fence edges, where a
/// per-candidate fence would cost `C·k`. An edit to any fence canonical
/// still reaches every candidate, and the last eviction drains the index.
#[test]
fn candidates_sharing_a_dispatch_fence_register_its_canonicals_once() {
    use crate::semantic_query::{
        BinderScopeId, DepEntry, DepSignature, DepVersion, PrimitiveKind, QueryResult,
        ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryKey,
    };
    use verter_session_query::facts::fact_cache::ReadSetSignature;
    use verter_session_query::retention::StoreAccount;

    const FENCE: usize = 8;
    const CANDIDATES: usize = 6;
    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let store = super::super::SemanticGraphStore::with_account(StoreAccount::new(account));
    let carrier = ReadSetSignature::new(Arc::from(vec![file("/w/carrier.ts")]));
    let fence = || {
        DepSignature::new(
            (0..FENCE)
                .map(|j| {
                    DepEntry::new(
                        &format!("/w/fence{j}.ts"),
                        DepVersion::WholeHash([j as u8; 16]),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    for i in 0..CANDIDATES {
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: verter_type_expr::OwnedStr::from("/w/carrier.ts"),
                owner,
                local_scope: None,
                binder_scope_id: BinderScopeId::file_scope(owner),
            },
            name: verter_type_expr::OwnedStr::from(format!("N{i}").as_str()),
        });
        assert_eq!(
            store.publish_with_carrier_and_dispatch_for_tests(
                key,
                QueryResult::Value(node),
                carrier.clone(),
                Arc::from([]),
                fence(),
            ),
            1
        );
    }
    assert_eq!(
        store.reverse_index_occupancy(),
        ReverseIndexOccupancy {
            canonicals: 1 + FENCE,
            evidence: 2,
            candidates: CANDIDATES,
            edges: 1 + FENCE + 2 * CANDIDATES,
        }
    );
    assert_eq!(store.canonical_to_entries_count("/w/fence3.ts"), CANDIDATES);

    assert_eq!(store.invalidate_canonical("/w/fence3.ts"), CANDIDATES);
    assert_eq!(
        store.reverse_index_occupancy(),
        ReverseIndexOccupancy::default()
    );
    assert_eq!(store.canonical_to_entries.storage_charge_bytes(), 0);
    let ops = store.reverse_index_edge_ops();
    assert_eq!(ops.linked, ops.unlinked);
}
