use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::{InternDomain, Interned, RETAINED_SLOT_FLOOR};

/// The bound a drained index's capacity returns to, independent of how
/// many records it once indexed.
const DRAINED_CAPACITY_BOUND: usize = RETAINED_SLOT_FLOOR * 4;

// Each test interns its own kind, so a concurrently running test can never
// perturb another's record or capacity counts.

#[derive(Debug, PartialEq, Eq, Hash)]
struct Plain(u64);
intern_domain!(Plain {
    /// A `u64`: inline.
    fn value_heap_bytes(&self) -> usize {
        0
    }
});

#[derive(Debug, PartialEq, Eq, Hash)]
struct Shared(u64);
intern_domain!(Shared {
    /// A `u64`: inline.
    fn value_heap_bytes(&self) -> usize {
        0
    }
});

/// A value that counts its own destruction.
struct Counted {
    key: u64,
    drops: Arc<AtomicUsize>,
}
intern_domain!(Counted {
    /// The shared drop counter's block.
    fn value_heap_bytes(&self) -> usize {
        verter_session_query::retention::allocation::arc_block_bytes(std::mem::size_of::<
            AtomicUsize,
        >())
    }
});

impl PartialEq for Counted {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Counted {}
impl Hash for Counted {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}
impl Drop for Counted {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

/// A value whose every instance shares one digest.
#[derive(Debug, PartialEq, Eq)]
struct Colliding(u64);
intern_domain!(Colliding {
    /// A `u64`: inline.
    fn value_heap_bytes(&self) -> usize {
        0
    }
});
impl Hash for Colliding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0u64.hash(state);
    }
}

/// A second all-colliding kind, so the bucket-capacity probe is not
/// perturbed by the aliasing test's records.
#[derive(Debug, PartialEq, Eq)]
struct CollidingPeer(u64);
intern_domain!(CollidingPeer {
    /// A `u64`: inline.
    fn value_heap_bytes(&self) -> usize {
        0
    }
});
impl Hash for CollidingPeer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0u64.hash(state);
    }
}

/// A kind half of whose values collide on one digest, observed only
/// through the production occupancy accessor.
#[derive(Debug, PartialEq, Eq)]
struct Observed(u64);
intern_domain!(Observed {
    /// A `u64`: inline.
    fn value_heap_bytes(&self) -> usize {
        0
    }
});
impl Hash for Observed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (if self.0.is_multiple_of(2) { 0 } else { self.0 }).hash(state);
    }
}

/// A parent record that owns child handles of its own kind.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Tree {
    Leaf(u64),
    Node(Vec<Interned<Tree>>),
}
intern_domain!(Tree {
    /// A node's child list; each child is its own record.
    fn value_heap_bytes(&self) -> usize {
        match self {
            Tree::Leaf(_) => 0,
            Tree::Node(children) => verter_session_query::retention::allocation::vec_bytes(children),
        }
    }
});

#[test]
fn content_equal_values_share_one_record() {
    let a = Interned::new(Plain(7));
    let b = Interned::new(Plain(7));
    let c = Interned::new(Plain(8));
    assert!(a.same_record(&b));
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(Plain::index().len(), 2);
    assert_eq!(Plain::index().get(&Plain(7)).as_ref(), Some(&a));
    assert!(Plain::index().get(&Plain(9)).is_none());
}

#[test]
fn churn_returns_records_and_index_capacity_to_baseline() {
    let drops = Arc::new(AtomicUsize::new(0));
    let churn = 20_000u64;
    let held: Vec<_> = (0..churn)
        .map(|key| {
            Interned::new(Counted {
                key,
                drops: Arc::clone(&drops),
            })
        })
        .collect();
    let index = Counted::index();
    assert_eq!(index.len(), churn as usize);
    let high_water = index.capacity();
    assert!(high_water >= churn as usize);
    drop(held);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        churn as usize,
        "every record is destroyed once its owners drain"
    );
    assert!(index.is_empty(), "a drained index retains no entry");
    assert!(
        index.capacity() <= DRAINED_CAPACITY_BOUND,
        "a drained index returns its backing capacity: {} of a {high_water} high water",
        index.capacity()
    );
}

#[test]
fn digest_collisions_never_alias_distinct_values() {
    let one = Interned::new(Colliding(1));
    let two = Interned::new(Colliding(2));
    assert_ne!(one, two, "a shared digest is not shared identity");
    assert!(Interned::new(Colliding(1)).same_record(&one));
    drop(one);
    assert_eq!(
        Colliding::index().len(),
        1,
        "only the dropped record leaves the bucket"
    );
    assert_eq!(
        *Colliding::index().get(&Colliding(2)).expect("still live"),
        Colliding(2)
    );
}

