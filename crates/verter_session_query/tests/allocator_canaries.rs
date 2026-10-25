//! A receipt's canonical summary shares its structure with the receipts it
//! consumed: the memory a whole receipt graph keeps alive grows with the
//! graph, never with every prefix of it.
//!
//! Its own test binary, so the counting allocator below observes only this
//! test's allocations.

use std::alloc::{GlobalAlloc, Layout, System};

use verter_session_query::facts::receipt::ResultReceipt;
use verter_session_query::facts::version::FactVersionRef;

struct Counting;

thread_local! {
    /// The bytes this thread allocated and has not freed, for a measurement
    /// the other tests of this binary cannot disturb.
    static THREAD_LIVE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn thread_live() -> isize {
    THREAD_LIVE.with(std::cell::Cell::get)
}

fn note(delta: isize) {
    // `try_with`: the allocator runs during thread teardown too.
    let _ = THREAD_LIVE.try_with(|live| live.set(live.get() + delta));
}

// SAFETY: every call forwards to the system allocator unchanged; the
// counter only observes the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn whole(canonical: String, byte: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.into(),
        hash: [byte; 16],
    }
}

/// The bytes a shape of `n` levels keeps alive with every receipt held.
fn retained(n: usize, build: fn(usize) -> Vec<ResultReceipt>) -> usize {
    let before = thread_live();
    let held = build(n);
    let bytes = usize::try_from(thread_live() - before).expect("the shape allocated");
    drop(held);
    bytes
}

/// A chain whose every level names one new canonical.
fn distinct_chain(n: usize) -> Vec<ResultReceipt> {
    let mut held = Vec::with_capacity(n);
    let mut below = ResultReceipt::new(vec![whole("/0.ts".into(), 0)]);
    held.push(below.clone());
    for level in 1..n {
        below = ResultReceipt::new(vec![
            FactVersionRef::Receipt(below),
            whole(format!("/{level}.ts"), (level % 251) as u8),
        ]);
        held.push(below.clone());
    }
    held
}

/// A chain whose every level names the same canonical.
fn same_file_chain(n: usize) -> Vec<ResultReceipt> {
    let mut held = Vec::with_capacity(n);
    let mut below = ResultReceipt::new(vec![whole("/n.ts".into(), 0)]);
    held.push(below.clone());
    for level in 1..n {
        below = ResultReceipt::new(vec![
            FactVersionRef::Receipt(below),
            whole("/n.ts".into(), (level % 251) as u8),
        ]);
        held.push(below.clone());
    }
    held
}

/// A ladder of diamonds: each level joins two receipts that share the
/// level below, each side naming one new canonical.
fn diamond_ladder(n: usize) -> Vec<ResultReceipt> {
    let mut held = Vec::with_capacity(3 * n);
    let mut below = ResultReceipt::new(vec![whole("/d0.ts".into(), 0)]);
    held.push(below.clone());
    for level in 1..n {
        let left = ResultReceipt::new(vec![
            FactVersionRef::Receipt(below.clone()),
            whole(format!("/l{level}.ts"), 1),
        ]);
        let right = ResultReceipt::new(vec![
            FactVersionRef::Receipt(below),
            whole(format!("/r{level}.ts"), 2),
        ]);
        below = ResultReceipt::new(vec![
            FactVersionRef::Receipt(left.clone()),
            FactVersionRef::Receipt(right.clone()),
        ]);
        held.extend([left, right, below.clone()]);
    }
    held
}

/// Doubling a shape's size at most multiplies the memory it keeps alive by
/// a near-linear factor (`n log n` gives about 2.2); a summary copying
/// every prefix multiplies it by about 4.
#[test]
fn receipt_summaries_grow_near_linearly_with_the_graph() {
    for (name, build) in [
        (
            "distinct-file chain",
            distinct_chain as fn(usize) -> Vec<ResultReceipt>,
        ),
        ("same-file chain", same_file_chain),
        ("diamond ladder", diamond_ladder),
    ] {
        let sizes = [128usize, 256, 512, 1024];
        let bytes: Vec<usize> = sizes.iter().map(|&n| retained(n, build)).collect();
        for pair in sizes.iter().zip(&bytes).collect::<Vec<_>>().windows(2) {
            let ((small, small_bytes), (large, large_bytes)) = (pair[0], pair[1]);
            let ratio = *large_bytes as f64 / *small_bytes as f64;
            assert!(
                ratio < 3.0,
                "{name}: {small} -> {large} levels kept {small_bytes} -> {large_bytes} bytes \
                 alive (x{ratio:.2}); all sizes {bytes:?}"
            );
        }
        eprintln!("{name}: bytes kept alive at {sizes:?} = {bytes:?}");
    }
}

/// A tree set's retention charge, compared with the real allocator: every
/// node charges at least the bytes its allocations really take, and at most
/// one allocator granule and header more per block (the declared allocation
/// convention every charge uses), so the charge is exact under that
/// convention and never an undercharge. When the last set holding a node
/// drops, every byte comes back: the charge lives in the node and is
/// released with it.
#[test]
fn a_fact_tree_charges_exactly_what_it_allocates() {
    use verter_session_query::facts::fact_set::FactSet;
    use verter_session_query::retention::allocation::{
        ALLOCATION_GRANULE_BYTES, ALLOCATION_HEADER_BYTES,
    };
    // Facts that own no heap, so the bytes measured are the trees' alone.
    let fact = |generation: u64| FactVersionRef::ProjectGeneration { generation };
    let base = FactSet::from_facts((0..2_000).map(fact).collect());
    let mut held: Vec<FactSet> = Vec::with_capacity(300);
    let before = thread_live();
    // The first extension builds the tree; each later link adds one fact
    // and every seventh also removes one, sharing the rest.
    let mut set = base.union_with_facts(&[fact(1_000_000)]);
    for link in 0..300u64 {
        let mut edit = set.edit();
        edit.insert(fact(2_000_000 + link));
        if link % 7 == 0 {
            edit.remove(&fact(link * 3));
        }
        set = edit.finish();
        held.push(set.clone());
    }
    drop(set);
    let allocated = usize::try_from(thread_live() - before).expect("the trees allocated");
    let footprint = FactSet::tree_footprint_of(&held);
    let headers = held.len() * FactSet::header_bytes();
    // `held`'s own buffer was allocated before the measurement began.
    let held_buffer = held.capacity() * std::mem::size_of::<FactSet>();
    let real = allocated;
    let charged = footprint.charged_bytes + headers;
    let slack =
        (footprint.blocks + held.len()) * (ALLOCATION_GRANULE_BYTES + ALLOCATION_HEADER_BYTES);
    assert!(
        charged >= real,
        "never an undercharge: charged {charged} < allocated {real}"
    );
    assert!(
        charged <= real + slack,
        "exact within the allocation convention: charged {charged}, allocated {real},          slack {slack} over {} blocks",
        footprint.blocks
    );
    drop(held);
    assert_eq!(
        thread_live(),
        before - held_buffer as isize,
        "every node's bytes, and so its charge, come back when the last set drops"
    );
    eprintln!(
        "fact tree: allocated {real} bytes, charged {charged} over {} nodes",
        footprint.nodes
    );
}
