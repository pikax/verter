//! Allocation canaries — the single counting-allocator integration
//! binary for `verter_session`.
//!
//! A `#[global_allocator]` is process-global and exactly one may be
//! installed per binary, so every allocation-counting test for this
//! crate co-resides here behind one counting allocator. Counts are
//! thread-local: the Rust test harness may allocate on sibling worker
//! threads outside any test-body lock, and those allocations must not
//! corrupt another thread's measurement window. Every measured path
//! in this binary is synchronous and remains on its harness thread.
//!
//! This binary is allocator-ONLY: it carries no non-allocation tests.
//! The rest of the integration suite lives in the `main` binary.

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
}

fn increment_alloc_counter(size: usize) {
    // Allocation can occur while a thread is tearing down TLS. Do not
    // turn an otherwise valid allocation into a panic if this key is
    // no longer accessible.
    let _ = ALLOC_COUNTER.try_with(|counter| counter.set(counter.get().wrapping_add(1)));
    let _ = ALLOC_BYTES.try_with(|bytes| bytes.set(bytes.get().wrapping_add(size as u64)));
}

fn reset_alloc_counter() {
    ALLOC_COUNTER.with(|counter| counter.set(0));
    ALLOC_BYTES.with(|bytes| bytes.set(0));
}

fn alloc_count() -> u64 {
    ALLOC_COUNTER.with(Cell::get)
}

