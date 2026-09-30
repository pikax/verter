use std::sync::Arc;

use super::*;

fn whole(canonical: &str, byte: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: [byte; 16],
    }
}

/// A receipt's evidence is its own facts plus the receipts it consumed:
/// equal evidence is one receipt however it was assembled, and the
/// summaries reach through consumed receipts without copying their facts.
#[test]
fn a_receipt_holds_its_own_facts_and_summarizes_what_it_consumed() {
    let leaf = ResultReceipt::new(vec![whole("/a.ts", 1), whole("/b.ts", 2)]);
    let parent = ResultReceipt::new(vec![
        FactVersionRef::Receipt(leaf.clone()),
        whole("/c.ts", 3),
    ]);
    assert_eq!(parent.facts().len(), 2, "one own fact and one receipt");
    let canonicals: Vec<&str> = parent.canonicals().iter().map(AsRef::as_ref).collect();
    assert_eq!(canonicals, ["/a.ts", "/b.ts", "/c.ts"]);
    let again = ResultReceipt::new(vec![
        whole("/c.ts", 3),
        FactVersionRef::Receipt(ResultReceipt::new(vec![
            whole("/b.ts", 2),
            whole("/a.ts", 1),
        ])),
    ]);
    assert!(!again.ptr_eq(&parent), "premise: assembled apart");
    assert_eq!(again, parent, "equal evidence is one receipt");
    assert_eq!(again.digest(), parent.digest());
    let other = ResultReceipt::new(vec![whole("/c.ts", 4)]);
    assert_ne!(other, parent);
}

/// The leaves reachable through a shared evidence graph are visited once
/// per distinct receipt: a diamond visits its shared child's facts once.
#[test]
fn a_receipt_walk_visits_a_shared_child_once() {
    let shared = ResultReceipt::new(vec![whole("/s.ts", 1)]);
    let left = ResultReceipt::new(vec![
        FactVersionRef::Receipt(shared.clone()),
        whole("/l.ts", 2),
    ]);
    let right = ResultReceipt::new(vec![FactVersionRef::Receipt(shared), whole("/r.ts", 3)]);
    let top = ResultReceipt::new(vec![
        FactVersionRef::Receipt(left),
        FactVersionRef::Receipt(right),
    ]);
    let mut visited = Vec::new();
    assert!(top.all_leaves(|fact| {
        visited.push(fact.canonical_id().map(str::to_owned));
        true
    }));
    visited.sort();
    assert_eq!(
        visited,
        [
            Some("/l.ts".into()),
            Some("/r.ts".into()),
            Some("/s.ts".into())
        ],
        "each fact once"
    );
    let mut seen = 0;
    assert!(
        !top.all_leaves(|_| {
            seen += 1;
            false
        }),
        "a refused leaf stops the walk"
    );
    assert_eq!(seen, 1);
}

/// Two evidences whose digests collide are still different receipts: the
/// digest orders and hashes, it never decides equality alone.
#[test]
fn a_digest_collision_never_makes_two_evidences_equal() {
    let a = ResultReceipt::new(vec![whole("/a.ts", 1)]);
    let forged = ResultReceipt(Arc::new(ResultEvidence {
        facts: Arc::from(vec![whole("/z.ts", 9)]),
        digest: a.digest(),
        canonicals: ResultReceipt::new(vec![whole("/z.ts", 9)])
            .canonicals()
            .clone(),
        aggregated: Arc::from(Vec::new()),
        resolution_evidence: false,
    }));
    assert_ne!(a, forged);
    assert_ne!(a.cmp(&forged), std::cmp::Ordering::Equal);
}

