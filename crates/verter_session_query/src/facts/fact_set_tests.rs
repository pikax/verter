use std::collections::BTreeSet;

use super::*;

fn fact(index: usize) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("/fact-{index:05}.ts").into(),
        hash: [(index & 0xff) as u8; 16],
    }
}

fn hash_of(set: &FactSet) -> u64 {
    use std::hash::{DefaultHasher, Hasher};
    let mut hasher = DefaultHasher::new();
    set.hash(&mut hasher);
    hasher.finish()
}

/// A small deterministic generator, so the randomized cases replay.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % bound
    }
}

#[test]
fn one_content_is_one_set_whatever_its_history() {
    let forward: Vec<_> = (0..200).map(fact).collect();
    let direct = crate::facts::policy::FactPolicy::production().set_of(forward.clone());
    // Built link by link, each link the one below plus one fact.
    let mut chained = crate::facts::policy::FactPolicy::production().empty_set();
    for index in (0..200).rev() {
        chained = chained.union_with_facts(&[fact(index)]);
    }
    // Built from shuffled halves with repetitions.
    let mut halves = crate::facts::policy::FactPolicy::production()
        .set_of((100..200).rev().map(fact).collect::<Vec<_>>());
    halves = halves.union_with_facts(&(0..150).map(fact).collect::<Vec<_>>());
    assert!(
        chained.tree_storage().is_some(),
        "an extended wide set is a tree"
    );
    assert!(direct.tree_storage().is_none(), "a fresh seal is one slice");
    for set in [&chained, &halves] {
        assert_eq!(set, &direct);
        assert_eq!(set.cmp(&direct), std::cmp::Ordering::Equal);
        assert_eq!(set.digest(), direct.digest());
        assert_eq!(hash_of(set), hash_of(&direct));
        assert_eq!(set.len(), 200);
        assert!(set.iter().eq(forward.iter()));
        assert!(set.iter().rev().eq(forward.iter().rev()));
    }
    let mut removed = chained.edit();
    removed.remove(&fact(7));
    let removed = removed.finish();
    assert_ne!(removed, direct);
    assert_eq!(removed.union_with_facts(&[fact(7)]), direct);
}

#[test]
fn equal_digests_still_compare_content() {
    let left = crate::facts::policy::FactPolicy::production()
        .set_of((0..64).map(fact).collect::<Vec<_>>());
    let right = crate::facts::policy::FactPolicy::production()
        .set_of((1..65).map(fact).collect::<Vec<_>>());
    assert_eq!(left.len(), right.len());
    assert_ne!(left, right);
}

/// The tree answers exactly what an ordered set would, through every
/// reader: membership, forward, backward and mixed iteration, a range from
/// any fact, the extremes and the length, across random insertions and
/// removals into shared and unshared trees.
#[test]
fn the_tree_reads_exactly_as_an_ordered_set() {
    let mut rng = Lcg(7);
    let mut oracle: BTreeSet<FactVersionRef> = BTreeSet::new();
    let mut set = crate::facts::policy::FactPolicy::production().empty_set();
    let mut history: Vec<(FactSet, BTreeSet<FactVersionRef>)> = Vec::new();
    for round in 0..600 {
        let mut edit = set.edit();
        for _ in 0..rng.next(6) + 1 {
            let candidate = fact(rng.next(700));
            if rng.next(4) == 0 {
                assert_eq!(edit.remove(&candidate), oracle.remove(&candidate));
            } else {
                assert_eq!(edit.insert(candidate.clone()), oracle.insert(candidate));
            }
        }
        set = edit.finish();
        assert_eq!(set.len(), oracle.len(), "round {round}");
        assert!(set.iter().eq(oracle.iter()), "round {round}: forward");
        assert!(
            set.iter().rev().eq(oracle.iter().rev()),
            "round {round}: backward"
        );
        assert_eq!(set.first(), oracle.first());
        assert_eq!(set.last(), oracle.last());
        let probe = fact(rng.next(700));
        assert_eq!(set.contains(&probe), oracle.contains(&probe));
        assert!(
            set.iter_from(&probe).eq(oracle.range(probe.clone()..)),
            "round {round}: range from a fact"
        );
        // Mixed ends meet exactly once.
        let mut iter = set.iter();
        let mut seen = Vec::new();
        let mut from_back = Vec::new();
        loop {
            match (iter.next(), iter.next_back()) {
                (Some(front), Some(back)) => {
                    seen.push(front.clone());
                    from_back.push(back.clone());
                }
                (Some(front), None) => {
                    seen.push(front.clone());
                    break;
                }
                (None, Some(back)) => {
                    from_back.push(back.clone());
                    break;
                }
                (None, None) => break,
            }
        }
        seen.extend(from_back.into_iter().rev());
        assert!(seen.iter().eq(oracle.iter()), "round {round}: mixed ends");
        if round % 50 == 0 {
            history.push((set.clone(), oracle.clone()));
        }
    }
    // Every earlier set is untouched by the edits built on it.
    for (earlier, expected) in &history {
        assert!(earlier.iter().eq(expected.iter()));
    }
}