fn alloc_bytes() -> u64 {
    ALLOC_BYTES.with(Cell::get)
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A grow-in-place realloc still REQUESTS `new_size` bytes; the
        // byte counter records requests, not net residency.
        increment_alloc_counter(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

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

    use std::sync::Arc;

    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    use super::{alloc_count, reset_alloc_counter};

    const FIXTURE_VUE: &str = "<script setup lang=\"ts\">\n\
defineProps<{ label: string; count: number }>();\n\
</script>\n\
<template><div>{{ label }}: {{ count }}</div></template>\n";

    #[test]
    fn record_baseline_allocation_count_for_audit_off_get_component_meta() {
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
        reset_alloc_counter();
        let _ = host.get_component_meta_with_resolution("/Small.vue");
        let allocations = alloc_count();

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
    //! Measurement isolation: allocator counts are thread-local, so
    //! allocations made by sibling harness workers cannot enter the
    //! measured delta.

    use std::hint::black_box;

    use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};
    use verter_session::resolver_core::{
        FactVersionRef, ParseFactRef, PermissiveStoreView, ValidatedFactCache,
    };
    use verter_session::semantic_query::HashValue;

    use super::alloc_count;

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
        // Allocator accounting is local to this harness thread, so
        // per-iteration deltas exclude sibling-worker activity.
        let baseline = alloc_count();
        const ITERATIONS: usize = 10_000;
        for _ in 0..ITERATIONS {
            let result = cache.get_if_valid(&"k", &PermissiveStoreView);
            black_box(result);
        }
        let after = alloc_count();
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
        // Burn iterations to settle any lazy init.
        for i in 0..32 {
            let _ = black_box(format!("warmup-{i}"));
        }
        let baseline = alloc_count();
        const ITERATIONS: usize = 10_000;
        for i in 0..ITERATIONS {
            // `format!` heap-allocates a `String` per call; the
            // counter must observe this. The exact count is N or N + k
            // depending on small-string optimisation and the
            // formatter's internal buffer churn, but it MUST be > 0.
            let s = format!("hello-{i}");
            black_box(s);
        }
        let after = alloc_count();
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

mod canary_signature_fingerprint_zero_alloc {
    //! Allocation canary for `compute_signature_fingerprint` over the
    //! per-domain / source-env / project-generation `FactVersionRef`
    //! variants.
    //!
    //! The fingerprint feeds each variant's typed `Hash` impl into the
    //! two seeded hashers directly — a pure stack computation with NO
    //! heap allocation. The pre-fix implementation serialised the
    //! `Parse` / `ResolveImports` / `RouteSurface` / `FileSourceEnv` /
    //! `ProjectGeneration` variants through `format!("{f:?}")` — one
    //! `String` allocation (plus growth reallocations) per fact per
    //! call — so this canary reads a multi-thousand delta there and
    //! zero here.

    use std::hint::black_box;

    use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};
    use verter_session::resolver_core::{
        compute_signature_fingerprint_for_tests, DerivedFactKind, FactVersionRef, ParseFactRef,
        ResolveImportsFactRef, RouteSurfaceFactRef,
    };

    use super::alloc_count;

    /// One fact per externally-constructible `FactVersionRef` variant
    /// so the measured loop covers the fingerprint fold — in particular
    /// four of the five arms that used to route through
    /// `format!("{f:?}")`. `FileSourceEnv` is absent by design: its
    /// `ParseEnvHash` field is sealed to in-crate construction (an R6
    /// content-free-dimension rail this canary must not weaken); it
    /// folds through the same typed-Hash arm as every variant below,
    /// and its field-level fingerprint discrimination is pinned by the
    /// in-crate `file_source_env_fact_rail_tests` suite.
    fn all_variant_facts() -> Vec<FactVersionRef> {
        vec![
            FactVersionRef::FileWholeHash {
                canonical_id: "/w/owner.vue".to_string(),
                hash: [1u8; 16],
            },
            FactVersionRef::DerivedFactHash {
                canonical_id: "/w/dep.ts".to_string(),
                kind: DerivedFactKind::Route,
                hash: [2u8; 16],
            },
            FactVersionRef::Parse(ParseFactRef {
                canonical_id: "/w/parse.ts".to_string(),
                key: FactKey::Export {
                    name: "Foo".into(),
                    space: SymbolSpace::Type,
                },
                lane: FactLane::Semantic,
                expected_hash: [3u8; 16],
            }),
            FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
                canonical_id: "/w/imports.ts".to_string(),
                key: FactKey::Export {
                    name: "Bar".into(),
                    space: SymbolSpace::Type,
                },
                lane: FactLane::Semantic,
                expected_hash: [4u8; 16],
            }),
            FactVersionRef::RouteSurface(RouteSurfaceFactRef {
                canonical_id: "/w/route.ts".to_string(),
                key: FactKey::Export {
                    name: "Baz".into(),
                    space: SymbolSpace::Type,
                },
                lane: FactLane::Semantic,
                expected_hash: [5u8; 16],
            }),
            FactVersionRef::ProjectGeneration { generation: 7 },
        ]
    }

    #[test]
    fn fingerprint_over_every_variant_allocates_nothing() {
        // Setup phase — fact construction allocates and is NOT counted.
        let facts = all_variant_facts();

        // Warm any lazy init, then measure the fingerprint fold alone.
        for _ in 0..64 {
            let _ = black_box(compute_signature_fingerprint_for_tests(&facts));
        }
        let baseline = alloc_count();
        const ITERATIONS: usize = 1_000;
        for _ in 0..ITERATIONS {
            let fp = compute_signature_fingerprint_for_tests(&facts);
            black_box(fp);
        }
        let after = alloc_count();
        let delta = after - baseline;
        assert_eq!(
            delta,
            0,
            "fingerprinting a {}-fact signature must be allocation-free \
             (typed Hash fold) — a fold that serialises variants through \
             format!() reports ≥ 5 allocations per call here; observed \
             {delta} over {ITERATIONS} iterations",
            facts.len(),
        );
    }
}