/// A chain's evidence grows with its length, never with its prefixes:
/// each level holds its own fact and one receipt.
#[test]
fn a_chain_of_receipts_holds_each_level_once() {
    let mut below = ResultReceipt::new(vec![whole("/0.ts", 0)]);
    for level in 1..2_000u32 {
        below = ResultReceipt::new(vec![
            FactVersionRef::Receipt(below),
            whole(&format!("/{}.ts", level % 7), (level % 251) as u8),
        ]);
        assert!(below.facts().len() <= 2);
    }
    let mut leaves = 0usize;
    assert!(below.all_leaves(|_| {
        leaves += 1;
        true
    }));
    assert_eq!(leaves, 2_000, "every level's fact once");
    assert_eq!(below.canonicals().len(), 7);
}

/// A chain of receipts far longer than a native stack could drop one frame
/// per level is dropped on a 1 MiB thread.
#[test]
fn a_long_chain_of_receipts_drops_without_native_recursion() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let mut below = ResultReceipt::new(vec![whole("/0.ts", 0)]);
            for level in 1..200_000u32 {
                below = ResultReceipt::new(vec![
                    FactVersionRef::Receipt(below),
                    whole("/n.ts", (level % 251) as u8),
                ]);
            }
            drop(below);
        })
        .expect("spawn the dropping thread")
        .join()
        .expect("the chain drops");
}

/// Two equal chains assembled apart, far longer than a native stack could
/// compare one frame per level, compare equal on a 1 MiB thread; one
/// differing fact at the bottom orders them apart.
#[test]
fn equal_chains_assembled_apart_compare_without_native_recursion() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            // Each level's digest is `digests[level]` when given, so a
            // chain can forge another's digests all the way down.
            let chain = |bottom: u8, digests: Option<&[u128]>| {
                let mut seen = Vec::new();
                let mut below: Option<ResultReceipt> = None;
                for level in 0..100_000u32 {
                    let mut facts = vec![whole("/n.ts", (level % 251) as u8)];
                    match below.take() {
                        Some(below) => facts.push(FactVersionRef::Receipt(below)),
                        None => facts = vec![whole("/0.ts", bottom)],
                    }
                    let built = ResultReceipt::new(facts);
                    seen.push(built.digest());
                    below = Some(match digests {
                        None => built,
                        Some(digests) => ResultReceipt(Arc::new(ResultEvidence {
                            facts: Arc::clone(&built.0.facts),
                            digest: digests[level as usize],
                            canonicals: built.0.canonicals.clone(),
                            aggregated: Arc::clone(&built.0.aggregated),
                            resolution_evidence: false,
                        })),
                    });
                }
                (below.expect("a chain has a top"), seen)
            };
            let (a, digests) = chain(0, None);
            let (b, _) = chain(0, None);
            assert!(!a.ptr_eq(&b), "premise: assembled apart");
            assert_eq!(a, b);
            assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
            // Equal digests at every level over a different bottom: only
            // the bottom tells them apart.
            let (forged, _) = chain(1, Some(&digests));
            assert_eq!(forged.digest(), a.digest(), "premise: digests forged");
            assert_ne!(a, forged);
            assert_ne!(a.cmp(&forged), std::cmp::Ordering::Equal);
        })
        .expect("spawn the comparing thread")
        .join()
        .expect("the chains compare");
}

/// A walk that is told a receipt is already validated reads nothing only
/// that receipt reaches, and asks about each distinct receipt once.
#[test]
fn a_walk_skips_the_receipts_it_is_told_are_settled() {
    let shared = ResultReceipt::new(vec![whole("/s.ts", 1)]);
    let left = ResultReceipt::new(vec![
        FactVersionRef::Receipt(shared.clone()),
        whole("/l.ts", 2),
    ]);
    let right = ResultReceipt::new(vec![
        FactVersionRef::Receipt(shared.clone()),
        whole("/r.ts", 3),
    ]);
    let top = ResultReceipt::new(vec![
        FactVersionRef::Receipt(left.clone()),
        FactVersionRef::Receipt(right),
    ]);
    let mut asked = 0usize;
    let mut visited = Vec::new();
    assert!(ReceiptWalk::default().leaves_unless(
        &top,
        |met| {
            asked += 1;
            met.ptr_eq(&left)
        },
        |fact| {
            visited.push(fact.canonical_id().map(str::to_owned));
            true
        },
    ));
    visited.sort();
    assert_eq!(visited, [Some("/r.ts".into()), Some("/s.ts".into())]);
    assert_eq!(asked, 4, "top, left, right and shared, once each");
}

