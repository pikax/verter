//! A receipt's canonical summary shares its structure with the receipts it
//! consumed: the memory a whole receipt graph keeps alive grows with the
//! graph, never with every prefix of it.
//!
//! Its own test binary, so the counting allocator below observes only this
//! test's allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use verter_session_query::facts::receipt::ResultReceipt;
use verter_session_query::facts::version::FactVersionRef;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call forwards to the system allocator unchanged; the
// counter only observes the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn whole(canonical: String, byte: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical,
        hash: [byte; 16],
    }
}

/// The bytes a shape of `n` levels keeps alive with every receipt held.
fn retained(n: usize, build: fn(usize) -> Vec<ResultReceipt>) -> usize {
    let before = LIVE.load(Ordering::Relaxed);
    let held = build(n);
    let bytes = LIVE.load(Ordering::Relaxed) - before;
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