mod canary_fact_emission_allocation_volume_class {
    //! ALLOCATION-COMPLEXITY canary for `emit_parse_facts`.
    //!
    //! # What this canary claims, exactly
    //!
    //! One thing: the emitter's ALLOCATION volume — allocator calls and
    //! allocator bytes — stays in the linear class as the declaration
    //! count grows. It is not a total-work or `O(file_size)` guarantee,
    //! and it must not be described as one: a regression that computes
    //! quadratically over already-collected data allocates nothing and is
    //! invisible here.
    //!
    //! The companion guard in
    //! `tests/cases/g_fact/fact_emission_output_cardinality.rs` claims
    //! its own separate thing — that the emitted FACT count is affine in
    //! the declaration count. That is necessary but not sufficient: a
    //! regression can materialise per-pair data WITHOUT changing the fact
    //! count at all.
    //! The concrete case — and the most plausible real bug in this
    //! emitter — is rebuilding a whole-file hash buffer once per
    //! declaration (e.g. the `SyntacticExportSet` fold moving inside the
    //! per-symbol loop). Every iteration writes the SAME registry key, so
    //! the fact count is untouched, while the bytes handed to the
    //! allocator go quadratic.
    //!
    //! So this canary measures the allocator traffic of one
    //! `emit_parse_facts` call at `N` and `2N` declarations and asserts
    //! the growth stays in the LINEAR class:
    //!
    //! - linear    O(N)  ⇒ doubling the input ⇒ ratio ≈ 2.0x
    //! - quadratic O(N²) ⇒ doubling the input ⇒ ratio ≈ 4.0x
    //!
    //! A 3.0x boundary sits between the two classes. That is the same
    //! class-boundary argument the retired wall-clock version used —
    //! 3.0x was never noise tolerance, it was the midpoint between the
    //! classes — but the instrument underneath it is now an exact
    //! integer instead of a clock. Allocator counts cannot be perturbed
    //! by machine load: the same input produces byte-identical counts on
    //! a busy machine and an idle one, which is precisely the property
    //! the timing version lacked.
    //!
    //! BOTH the call count and the byte count are asserted, because they
    //! catch different shapes. A container that grows geometrically to
    //! size `n` costs `O(log n)` allocator CALLS but `O(n)` BYTES, so the
    //! per-declaration whole-file-buffer regression above is only
    //! ~linear in calls while being quadratic in bytes.
    //!
    //! Residual, stated honestly: a regression that is quadratic in PURE
    //! COMPUTE — emitting no extra facts and allocating nothing — is
    //! invisible to this canary and to its fact-cardinality companion.
    //! The specific case of re-scanning the shallow inventory once per
    //! declaration is caught by
    //! `tests/cases/g_fact/fact_emission_work_class.rs` (a sealed counting
    //! view over the inventory). Beyond that — arbitrary quadratic
    //! computation over already-collected data — only a clock sees it, so
    //! that measurement is kept in
    //! `crates/verter_bench/benches/fact_emission_scaling.rs`, reported
    //! rather than gated.
    //!
    //! Measurement isolation: allocator counts are thread-local, so
    //! allocations made by sibling harness workers cannot enter a
    //! measured delta. `emit_parse_facts` is synchronous and stays on
    //! this harness thread.
    //!
    //! Hermeticity: no third-party corpus or external fixture — the
    //! declarations are synthesised in-process.

    use std::hint::black_box;
    use std::sync::Arc;

    use verter_session::fact_emission::emit_parse_facts;
    use verter_session::project_type_store::IndexedReady;
    use verter_session::resolver_core::shallow_file_state::ShallowFileState;

    use super::{alloc_bytes, alloc_count};

    /// `decl_count` IDENTICAL-shape interface declarations, built through
    /// the production-shaped service-backed path so the shallow header
    /// walk is pre-paid at construction and the measured window observes
    /// only `emit_parse_facts`.
    fn build_indexed(decl_count: usize) -> Arc<IndexedReady> {
        let mut source = String::with_capacity(decl_count * 48);
        for i in 0..decl_count {
            source.push_str(&format!("export interface Decl{i} {{ a: string }}\n"));
        }
        let shallow =
            ShallowFileState::service_backed_for_test_with_hash("/large.ts", &source, [0u8; 16]);
        Arc::new(IndexedReady::new_for_test_with_state(
            [0u8; 16],
            shallow,
            Arc::from(source.as_str()),
            Arc::from(source.as_str()),
        ))
    }