/// Two equal lattices assembled apart, each level's two receipts
/// consuming both of the level below, compare in their distinct pairs of
/// receipts: the paths through them double per level and are never
/// followed one by one.
#[test]
fn equal_lattices_assembled_apart_compare_in_their_distinct_pairs() {
    let lattice = || {
        let (mut left, mut right) = (
            ResultReceipt::new(vec![whole("/l.ts", 0)]),
            ResultReceipt::new(vec![whole("/r.ts", 0)]),
        );
        for level in 1..64u8 {
            let below = [
                FactVersionRef::Receipt(left.clone()),
                FactVersionRef::Receipt(right.clone()),
            ];
            left = ResultReceipt::new([below.to_vec(), vec![whole("/l.ts", level)]].concat());
            right = ResultReceipt::new([below.to_vec(), vec![whole("/r.ts", level)]].concat());
        }
        ResultReceipt::new(vec![
            FactVersionRef::Receipt(left),
            FactVersionRef::Receipt(right),
        ])
    };
    let (a, b) = (lattice(), lattice());
    assert!(!a.ptr_eq(&b), "premise: assembled apart");
    assert_eq!(a, b);
    assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
}

/// A receipt's canonical set is exactly the canonicals its reachable facts
/// name, in order: through a long chain of distinct files, through a
/// diamond, and never a canonical nothing reaches. A receipt naming only
/// canonicals its consumed receipt already holds shares that set.
#[test]
fn a_receipts_canonical_set_is_exactly_what_it_reaches() {
    let mut chain = ResultReceipt::new(vec![whole("/0.ts", 0)]);
    for level in 1..1_024u32 {
        chain = ResultReceipt::new(vec![
            FactVersionRef::Receipt(chain),
            whole(&format!("/{level}.ts"), (level % 251) as u8),
        ]);
    }
    assert_eq!(chain.canonicals().len(), 1_024);
    assert!(chain.references_canonical("/0.ts"), "the deepest level");
    assert!(chain.references_canonical("/1023.ts"), "the top level");
    assert!(!chain.references_canonical("/unrelated.ts"));
    let mut walked = std::collections::BTreeSet::new();
    assert!(chain.all_leaves(|fact| {
        walked.insert(fact.canonical_id().expect("a file fact").to_owned());
        true
    }));
    let listed: Vec<String> = chain.canonicals().iter().map(|c| c.to_string()).collect();
    assert_eq!(
        listed,
        walked.into_iter().collect::<Vec<_>>(),
        "the walk's canonicals, in order"
    );

    let shared = ResultReceipt::new(vec![whole("/s.ts", 1)]);
    let left = ResultReceipt::new(vec![
        FactVersionRef::Receipt(shared.clone()),
        whole("/l.ts", 2),
    ]);
    let right = ResultReceipt::new(vec![
        FactVersionRef::Receipt(shared.clone()),
        whole("/r.ts", 3),
    ]);
    let top = ResultReceipt::new(vec![
        FactVersionRef::Receipt(left),
        FactVersionRef::Receipt(right),
    ]);
    let listed: Vec<&str> = top.canonicals().iter().map(AsRef::as_ref).collect();
    assert_eq!(listed, ["/l.ts", "/r.ts", "/s.ts"]);

    let again = ResultReceipt::new(vec![
        FactVersionRef::Receipt(shared.clone()),
        whole("/s.ts", 4),
    ]);
    assert_eq!(
        again.canonicals(),
        shared.canonicals(),
        "no canonical added"
    );
}