#[test]
fn a_live_collision_peer_does_not_pin_its_drained_bucket_capacity() {
    let index = CollidingPeer::index();
    let sentinel = Interned::new(CollidingPeer(0));
    let peers: Vec<_> = (1..=2_000)
        .map(|key| Interned::new(CollidingPeer(key)))
        .collect();
    let high_water = index
        .bucket_capacity(&CollidingPeer(0))
        .expect("one bucket");
    assert!(high_water > 2_000);
    drop(peers);
    assert_eq!(index.len(), 1, "only the sentinel stays indexed");
    let retained = index
        .bucket_capacity(&CollidingPeer(0))
        .expect("sentinel's bucket");
    assert!(
        retained <= 4,
        "the surviving bucket returns its backing capacity: {retained} of a {high_water} high water"
    );
    drop(sentinel);
    assert!(index.is_empty());
}

#[test]
fn held_nested_children_outlive_their_minting_handles_and_drain_with_the_parent() {
    let index = Tree::index();
    let parent = {
        let left = Interned::new(Tree::Leaf(1));
        let right = Interned::new(Tree::Leaf(2));
        Interned::new(Tree::Node(vec![left, right]))
    };
    assert_eq!(index.len(), 3, "the parent keeps both children resident");
    let Tree::Node(children) = parent.value() else {
        panic!("parent is a node");
    };
    assert_eq!(*children[0], Tree::Leaf(1));
    assert!(
        Interned::new(Tree::Leaf(2)).same_record(&children[1]),
        "a re-intern resolves to the child the parent holds"
    );
    // A duplicate parent built from fresh child handles dedups onto the
    // live parent; releasing the duplicate frees nothing the parent owns.
    let duplicate = Interned::new(Tree::Node(vec![
        Interned::new(Tree::Leaf(1)),
        Interned::new(Tree::Leaf(2)),
    ]));
    assert!(duplicate.same_record(&parent));
    drop(duplicate);
    assert_eq!(index.len(), 3);
    drop(parent);
    assert!(
        index.is_empty(),
        "dropping the parent releases its children"
    );
}

#[test]
fn concurrent_intern_and_release_converge_and_drain() {
    std::thread::scope(|scope| {
        for worker in 0..8u64 {
            scope.spawn(move || {
                for round in 0..2_000u64 {
                    let shared = Interned::new(Shared(round % 16));
                    let own = Interned::new(Shared(1_000_000 + worker * 10_000 + round));
                    assert_eq!(*shared, Shared(round % 16));
                    assert!(Interned::new(Shared(round % 16)).same_record(&shared));
                    drop((shared, own));
                }
            });
        }
    });
    assert!(Shared::index().is_empty());
    assert!(Shared::index().capacity() <= DRAINED_CAPACITY_BOUND);
}

#[test]
fn occupancy_reports_held_records_and_both_backing_capacities_until_owners_drain() {
    let index = Observed::index();
    let held: Vec<_> = (0..1_000u64)
        .map(|key| Interned::new(Observed(key)))
        .collect();
    let live = index.occupancy();
    assert_eq!(live.records, 1_000, "every held record is indexed");
    assert_eq!(
        live.digests, 501,
        "500 distinct digests plus one collision digest"
    );
    assert!(live.digest_capacity >= live.digests);
    assert!(
        live.collision_entry_capacity >= 500,
        "the shared digest's spilled bucket is counted: {live:?}"
    );
    drop(held);
    let drained = index.occupancy();
    assert_eq!((drained.records, drained.digests), (0, 0), "{drained:?}");
    assert_eq!(drained.collision_entry_capacity, 0, "{drained:?}");
    assert!(
        drained.digest_capacity <= DRAINED_CAPACITY_BOUND,
        "the digest map returns its backing capacity: {drained:?} after {live:?}"
    );
}

/// A record is charged its block and its value's own heap for exactly as
/// long as a handle names it, and the index charges its own backing by
/// capacity, giving it back as the index drains.
#[test]
fn a_record_and_its_index_backing_are_charged_until_the_last_handle_drops() {
    use verter_session_query::retention::allocation::{arc_block_bytes, vec_bytes};
    const CHILDREN: usize = 4_096;
    let children: Vec<Interned<Tree>> = (0..CHILDREN as u64)
        .map(|leaf| Interned::new(Tree::Leaf(leaf + 1_000_000)))
        .collect();
    let list = vec_bytes(&children);
    let parent = Interned::new(Tree::Node(children));
    let record = arc_block_bytes(std::mem::size_of::<super::InternRecord<Tree>>());
    assert_eq!(
        parent.record_charge_bytes(),
        record + list,
        "the parent is charged its block and its child list"
    );
    let leaf = Interned::new(Tree::Leaf(1_000_000));
    assert_eq!(
        leaf.record_charge_bytes(),
        record,
        "a leaf is only its block"
    );
    let backing = Tree::index().backing_charge_bytes();
    assert!(
        backing
            >= Tree::index().capacity() * (std::mem::size_of::<(u64, super::Bucket<Tree>)>() + 1),
        "the index's table is charged by capacity: {backing}"
    );
    drop(leaf);
    drop(parent);
    assert!(Tree::index().is_empty());
    assert_eq!(
        Tree::index().backing_charge_bytes(),
        0,
        "a drained index keeps no backing charge"
    );
}