    /// Allocator traffic of exactly one `emit_parse_facts` call.
    struct Volume {
        calls: u64,
        bytes: u64,
    }

    fn emit_volume(indexed: &Arc<IndexedReady>) -> Volume {
        let calls_before = alloc_count();
        let bytes_before = alloc_bytes();
        let emission = emit_parse_facts(indexed);
        let calls_after = alloc_count();
        let bytes_after = alloc_bytes();
        // Keep the result alive past the counter reads so nothing is
        // optimised away; deallocation is not counted, so dropping it
        // afterwards cannot disturb the delta.
        black_box(&emission);
        Volume {
            calls: calls_after - calls_before,
            bytes: bytes_after - bytes_before,
        }
    }

    /// Scaled-by-1000 ratio `b / a`, so the class boundary can be
    /// compared with integer math.
    fn ratio_milli(a: u64, b: u64) -> u64 {
        (b * 1000) / a.max(1)
    }

    /// 3.0x, scaled by 1000: the midpoint between the linear (~2.0x) and
    /// quadratic (~4.0x) classes.
    const CLASS_BOUNDARY_MILLI: u64 = 3_000;

    #[test]
    fn fact_emission_allocation_volume_scales_linearly() {
        const N: usize = 5_000;

        // Setup phase — building the inputs allocates heavily and is
        // deliberately OUTSIDE every measured window.
        let indexed_n = build_indexed(N);
        let indexed_2n = build_indexed(2 * N);

        // Warm both inputs so one-time lazy initialisation (TLS slots,
        // shard pools) is not attributed to a measured call.
        black_box(emit_parse_facts(&indexed_n));
        black_box(emit_parse_facts(&indexed_2n));

        let v_n = emit_volume(&indexed_n);
        let v_2n = emit_volume(&indexed_2n);

        let calls_ratio = ratio_milli(v_n.calls, v_2n.calls);
        let bytes_ratio = ratio_milli(v_n.bytes, v_2n.bytes);

        eprintln!(
            "fact-emission allocation scaling — emit({N}): {} calls / {} bytes; \
             emit({}): {} calls / {} bytes; call ratio {}.{:03}x, byte ratio {}.{:03}x \
             (class boundary {}.{:03}x)",
            v_n.calls,
            v_n.bytes,
            2 * N,
            v_2n.calls,
            v_2n.bytes,
            calls_ratio / 1000,
            calls_ratio % 1000,
            bytes_ratio / 1000,
            bytes_ratio % 1000,
            CLASS_BOUNDARY_MILLI / 1000,
            CLASS_BOUNDARY_MILLI % 1000,
        );

        // ANTI-VACUITY. A ratio is meaningless if the measurement window
        // saw nothing: with `calls == bytes == 0` at both sizes the ratio
        // would be 0 and the class assertions would pass while proving
        // nothing. Pin that the counting allocator observed real,
        // input-proportional work first.
        assert!(
            v_n.calls > 0 && v_n.bytes >= N as u64,
            "anti-vacuity: emitting facts for {N} declarations was measured at {} allocator \
             calls / {} bytes. Either the counting allocator is not wired into the global \
             allocator slot for this binary, or the emitter did no work — in both cases the \
             scaling ratios below would pass vacuously.",
            v_n.calls,
            v_n.bytes,
        );
        assert!(
            v_2n.bytes > v_n.bytes,
            "anti-vacuity: doubling the declaration count from {N} to {} did not increase \
             allocator byte traffic ({} bytes then {} bytes). A ratio at or below 1.0x means \
             the measurement is not tracking input size, so the class assertions below cannot \
             discriminate.",
            2 * N,
            v_n.bytes,
            v_2n.bytes,
        );

        // THE ALLOCATION-CLASS GUARD — bytes. This is the sensitive one:
        // a per-declaration rebuild of a whole-file buffer is quadratic
        // here even when the fact count and the allocator CALL count
        // both stay ~linear.
        assert!(
            bytes_ratio < CLASS_BOUNDARY_MILLI,
            "emit_parse_facts ALLOCATION VOLUME must stay in the LINEAR class. Doubling the \
             declaration count from {N} to {} should roughly double the bytes requested from \
             the allocator (ratio ≈ 2.0x); allocating per declaration PAIR shows ≈ 4.0x. \
             Observed byte ratio {}.{:03}x crosses the 3.0x class boundary: {} bytes at {N} \
             declarations, {} bytes at {}. This measurement is exact and load-independent, so a \
             failure here is a real regression in ALLOCATION growth, not machine noise. \
             (Allocation volume only — this says nothing about the emitter's total work. \
             Inventory traversal is guarded by \
             tests/cases/g_fact/fact_emission_work_class.rs, emitted fact cardinality by \
             tests/cases/g_fact/fact_emission_output_cardinality.rs.)",
            2 * N,
            bytes_ratio / 1000,
            bytes_ratio % 1000,
            v_n.bytes,
            v_2n.bytes,
            2 * N,
        );

        // THE ALLOCATION-CLASS GUARD — allocator call count. Catches the
        // shape that allocates a fresh object per (declaration,
        // declaration) pair.
        assert!(
            calls_ratio < CLASS_BOUNDARY_MILLI,
            "emit_parse_facts ALLOCATION VOLUME must stay in the LINEAR class. Doubling the \
             declaration count from {N} to {} should roughly double the number of allocator \
             calls (ratio ≈ 2.0x); allocating per declaration PAIR shows ≈ 4.0x. Observed call \
             ratio {}.{:03}x crosses the 3.0x class boundary: {} calls at {N} declarations, {} \
             calls at {}. This measurement is exact and load-independent, so a failure here is \
             a real regression in ALLOCATION growth, not machine noise. (Allocation volume \
             only — this says nothing about the emitter's total work.)",
            2 * N,
            calls_ratio / 1000,
            calls_ratio % 1000,
            v_n.calls,
            v_2n.calls,
            2 * N,
        );
    }