#[test]
fn a_chain_of_sets_shares_its_storage_and_charges_each_link_its_own_part() {
    let links = 1000;
    let mut sets: Vec<FactSet> = Vec::with_capacity(links);
    let mut set = crate::facts::policy::FactPolicy::production()
        .set_of((0..40).map(fact).collect::<Vec<_>>());
    for index in 40..40 + links {
        let next = set.union_with_facts(&[fact(index)]);
        if set.tree_storage().is_some() {
            let (own, shared) = next.tree_nodes_against(&set);
            assert!(own <= 8, "link {index} copied {own} nodes");
            assert!(shared > 0, "link {index} shares the link below");
        }
        set = next;
        sets.push(set.clone());
    }
    let mut charged = 0;
    for pair in sets.windows(2) {
        charged += pair[1].tree_charge_beyond(&pair[0]);
    }
    let flat_copies: usize = sets
        .iter()
        .map(|set| set.len() * std::mem::size_of::<FactVersionRef>())
        .sum();
    // Every link pays for its path, never for a copy of the chain below it.
    assert!(
        charged * 10 < flat_copies,
        "charged {charged} bytes for a chain whose flat copies hold {flat_copies}"
    );
}

/// A tree node is charged exactly its own allocations, measured from the
/// node itself. (The charge lives in the node, so it is released with the
/// node's allocation; the allocator canaries prove both against the real
/// allocator.)
#[test]
fn every_node_charges_exactly_its_own_bytes() {
    let base = crate::facts::policy::FactPolicy::production()
        .set_of((0..500).map(fact).collect::<Vec<_>>());
    let mut sets = vec![base.union_with_facts(&[fact(10_000)])];
    for index in 0..50 {
        let next = sets[index].union_with_facts(&[fact(20_000 + index)]);
        sets.push(next);
    }
    let _charged: usize = {
        let mut nodes: Vec<Arc<Node>> = Vec::new();
        for set in &sets {
            for node in set.tree_node_list() {
                if !nodes.iter().any(|held| Arc::ptr_eq(held, &node)) {
                    nodes.push(node);
                }
            }
        }
        for node in &nodes {
            assert_eq!(
                node.charged_bytes(),
                super::fact_tree::node_bytes(node, false),
                "a node charges exactly its own bytes"
            );
        }
        nodes.iter().map(|node| node.charged_bytes()).sum()
    };
}

