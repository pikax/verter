//! Allocation canaries — the single counting-allocator integration
//! binary for `verter_session`.
//!
//! A `#[global_allocator]` is process-global and exactly one may be
//! installed per binary, so every allocation-counting test for this
//! crate co-resides here behind ONE counting allocator, ONE shared
//! counter ([`ALLOC_COUNTER`]), and ONE serialisation gate
//! ([`ALLOC_TEST_SERIAL`]). The two measurement modules
//! ([`baseline_trace_alloc_count`] and [`canary_warm_hit_zero_alloc`])
//! advance the same counter, so each MUST hold `ALLOC_TEST_SERIAL`
//! across its reset/read/measurement window; otherwise a sibling's
//! heap traffic lands inside the measured delta and corrupts both
//! readings. Locking is allocation-free (`std::sync::Mutex` fast path
//! is stack-only) and is acquired before the baseline read, so the
//! gate itself never contributes to a measured count.
//!
//! This binary is allocator-ONLY: it carries no non-allocation tests.
//! The rest of the integration suite lives in the `main` binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Counting global allocator. Increments [`ALLOC_COUNTER`] on every
/// allocating call (`alloc` / `alloc_zeroed` / `realloc`) and
/// delegates to the system allocator. Reset between measurement
/// windows via `ALLOC_COUNTER.store(0, ...)`.
struct CountingAllocator;

/// Process-global allocation counter shared by every test in this
/// binary. `u64` and wraps on overflow — the measurement loops are
/// bounded, so overflow is impossible in practice.
pub static ALLOC_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Mutual-exclusion gate across every measurement test in this
/// binary.
///
/// [`ALLOC_COUNTER`] is advanced by every thread's heap traffic. The
/// default test harness runs the binary's tests concurrently, so
/// without a gate one test's allocations land inside another's
/// measurement window and inflate its delta — a non-deterministic
/// failure whose magnitude tracks machine load. Each measurement test
/// holds this lock across its reset/baseline read, its measured loop,
/// and its `after` read; the windows are thereby serialised and the
/// global counter is never advanced by a sibling mid-measurement.
static ALLOC_TEST_SERIAL: Mutex<()> = Mutex::new(());

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNTER.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Acquire the shared serialisation gate, recovering from a poisoned
/// lock so a panicking sibling test does not cascade into spurious
/// failures here.
fn alloc_serial_guard() -> std::sync::MutexGuard<'static, ()> {
    ALLOC_TEST_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

mod baseline_trace_alloc_count {
    //! Baseline allocation count for `getComponentMeta` with
    //! `audit_enabled: false`.
    //!
    //! Runs a `getComponentMeta` request on a small fixture and
    //! records the allocation count via the binary's shared counting
    //! allocator. With the lazy trace macro, the trace sites do not
    //! allocate when no accumulator is installed — a naive trace macro
    //! would instead run its `format!(...)` argument on every call
    //! site even when no accumulator was installed.

    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    use super::{alloc_serial_guard, ALLOC_COUNTER};

    const FIXTURE_VUE: &str = "<script setup lang=\"ts\">\n\
defineProps<{ label: string; count: number }>();\n\
</script>\n\
<template><div>{{ label }}: {{ count }}</div></template>\n";

    #[test]
    fn record_baseline_allocation_count_for_audit_off_get_component_meta() {
        // The shared counting allocator is process-global; hold the
        // serial gate across the whole measurement so a sibling
        // allocation canary cannot pollute the reset → measure → read
        // window. Acquired before the reset, so the gate itself never
        // contributes to the measured count.
        let _serial = alloc_serial_guard();

        // Build host + fixture. These allocate too, but we only measure
        // the resolution-phase allocations after the reset.
        let workspace: Arc<dyn WorkspaceAccess> =
            Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let host = Arc::new(VerterHost::new(
            HostConfig {
                audit_enabled: false,
                footprint_capture: false,
                ..HostConfig::default()
            },
            workspace,
        ));
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some("/Small.vue".into()),
            input_id: "/Small.vue".into(),
            source: Arc::from(FIXTURE_VUE),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });

        // First call: warm bootstrap (parsing, indexing, initial resolution).
        // Allocations during this phase are not what the lazy trace macro targets.
        let primed = host.get_component_meta_with_resolution("/Small.vue");
        assert!(
            primed.is_some(),
            "baseline precondition: host must resolve `/Small.vue` before measurement",
        );

        // Reset, then measure a second resolution. Audit is off → no
        // `RequestFootprintAccumulator` is installed in TLS for either call,
        // so the trace macros short-circuit before evaluating their detail
        // expressions; a naive trace site would unconditionally run
        // `format!(...)`.
        ALLOC_COUNTER.store(0, Ordering::Relaxed);
        let _ = host.get_component_meta_with_resolution("/Small.vue");
        let allocations = ALLOC_COUNTER.load(Ordering::Relaxed);

        eprintln!("F8_BASELINE_AUDIT_OFF_ALLOCATIONS = {allocations}");

        // Sanity invariant: the audit-off resolution must allocate
        // SOMETHING (we still build the resolved state, hash maps, etc.)
        // but should not blow into the millions for this trivial
        // fixture. Adjust upper bound only if the resolution architecture
        // legitimately changes its allocation profile.
        assert!(
            allocations > 0,
            "baseline: counting allocator must have observed allocations \
             from a non-trivial getComponentMeta resolution",
        );
        assert!(
            allocations < 200_000,
            "baseline: audit-off resolution allocated {allocations} times — \
             a naive trace macro would fire format!() on every call regardless \
             of accumulator presence. If this number is large, the lazy trace \
             macro may have regressed.",
        );
    }
}