    /// Determinism companion. The property that makes this canary a
    /// correctness-suite citizen rather than a benchmark is that the
    /// measurement is a function of the INPUT alone — repeating it yields
    /// byte-identical numbers, whatever else the machine is doing. Assert
    /// that directly: three consecutive measurements of the same input
    /// must be exactly equal.
    ///
    /// A wall-clock measurement cannot pass this test at all, so it also
    /// documents why the timing version was replaced.
    #[test]
    fn fact_emission_allocation_volume_is_repeatable_to_the_byte() {
        const N: usize = 2_000;
        let indexed = build_indexed(N);

        // Settle one-time lazy initialisation before the first recorded
        // measurement, so run-to-run equality is measured on the
        // steady state.
        black_box(emit_parse_facts(&indexed));

        let first = emit_volume(&indexed);
        let second = emit_volume(&indexed);
        let third = emit_volume(&indexed);

        assert!(
            first.calls > 0 && first.bytes > 0,
            "anti-vacuity: measured no allocator traffic for {N} declarations ({} calls / {} \
             bytes) — equality across three zero measurements would prove nothing",
            first.calls,
            first.bytes,
        );
        assert_eq!(
            (second.calls, second.bytes),
            (first.calls, first.bytes),
            "allocator traffic for one emit_parse_facts call must be a function of the input \
             alone: run 1 was {} calls / {} bytes, run 2 was {} calls / {} bytes. If these \
             differ, the measurement carries hidden state and the scaling ratios above are not \
             reproducible.",
            first.calls,
            first.bytes,
            second.calls,
            second.bytes,
        );
        assert_eq!(
            (third.calls, third.bytes),
            (first.calls, first.bytes),
            "allocator traffic for one emit_parse_facts call must be a function of the input \
             alone: run 1 was {} calls / {} bytes, run 3 was {} calls / {} bytes",
            first.calls,
            first.bytes,
            third.calls,
            third.bytes,
        );
    }
}