/// A wide set has no slice view and no public copy: a reader reads it in
/// place or not at all. A source guard keeps a flat-copy path from being
/// added back unnoticed.
#[test]
fn a_tree_set_offers_no_flat_copy() {
    let set = crate::facts::policy::FactPolicy::production()
        .set_of((0..10).map(fact).collect::<Vec<_>>())
        .union_with_facts(&(10..100).map(fact).collect::<Vec<_>>());
    assert!(set.tree_storage().is_some());
    assert!(set.as_flat().is_none(), "a tree set has no slice");
    let source = include_str!("fact_set.rs");
    for forbidden in [
        "impl std::ops::Deref for FactSet",
        "impl Deref for FactSet",
        "impl AsRef<[FactVersionRef]> for FactSet",
        "impl std::borrow::Borrow<[FactVersionRef]> for FactSet",
        "pub fn as_slice",
    ] {
        assert!(
            !source.contains(forbidden),
            "a flat view of a tree set came back: `{forbidden}`"
        );
    }
    let to_vec = source
        .find("pub fn to_vec")
        .expect("to_vec exists for tests");
    assert!(
        source[..to_vec]
            .trim_end()
            .ends_with("#[cfg(any(test, feature = \"test-support\"))]\n    #[must_use]")
            || source[to_vec.saturating_sub(120)..to_vec]
                .contains("#[cfg(any(test, feature = \"test-support\"))]"),
        "to_vec stays test support"
    );
}

/// CPU direction of the tree against a flat sorted slice: iteration,
/// membership, a chain of one-fact links and a union of two wide siblings.
/// A measurement, not a check:
/// `cargo test --release -p verter_session_query --lib fact_set_cpu -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, not a check"]
fn fact_set_cpu_direction() {
    use std::time::Instant;
    fn per<F: FnMut() -> usize>(rounds: usize, mut f: F) -> f64 {
        let mut sink: usize = 0;
        let start = Instant::now();
        for _ in 0..rounds {
            sink = sink.wrapping_add(f());
        }
        std::hint::black_box(sink);
        start.elapsed().as_secs_f64() * 1e9 / rounds as f64
    }
    for n in [100usize, 1_000, 10_000] {
        let facts: Vec<FactVersionRef> = (0..n).map(fact).collect();
        let flat: Arc<[FactVersionRef]> = Arc::from(facts.clone());
        let tree = crate::facts::policy::FactPolicy::production()
            .set_of(facts[..1].to_vec())
            .union_with_facts(&facts[1..]);
        let rounds = (2_000_000 / n).max(20);
        let flat_iter = per(rounds, || {
            flat.iter()
                .filter(|f| matches!(f, FactVersionRef::FileWholeHash { .. }))
                .count()
        });
        let tree_iter = per(rounds, || {
            tree.iter()
                .filter(|f| matches!(f, FactVersionRef::FileWholeHash { .. }))
                .count()
        });
        let probes: Vec<FactVersionRef> = (0..n).step_by((n / 64).max(1)).map(fact).collect();
        let flat_find = per(rounds, || {
            probes
                .iter()
                .filter(|p| flat.binary_search(p).is_ok())
                .count()
        });
        let tree_find = per(rounds, || {
            probes.iter().filter(|p| tree.contains(p)).count()
        });
        let links = n.min(2_000);
        let base: Vec<FactVersionRef> = (0..40).map(fact).collect();
        let start = Instant::now();
        let mut flat_link: Arc<[FactVersionRef]> = Arc::from(base.clone());
        for index in 0..links {
            let mut next = flat_link.to_vec();
            let extra = fact(100_000 + index);
            let at = next.partition_point(|held| *held < extra);
            next.insert(at, extra);
            flat_link = Arc::from(next);
        }
        let flat_chain = start.elapsed().as_secs_f64() * 1e9 / links as f64;
        let start = Instant::now();
        let mut tree_link = crate::facts::policy::FactPolicy::production().set_of(base);
        for index in 0..links {
            tree_link = tree_link.union_with_facts(&[fact(100_000 + index)]);
        }
        let tree_chain = start.elapsed().as_secs_f64() * 1e9 / links as f64;
        assert!(tree_link.iter().eq(flat_link.iter()));
        println!(
            "n={n}: iterate flat {flat_iter:.0} ns tree {tree_iter:.0} ns ({:.1}x); 64 lookups flat {flat_find:.0} ns tree {tree_find:.0} ns ({:.1}x); chain link ({links} links) flat {flat_chain:.0} ns tree {tree_chain:.0} ns ({:.3}x)",
            tree_iter / flat_iter,
            tree_find / flat_find,
            tree_chain / flat_chain,
        );
    }
}

