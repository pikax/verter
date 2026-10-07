use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Counting global allocator. Increments [`ALLOC_COUNTER`] on every
/// allocating call (`alloc` / `alloc_zeroed` / `realloc`), adds the
/// requested size to [`ALLOC_BYTES`], and delegates to the system
/// allocator.
struct CountingAllocator;

thread_local! {
    /// Allocation count for the current harness thread. A `const`
    /// initializer keeps allocator access allocation-free.
    static ALLOC_COUNTER: Cell<u64> = const { Cell::new(0) };
    /// Bytes REQUESTED from the allocator on the current harness
    /// thread. Tracked beside the call count because the two answer
    /// different questions: a container that grows geometrically to
    /// size `n` costs `O(log n)` allocator CALLS but `O(n)` BYTES, so a
    /// regression that rebuilds a whole-file buffer once per
    /// declaration is near-invisible in the call count and quadratic in
    /// the byte count.
    static ALLOC_BYTES: Cell<u64> = const { Cell::new(0) };
    /// Bytes allocated and not yet freed on the current harness thread,
    /// and the highest that count reached since the last reset: the
    /// live construction a measured window holds at its peak.
    static LIVE_BYTES: Cell<i64> = const { Cell::new(0) };
    static PEAK_LIVE_BYTES: Cell<i64> = const { Cell::new(0) };
}

fn increment_alloc_counter(size: usize) {
    // Allocation can occur while a thread is tearing down TLS. Do not
    // turn an otherwise valid allocation into a panic if this key is
    // no longer accessible.
    let _ = ALLOC_COUNTER.try_with(|counter| counter.set(counter.get().wrapping_add(1)));
    let _ = ALLOC_BYTES.try_with(|bytes| bytes.set(bytes.get().wrapping_add(size as u64)));
    add_live_bytes(size as i64);
}

fn add_live_bytes(delta: i64) {
    let _ = LIVE_BYTES.try_with(|live| {
        let now = live.get().wrapping_add(delta);
        live.set(now);
        let _ = PEAK_LIVE_BYTES.try_with(|peak| {
            if now > peak.get() {
                peak.set(now);
            }
        });
    });
}

fn alloc_count() -> u64 {
    ALLOC_COUNTER.with(Cell::get)
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        add_live_bytes(-(layout.size() as i64));
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A grow-in-place realloc still REQUESTS `new_size` bytes; the
        // byte counter records requests, not net residency; the live
        // count trades the old block for the new one.
        add_live_bytes(-(layout.size() as i64));
        increment_alloc_counter(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

mod canary_absolutize_already_absolute_zero_alloc {
    //! Allocation canary for `SemanticTypeSource::absolutized_against`
    //! over a LARGE already-absolute surface.
    //!
    //! The absolutization walker is copy-on-first-change: scanning an
    //! already-absolute source performs NO clones and NO heap allocation
    //! (the dominant case — a fallthrough source re-absolutized under a
    //! consuming scope). The pre-fix walker eagerly cloned every member
    //! into a fresh `Vec` before learning nothing changed, so this canary
    //! reads a per-member allocation delta there and zero here.

    use std::hint::black_box;
    use std::sync::Arc;

    use verter_type_expr::facts::{
        ClosedTypeFact, ObjectMemberFact, ObjectPropertyFact, ObjectShapeFact, SemanticTypeSource,
    };
    use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, TypeBodySlot};
    use verter_type_expr::span_origins::{MemberSpansOrigin, SourceSynthetic};
    use verter_type_expr::MemberVisibility;

    use super::alloc_count;

    fn absolute_member(index: usize) -> ObjectMemberFact {
        ObjectMemberFact::Property(ObjectPropertyFact {
            key: verter_type_expr::facts::FactAuthoredPropertyKey::string(format!("member{index}")),
            optional: false,
            readonly: false,
            visibility: MemberVisibility::Public,
            ty: TypeBodySlot {
                // ALREADY-ABSOLUTE anchor: nothing to rewrite.
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from("/already/absolute.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from("Anchored"),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
            span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
        })
    }

    #[test]
    fn absolutizing_a_large_already_absolute_surface_allocates_nothing() {
        const MEMBERS: usize = 256;
        let members: Vec<ObjectMemberFact> = (0..MEMBERS).map(absolute_member).collect();
        let source = SemanticTypeSource::Closed(ClosedTypeFact::Object(ObjectShapeFact {
            members: Arc::from(members.into_boxed_slice()),
        }));

        // Warm any lazy init, then measure the walk alone.
        let _ = black_box(source.absolutized_against("/consumer.vue"));
        let baseline = alloc_count();
        let rewritten = source.absolutized_against("/consumer.vue");
        let after = alloc_count();
        black_box(&rewritten);
        assert_eq!(source, rewritten, "already-absolute input round-trips");
        let delta = after - baseline;
        assert_eq!(
            delta, 0,
            "absolutizing a {MEMBERS}-member already-absolute surface must \
             be allocation-free (copy-on-first-change) — a walker that \
             eagerly clones the members into a Vec before discovering \
             nothing changed reports a per-member delta here; observed {delta}"
        );
    }
}