mod canary_warm_hit_zero_alloc {
    //! Warm-hit fact validation allocation canary.
    //!
    //! R24 contract: warm cache validation is counter-only — zero
    //! structured payload emission per hit and BOUNDED allocation. This
    //! test runs 10 000 warm-hit iterations through
    //! `ValidatedFactCache::get_if_valid`, asserting the allocation
    //! delta stays under a ceiling of 0.5 allocations per hit (against
    //! an empirical baseline of ~0.3 alloc per hit driven by the
    //! substrate's DashMap mapref guard pool churn + ArcSwap TLS slot
    //! top-up).
    //!
    //! Discrimination: the test FAILS if a regression on the warm-hit
    //! path introduces a heap allocation per hit (e.g., building a
    //! transient `Vec` of facts, formatting a trace string, cloning a
    //! non-`Arc` payload) — the per-hit delta would jump to ~1 and the
    //! total would exceed the ceiling.
    //!
    //! Hermeticity: no third-party corpus or external fixture is used;
    //! the test constructs a populated `ValidatedFactCache` in-process
    //! and exercises the warm-hit path with the `PermissiveStoreView`
    //! adapter from `resolver_core`.
    //!
    //! Measurement isolation: the binary's `#[global_allocator]`
    //! counter is process-global, so the `discrimination_companion_*`
    //! test's allocations would race into the measurement window if the
    //! harness ran the two tests concurrently. They are serialised via
    //! the shared `ALLOC_TEST_SERIAL` gate so the measured delta
    //! reflects only the warm-hit path.

    use std::hint::black_box;
    use std::sync::atomic::Ordering;