/// A policy narrow enough that small fixtures cross every storage boundary
/// a production-width set crosses: a fresh seal past 40 facts and an
/// extension past 4 are trees of several leaves under a branch.
fn narrow_policy() -> crate::facts::policy::FactPolicy {
    crate::facts::policy::FactPolicy::with_widths(crate::facts::policy::FactWidths {
        fresh_slice_max: 40,
        strict_root_threshold: 40,
        precise_domain_max: 40,
        extension_slice_max: 4,
    })
    .expect("valid widths")
}

/// A narrow policy changes only which storage a set takes, at exactly its
/// own widths, along both construction histories: a fresh seal is one slice
/// up to the fresh limit and a charged multi-node tree one fact beyond it;
/// an extension is one slice up to the extension limit and a tree one fact
/// beyond it, shrinking back to a slice when an edit narrows it again.
/// Every set built from another inherits the policy.
#[test]
fn a_narrow_policy_moves_both_storage_boundaries_and_nothing_else() {
    let policy = narrow_policy();
    let at_fresh = policy.set_of((0..40).map(fact).collect::<Vec<_>>());
    let past_fresh = policy.set_of((0..41).map(fact).collect::<Vec<_>>());
    assert!(
        at_fresh.as_flat().is_some(),
        "a fresh seal at the limit is one slice"
    );
    assert!(
        past_fresh.tree_storage().is_some(),
        "one fact past it is a tree"
    );
    let footprint = past_fresh.tree_footprint();
    assert!(
        footprint.nodes > 1,
        "the tree spans several leaves under a branch: {footprint:?}"
    );
    assert!(
        past_fresh.tree_node_charges() > 0,
        "its nodes charge their own allocations from birth"
    );
    // The same facts under production widths are one slice, and equal.
    let production = crate::facts::policy::FactPolicy::production()
        .set_of((0..41).map(fact).collect::<Vec<_>>());
    assert!(production.as_flat().is_some());
    assert_eq!(
        production, past_fresh,
        "content defines equality, not storage"
    );
    assert_eq!(production.digest(), past_fresh.digest());
    assert_eq!(hash_of(&production), hash_of(&past_fresh));

    let base = policy.empty_set();
    let at_extension = base.union_with_facts(&(0..4).map(fact).collect::<Vec<_>>());
    let past_extension = at_extension.union_with_facts(&[fact(4)]);
    assert!(
        at_extension.as_flat().is_some(),
        "an extension at its limit is one slice"
    );
    assert!(
        past_extension.tree_storage().is_some(),
        "one fact past it is a tree"
    );
    assert!(past_extension.tree_node_charges() > 0);
    let mut narrowed = past_extension.edit();
    narrowed.remove(&fact(4));
    let narrowed = narrowed.finish();
    assert!(
        narrowed.as_flat().is_some(),
        "narrowed back, it is one slice again"
    );
    assert_eq!(narrowed, at_extension);
    for set in [
        &at_fresh,
        &past_fresh,
        &at_extension,
        &past_extension,
        &narrowed,
    ] {
        assert_eq!(
            set.policy(),
            policy,
            "every set built here inherits the policy"
        );
    }
    assert_eq!(past_fresh.edit().finish().policy(), policy);
}

/// Sets of different policies compare by content, but combining them is a
/// defect: it is refused before the shared-storage shortcut could answer.
#[test]
#[should_panic(expected = "different fact policies")]
fn combining_sets_of_different_policies_is_refused_before_any_shortcut() {
    let narrow = narrow_policy().set_of((0..8).map(fact).collect::<Vec<_>>());
    let production =
        crate::facts::policy::FactPolicy::production().set_of((0..8).map(fact).collect::<Vec<_>>());
    let _ = production.is_superset(&narrow);
}
