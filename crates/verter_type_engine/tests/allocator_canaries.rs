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

fn reset_alloc_counter() {
    ALLOC_COUNTER.with(|counter| counter.set(0));
    ALLOC_BYTES.with(|bytes| bytes.set(0));
    LIVE_BYTES.with(|live| live.set(0));
    PEAK_LIVE_BYTES.with(|peak| peak.set(0));
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

fn alloc_bytes() -> u64 {
    ALLOC_BYTES.with(|counter| counter.get())
}

mod canary_flow_return_audit_emission_zero_alloc {
    //! Cold-vs-warm audit contract, allocation half
    //! (`U6.FLOW_RETURN_SUBSTRATE` exit acceptance): without an
    //! installed accumulator the flow-return audit emission helpers
    //! allocate NOTHING — no event payload, no detail string, no
    //! boxed variant. The warm family hit never reaches the helpers
    //! at all (the behavioral half, pinned by
    //! `warm_hit_emits_no_flow_return_started_event` in
    //! `tests/cases/g_type/flow_return_audit_contract.rs`); this
    //! canary pins that even the COLD-path helpers construct no
    //! audit payload when no accumulator is installed, so a request
    //! with audit off / footprint off pays zero audit allocation on
    //! the flow path.
    //!
    //! Discrimination: a regression that builds a payload BEFORE the
    //! accumulator gate — a `format!` detail, a `String::from`
    //! canonical, a boxed event — reports ≥ 1 allocation per call
    //! and pushes the 10 000-iteration delta to ≥ 10 000. The
    //! companion test proves the counter responds to a real
    //! per-iteration allocation through the same helpers' argument
    //! shape, so the zero cannot be vacuous.
    //!
    //! Measurement isolation: no request context and no accumulator
    //! are installed on this harness thread; the helpers' TLS probes
    //! are warmed before the measured window so lazy TLS init is not
    //! attributed to the loop.

    use std::hint::black_box;
    use std::sync::Arc;

    use verter_session_query::flow::peeker::{FlowSliceBudgetAxis, FlowSliceBudgetExceeded};
    use verter_type_engine::flow_return_audit::{
        record_flow_cycle_reentry, record_flow_return_started, record_flow_slice_budget_exceeded,
    };

    use super::alloc_count;

    #[test]
    fn emission_helpers_allocate_nothing_without_accumulator() {
        // Setup phase — argument construction allocates and is NOT
        // counted toward the measured delta.
        let canonical: Arc<str> = Arc::from("/w/flow-canary.ts");
        let symbol: Arc<str> = Arc::from("makeThing");
        let exceeded = FlowSliceBudgetExceeded {
            axis: FlowSliceBudgetAxis::SelectedNodes,
            limit: 4096,
            observed: 4097,
        };

        // Warm the TLS probes (request-context slot, accumulator
        // slot) so one-time lazy initialisation is pre-paid.
        for _ in 0..64 {
            record_flow_return_started(&canonical, &symbol);
            record_flow_slice_budget_exceeded(&exceeded);
            record_flow_cycle_reentry(1, &symbol);
        }

        let baseline = alloc_count();
        const ITERATIONS: usize = 10_000;
        for i in 0..ITERATIONS {
            record_flow_return_started(&canonical, &symbol);
            record_flow_slice_budget_exceeded(&exceeded);
            record_flow_cycle_reentry(i as u32, &symbol);
        }
        let after = alloc_count();
        let delta = after - baseline;
        assert_eq!(
            delta, 0,
            "flow-return audit emission helpers must allocate NOTHING without an \
             installed accumulator (the cold-vs-warm audit contract's \
             no-audit-payload half). A helper that builds a payload before the \
             accumulator gate — a format!() detail, a String canonical, a boxed \
             event — reports ≥ {ITERATIONS} here; observed {delta} over \
             {ITERATIONS} iterations of all three helpers."
        );
    }

    /// Discrimination companion: the same loop shape, with a real
    /// per-iteration payload allocation of the kind the gate must
    /// prevent. The counter must observe it — proving the zero above
    /// is a measured zero, not a dead counter.
    #[test]
    fn discrimination_companion_ungated_payload_is_observed() {
        let canonical: Arc<str> = Arc::from("/w/flow-canary.ts");
        let symbol: Arc<str> = Arc::from("makeThing");
        for i in 0..32 {
            let _ = black_box(format!("warmup-{i}"));
        }
        let baseline = alloc_count();
        const ITERATIONS: usize = 1_000;
        for _ in 0..ITERATIONS {
            // Exactly the payload an ungated helper would build.
            let detail = format!("{canonical}::{symbol}");
            black_box(detail);
        }
        let after = alloc_count();
        let delta = after - baseline;
        assert!(
            delta >= ITERATIONS as u64,
            "companion: a per-iteration format!() payload must be observed by the \
             counting allocator (≥ {ITERATIONS} allocations); got {delta}. A zero \
             here means the counter is not wired and the zero-allocation canary \
             above proves nothing."
        );
    }
}

#[path = "allocation_cases/flow_literal_provenance.rs"]
mod flow_literal_provenance_allocation;

mod signature_kernel_warm_positional {
    //! Warm positional Empty/One read: no per-candidate `Arc` clone and no
    //! intern-shard lock. The counting allocator is process-global in this
    //! binary; the measured window is the repeated read after the fixture is
    //! interned.

    use std::hint::black_box;

    use verter_type_engine::signature_kernel::test_support::{
        warm_positional_read, warm_positional_read_many, WarmPositionalLockProbe,
        WarmPositionalStore,
    };

    use super::{alloc_count, reset_alloc_counter};

    #[test]
    fn warm_positional_read_does_not_allocate_or_lock() {
        let fixture = WarmPositionalStore::fixture();
        let _ = warm_positional_read(&fixture);

        reset_alloc_counter();
        // bounded-loop: repeated warm positional reads of interned One and Many.
        for _ in 0..10_000 {
            black_box(warm_positional_read(&fixture));
            black_box(warm_positional_read_many(&fixture));
        }
        let allocations = alloc_count();
        assert_eq!(
            allocations, 0,
            "warm positional read allocated {allocations} times (per-candidate Arc clone?)"
        );

        let WarmPositionalLockProbe {
            acquires_before,
            acquires_after,
        } = fixture.lock_probe();
        assert_eq!(
            acquires_after, acquires_before,
            "warm positional read acquired intern-shard locks ({acquires_before} -> {acquires_after})"
        );
    }
}