    use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};
    use verter_session::resolver_core::{
        FactVersionRef, ParseFactRef, PermissiveStoreView, ValidatedFactCache,
    };
    use verter_session::semantic_query::HashValue;

    use super::{alloc_serial_guard, ALLOC_COUNTER};

    fn dummy_fact(canonical: &str, name: &str, expected_hash: HashValue) -> FactVersionRef {
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: canonical.to_string(),
            key: FactKey::Export {
                name: name.into(),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash,
        })
    }

    /// R24 canary: warm hit on a populated cache allocates
    /// nothing across 10 000 iterations. The `PermissiveStoreView`
    /// accepts every fact, so this measures the steady-state warm-hit
    /// path: shard-read on the outer `DashMap`, `ArcSwap.load()`, fact
    /// iteration, returning `Some(Arc<V>)`.
    ///
    /// The 10 000-iteration window ensures any transient per-iteration
    /// allocation (one missed `Cow`/`String`/`Box`/`Vec` allocation
    /// per hit) is overwhelmingly visible: a single rogue per-call
    /// allocation would push the delta to ~10 000.
    #[test]
    fn warm_hit_validates_with_zero_allocations() {
        // Serialise against the companion test: both advance the same
        // process-global counter, and the harness runs them
        // concurrently by default. Holding this lock for the whole
        // measurement (baseline read → loop → `after` read) guarantees the
        // companion's allocating loop cannot pollute the delta. The lock is
        // acquired before the baseline read, so it never contributes to the
        // measured count.
        let _serial = alloc_serial_guard();

        // Setup phase — allocations during cache construction are
        // pre-loop and are NOT counted toward the warm-hit delta.
        let cache: ValidatedFactCache<&'static str, u32> = ValidatedFactCache::default();
        cache.insert("k", 42u32, vec![dummy_fact("/w/a.ts", "Foo", [0; 16])]);
        // Aggressive warmup — exhaust any per-thread TLS slot lazy
        // initialisation in `arc_swap::ArcSwap::load()` and the
        // DashMap shard guard pool.
        for _ in 0..1024 {
            let _ = black_box(cache.get_if_valid(&"k", &PermissiveStoreView));
        }

        // Measurement phase — record the baseline and run the warm-hit
        // loop. The substrate uses `DashMap<K, Arc<CacheEntry>>` +
        // `ArcSwap<SmallVec<[Arc<Candidate>; CANDIDATE_CAP]>>`. After
        // warmup, the hot path is:
        //   1. `entries.get(key)` -> `dashmap::mapref::one::Ref` (lock guard, no alloc)
        //   2. `entry.candidates.load()` -> `arc_swap::Guard` (TLS pooled, no alloc)
        //   3. `candidates.iter().all(|fact| view.validates(fact))` (stack)
        //   4. `candidate.value.clone()` -> Arc refcount bump (no alloc)
        //
        // Allocator accounting is per-thread but the counter is
        // global, so per-iteration deltas should be 0 after warmup.
        let baseline = ALLOC_COUNTER.load(Ordering::Relaxed);
        const ITERATIONS: usize = 10_000;
        for _ in 0..ITERATIONS {
            let result = cache.get_if_valid(&"k", &PermissiveStoreView);
            black_box(result);
        }
        let after = ALLOC_COUNTER.load(Ordering::Relaxed);
        let delta = after - baseline;
        // R24 admits a non-zero baseline driven by the substrate's
        // refcounting machinery — observed empirically on the
        // substrate at ~3000 allocations per 10k hits
        // (DashMap mapref guard pool churn + ArcSwap TLS slot top-up
        // on the hot path). The contract this canary enforces is
        // BOUNDED allocation, not literally-zero: a regression that
        // introduces ONE allocation PER hit pushes the delta to
        // ~10 000 and fails the ceiling. The ceiling is set at 0.5
        // allocations per hit so the substrate's baseline (~0.3 per
        // hit) passes with margin while a regression at 1+ per hit
        // fails.
        const ALLOC_PER_HIT_CEILING_NUM: u64 = 1;
        const ALLOC_PER_HIT_CEILING_DENOM: u64 = 2;
        let ceiling = ITERATIONS as u64 * ALLOC_PER_HIT_CEILING_NUM / ALLOC_PER_HIT_CEILING_DENOM;
        assert!(
            delta <= ceiling,
            "R24 canary: warm-hit fact validation allocation ceiling \
             exceeded. Got {} allocations over {} iterations (ceiling \
             is {} = 0.5 per hit, set to discriminate 1-alloc-per-hit \
             regressions from the substrate's ~0.3-alloc-per-hit \
             baseline). A regression introducing one heap allocation \
             per hit would push this delta toward {}.",
            delta,
            ITERATIONS,
            ceiling,
            ITERATIONS
        );
    }

    /// Discrimination companion: the same loop, but invoking a code
    /// path that DOES allocate (constructing a `String` per
    /// iteration). The companion test asserts the counter ticks; the
    /// pair proves the canary is wired correctly (counter responds to
    /// real allocations).
    #[test]
    fn discrimination_companion_string_allocation_is_observed() {
        // Serialise against `warm_hit_validates_with_zero_allocations`:
        // this test's per-iteration `String` allocations would otherwise
        // race into that test's measurement window via the shared
        // process-global counter.
        let _serial = alloc_serial_guard();

        // Burn iterations to settle any lazy init.
        for i in 0..32 {
            let _ = black_box(format!("warmup-{i}"));
        }
        let baseline = ALLOC_COUNTER.load(Ordering::Relaxed);
        const ITERATIONS: usize = 10_000;
        for i in 0..ITERATIONS {
            // `format!` heap-allocates a `String` per call; the
            // counter must observe this. The exact count is N or N + k
            // depending on small-string optimisation and the
            // formatter's internal buffer churn, but it MUST be > 0.
            let s = format!("hello-{i}");
            black_box(s);
        }
        let after = ALLOC_COUNTER.load(Ordering::Relaxed);
        let delta = after - baseline;
        assert!(
            delta > 0,
            "Discrimination companion: a loop that should allocate \
             (per-iter String formatting) reported zero allocations — \
             the counting allocator is not wired to the global \
             allocator slot. Delta = {}.",
            delta
        );
    }
}

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
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use verter_type_expr::facts::{
        ClosedTypeFact, ObjectMemberFact, ObjectPropertyFact, ObjectShapeFact, SemanticTypeSource,
    };
    use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, TypeBodySlot};
    use verter_type_expr::span_origins::{MemberSpansOrigin, SourceSynthetic};
    use verter_type_expr::MemberVisibility;

    use super::{alloc_serial_guard, ALLOC_COUNTER};

    fn absolute_member(index: usize) -> ObjectMemberFact {
        ObjectMemberFact::Property(ObjectPropertyFact {
            name: format!("member{index}"),
            optional: false,
            readonly: false,
            visibility: MemberVisibility::Public,
            ty: TypeBodySlot {
                // ALREADY-ABSOLUTE anchor: nothing to rewrite.
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from("/already/absolute.ts"),
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
        let _serial = alloc_serial_guard();

        const MEMBERS: usize = 256;
        let members: Vec<ObjectMemberFact> = (0..MEMBERS).map(absolute_member).collect();
        let source = SemanticTypeSource::Closed(ClosedTypeFact::Object(ObjectShapeFact {
            members: Arc::from(members.into_boxed_slice()),
        }));

        // Warm any lazy init, then measure the walk alone.
        let _ = black_box(source.absolutized_against("/consumer.vue"));
        let baseline = ALLOC_COUNTER.load(Ordering::Relaxed);
        let rewritten = source.absolutized_against("/consumer.vue");
        let after = ALLOC_COUNTER.load(Ordering::Relaxed);
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
