//! Memory-audit unit tests: the disabled contract, the counters, the
//! peak/live coherence rails, and sampled site attribution.
//!
//! The runtime gate, the counters, and the site table are process-global,
//! so every test here takes the shared serialisation window in
//! [`audit_test_support`] and restores the disabled state on drop.

use super::*;

/// Shared serialization + gate windows for the memory-audit tests. The
/// runtime gate, counters, and site table are process-global, so every
/// test that touches them serialises on one mutex and restores the
/// disabled state on drop.
mod audit_test_support {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::runtime;

    static AUDIT_TEST_SERIAL: Mutex<()> = Mutex::new(());

    fn serial_guard() -> MutexGuard<'static, ()> {
        AUDIT_TEST_SERIAL
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Serialised window with the gate forced OFF.
    pub(super) struct DisabledWindow {
        _serial: MutexGuard<'static, ()>,
    }

    impl DisabledWindow {
        pub(super) fn acquire() -> Self {
            let serial = serial_guard();
            runtime::disable_for_tests();
            Self { _serial: serial }
        }
    }

    impl Drop for DisabledWindow {
        fn drop(&mut self) {
            runtime::disable_for_tests();
        }
    }

    /// Serialised window with the audit ENABLED through the production
    /// enable path (fresh epoch) and sampling armed at `interval`
    /// (0 = counters only). Disabled again on drop.
    pub(super) struct EnabledWindow {
        _serial: MutexGuard<'static, ()>,
    }

    impl EnabledWindow {
        pub(super) fn arm(interval: usize) -> Self {
            let serial = serial_guard();
            runtime::disable_for_tests();
            super::memory_audit_enable(Some(super::NapiMemoryAuditEnableOptions {
                sampleEvery: Some(interval as u32),
            }));
            Self { _serial: serial }
        }
    }

    impl Drop for EnabledWindow {
        fn drop(&mut self) {
            runtime::disable_for_tests();
        }
    }
}

/// Disabled contract (runtime gate off — the default): the exports stay
/// present but advertise a disabled audit (`null` snapshot, `false`
/// reset, `null` sites), and `memoryAuditEnable()` flips the runtime
/// gate on with fresh-epoch counters.
mod disabled_contract_tests {
    use super::*;

    #[test]
    fn snapshot_reset_and_sites_advertise_disabled_until_enabled() {
        let _window = audit_test_support::DisabledWindow::acquire();
        assert!(
            memory_audit_snapshot().is_none(),
            "disabled: memoryAuditSnapshot() must return null so callers \
             can detect that the runtime audit gate is off"
        );
        assert!(
            !memory_audit_reset_high_water(),
            "disabled: memoryAuditResetHighWater() must return false"
        );
        assert!(
            memory_audit_sites(50).is_none(),
            "disabled: memoryAuditSites() must return null"
        );
    }

    #[test]
    fn enable_arms_counters_and_optional_sampling_with_a_fresh_epoch() {
        let _window = audit_test_support::DisabledWindow::acquire();
        assert!(
            memory_audit_enable(Some(NapiMemoryAuditEnableOptions {
                sampleEvery: Some(5),
            })),
            "memoryAuditEnable must report the audit as enabled"
        );
        let snapshot =
            memory_audit_snapshot().expect("enabled: memoryAuditSnapshot() must return counters");
        // Fresh epoch: enabling resets the counters, so totals reflect
        // only post-enable activity (a tiny number of allocations can
        // land between the reset and this snapshot).
        assert!(
            snapshot.allocatedBytesTotal < (64 * 1024 * 1024) as f64,
            "enable must start a fresh counter epoch (allocatedBytesTotal \
             {} should be near zero right after enabling)",
            snapshot.allocatedBytesTotal
        );
        // Cleanup happens via the window drop.
    }
}

mod counter_tests {
    use std::hint::black_box;

    use super::*;

    const PROBE_BYTES: usize = 128 * 1024 * 1024;

    #[test]
    fn allocations_and_deallocations_move_counters() {
        let _window = audit_test_support::EnabledWindow::arm(0);

        let before = memory_audit_snapshot().expect("enabled audit must snapshot");

        let probe = black_box(vec![0u8; PROBE_BYTES]);
        let held = memory_audit_snapshot().expect("enabled audit must snapshot");
        assert!(
            held.allocCount > before.allocCount,
            "an allocation must advance allocCount ({} -> {})",
            before.allocCount,
            held.allocCount
        );
        assert!(
            held.allocatedBytesTotal >= before.allocatedBytesTotal + PROBE_BYTES as f64,
            "allocatedBytesTotal must grow by at least the probe size \
             ({} -> {}, probe {PROBE_BYTES})",
            before.allocatedBytesTotal,
            held.allocatedBytesTotal
        );
        assert!(
            held.liveBytes > before.liveBytes,
            "liveBytes must grow while the probe is held ({} -> {})",
            before.liveBytes,
            held.liveBytes
        );
        assert!(
            held.peakLiveBytes >= held.liveBytes,
            "peakLiveBytes is a high-water mark and can never trail liveBytes \
             (peak {}, live {})",
            held.peakLiveBytes,
            held.liveBytes
        );

        drop(black_box(probe));
        let after = memory_audit_snapshot().expect("enabled audit must snapshot");
        assert!(
            after.deallocCount > held.deallocCount,
            "dropping the probe must advance deallocCount ({} -> {})",
            held.deallocCount,
            after.deallocCount
        );
        assert!(
            after.liveBytes <= held.liveBytes - (PROBE_BYTES / 2) as f64,
            "dropping the {PROBE_BYTES}-byte probe must shrink liveBytes \
             substantially ({} -> {})",
            held.liveBytes,
            after.liveBytes
        );
        assert!(
            after.allocatedBytesTotal >= held.allocatedBytesTotal,
            "allocatedBytesTotal is monotonic ({} -> {})",
            held.allocatedBytesTotal,
            after.allocatedBytesTotal
        );
    }

    #[test]
    fn reset_high_water_drops_peak_to_current_live_and_rearms() {
        let _window = audit_test_support::EnabledWindow::arm(0);

        // Raise the high-water mark far above steady-state live bytes,
        // then release the spike.
        let spike = black_box(vec![0u8; PROBE_BYTES]);
        drop(black_box(spike));

        let peaked = memory_audit_snapshot().expect("enabled audit must snapshot");
        assert!(
            peaked.peakLiveBytes >= peaked.liveBytes + (PROBE_BYTES / 2) as f64,
            "precondition: after the spike is freed, peak ({}) must sit far \
             above live ({})",
            peaked.peakLiveBytes,
            peaked.liveBytes
        );

        assert!(
            memory_audit_reset_high_water(),
            "enabled audit: memoryAuditResetHighWater() must return true"
        );

        let reset = memory_audit_snapshot().expect("enabled audit must snapshot");
        assert!(
            reset.peakLiveBytes < peaked.peakLiveBytes - (PROBE_BYTES / 2) as f64,
            "reset must drop the high-water mark from the pre-reset peak \
             ({} -> {})",
            peaked.peakLiveBytes,
            reset.peakLiveBytes
        );
        // Post-reset the mark tracks current live bytes (small slack for
        // sibling-thread churn between the reset and this snapshot).
        assert!(
            reset.peakLiveBytes <= reset.liveBytes + (16 * 1024 * 1024) as f64,
            "reset must pin the high-water mark near current live bytes \
             (peak {}, live {})",
            reset.peakLiveBytes,
            reset.liveBytes
        );

        // The mark re-arms: a fresh spike raises it again.
        let respike = black_box(vec![0u8; PROBE_BYTES]);
        let rearmed = memory_audit_snapshot().expect("enabled audit must snapshot");
        drop(black_box(respike));
        assert!(
            rearmed.peakLiveBytes >= reset.peakLiveBytes + (PROBE_BYTES / 2) as f64,
            "a post-reset spike must advance the high-water mark again \
             ({} -> {})",
            reset.peakLiveBytes,
            rearmed.peakLiveBytes
        );
    }
}

/// Coherence between live bytes and the high-water mark.
///
/// The record path advances the two counters in separate atomic steps, so
/// a reader can land between them. Every pair the audit surface hands out
/// must still satisfy `peakLiveBytes >= liveBytes`, under concurrent
/// allocation and across a high-water re-arm. Two rails below look past
/// the reported pair at the STORED counters, which the reader's fold would
/// otherwise hide: the re-arm must keep a still-live block in the new
/// window, and enabling must clear both counters into a fresh epoch.
mod snapshot_coherence_tests {
    use std::hint::{black_box, spin_loop};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    /// Live level the injected mid-publication state is pinned at. Any
    /// value works: concurrent allocator traffic repairs the injected
    /// state by size, not by magnitude.
    const INJECTED_LIVE: i64 = 1 << 30;
    /// The mark trails live by one block — exactly the gap an allocating
    /// call opens between adding to live bytes and advancing the mark.
    const INJECTED_MARK: i64 = INJECTED_LIVE - 4096;

    /// The reader-side half: a snapshot taken while the record path sits
    /// between its two publication steps must not hand the caller a mark
    /// below the live value it reports in the same pair.
    #[test]
    fn snapshot_taken_between_the_record_path_steps_reports_a_coherent_pair() {
        let _window = audit_test_support::EnabledWindow::arm(0);
        const PROBES: usize = 4096;

        for probe in 0..PROBES {
            // Stored in record-path order: the mark lags, live is ahead.
            counting::PEAK_LIVE_BYTES.store(INJECTED_MARK, Ordering::Relaxed);
            counting::LIVE_BYTES.store(INJECTED_LIVE, Ordering::Relaxed);

            let snapshot = memory_audit_snapshot().expect("enabled audit must snapshot");
            assert!(
                snapshot.peakLiveBytes >= snapshot.liveBytes,
                "probe {probe}: peakLiveBytes is the high-water mark of \
                 liveBytes and must never be published below the liveBytes of \
                 the same snapshot, including when the read lands between the \
                 record path's two publication steps (peak {}, live {})",
                snapshot.peakLiveBytes,
                snapshot.liveBytes
            );
        }
    }

    /// The full acceptance shape: allocator threads churn live bytes while
    /// an observer snapshots and periodically re-arms the high-water mark.
    /// Every returned snapshot must be coherent, and the mark must not lose
    /// a live value the new window already reached.
    #[test]
    fn concurrent_allocation_and_high_water_rearm_never_publish_an_incoherent_pair() {
        let _window = audit_test_support::EnabledWindow::arm(0);

        const ALLOCATOR_THREADS: usize = 4;
        const ALLOCATIONS_PER_THREAD: usize = 200_000;
        const MAX_SNAPSHOTS: usize = 200_000;
        const REARM_EVERY: usize = 997;
        /// Enough snapshots that several re-arms provably landed while the
        /// allocator threads were still running.
        const MIN_OVERLAPPED_SNAPSHOTS: usize = 4 * REARM_EVERY;

        let finished = AtomicUsize::new(0);
        let mut snapshots = 0usize;
        let mut rearms = 0usize;

        std::thread::scope(|scope| {
            for _ in 0..ALLOCATOR_THREADS {
                scope.spawn(|| {
                    for step in 0..ALLOCATIONS_PER_THREAD {
                        // Mixed sizes so live bytes both climb and fall
                        // while the observer reads and re-arms.
                        let bytes = if step % 128 == 0 { 1 << 20 } else { 4096 };
                        drop(black_box(vec![0u8; bytes]));
                    }
                    finished.fetch_add(1, Ordering::Release);
                });
            }

            // Highest live value any snapshot has reported since the last
            // re-arm. The mark can never sit below it: live demonstrably
            // reached that value inside the current window.
            let mut live_high_water_since_rearm = i64::MIN;
            while finished.load(Ordering::Acquire) < ALLOCATOR_THREADS && snapshots < MAX_SNAPSHOTS
            {
                let snapshot = memory_audit_snapshot().expect("enabled audit must snapshot");
                snapshots += 1;
                let live = snapshot.liveBytes as i64;
                let peak = snapshot.peakLiveBytes as i64;
                assert!(
                    peak >= live,
                    "snapshot {snapshots}: peakLiveBytes must never be \
                     published below the liveBytes of the same snapshot \
                     (peak {peak}, live {live})"
                );
                live_high_water_since_rearm = live_high_water_since_rearm.max(live);
                assert!(
                    peak >= live_high_water_since_rearm,
                    "snapshot {snapshots}: the high-water mark must retain the \
                     highest live value observed since the last re-arm \
                     (peak {peak}, observed high water \
                     {live_high_water_since_rearm})"
                );

                if snapshots.is_multiple_of(REARM_EVERY) {
                    assert!(
                        memory_audit_reset_high_water(),
                        "enabled audit: memoryAuditResetHighWater() must return true"
                    );
                    rearms += 1;
                    live_high_water_since_rearm = i64::MIN;
                }
            }
        });

        assert!(
            snapshots >= MIN_OVERLAPPED_SNAPSHOTS,
            "the observer must overlap the allocator threads for the \
             assertions above to mean anything (only {snapshots} snapshots, \
             {rearms} re-arms)"
        );
    }

    /// The re-arm's own rail, read off the raw counters.
    ///
    /// `snapshot()` folds the live value it observes into the mark, so every
    /// pair it hands out is coherent even when the STORED mark is not — which
    /// makes it structurally blind to a re-arm that drops a still-live block
    /// out of the new window. This test therefore reads the counters
    /// directly, and each round is shaped so the read happens while the only
    /// other deliberate writer of those counters is quiescent:
    ///
    /// 1. the round opens a gate, and the writer performs EXACTLY ONE
    ///    record-path-shaped step — raise live, then advance the mark — and
    ///    never frees it, so that block is still live for the rest of the
    ///    round;
    /// 2. concurrently, the observer re-arms the high-water mark, so the
    ///    re-arm races the step;
    /// 3. the observer waits on the step's completion flag before reading, so
    ///    the writer cannot be sitting between its own two publication steps
    ///    while the pair is read.
    ///
    /// A re-arm that baselines the mark on a separately loaded live value
    /// loses the whole step: it reads live before the `fetch_add`, the step
    /// completes, and the store drops the mark back under the live value the
    /// block left behind. Nothing raises it again — the writer is done for the
    /// round — so the deficit is still there when the observer reads.
    ///
    /// Only the deliberate writer is quiesced. The process's real allocator
    /// traffic keeps running the record path with its own transient
    /// add-before-max gap, so a deficit is attributed to the re-arm only once
    /// it exceeds any plausible in-flight real allocation; the injected step
    /// is four times that tolerance.
    #[test]
    fn rearm_keeps_a_still_live_block_in_the_new_window() {
        let _window = audit_test_support::EnabledWindow::arm(0);

        /// Fake live bytes: the writer moves the counters directly, so the
        /// step can dwarf real allocator noise without committing memory.
        const STEP_BYTES: i64 = 1 << 30;
        /// Tolerance for the record-path gap of unrelated real allocations.
        const REAL_TRAFFIC_SLACK: i64 = STEP_BYTES / 4;
        const ROUNDS: usize = 20_000;
        /// Sweep the observer's start offset across the writer's reaction
        /// time so the re-arm lands at every phase relative to the step.
        const PHASE_SWEEP: usize = 128;

        let open = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let shutdown = AtomicBool::new(false);
        let mut lost_rounds = 0usize;
        let mut worst_deficit = 0i64;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut round = 0usize;
                loop {
                    while open.load(Ordering::Acquire) <= round {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        spin_loop();
                    }
                    round += 1;
                    // One record-path-shaped step, never freed.
                    let live =
                        counting::LIVE_BYTES.fetch_add(STEP_BYTES, Ordering::Relaxed) + STEP_BYTES;
                    counting::PEAK_LIVE_BYTES.fetch_max(live, Ordering::Release);
                    done.store(round, Ordering::Release);
                }
            });

            for round in 1..=ROUNDS {
                open.store(round, Ordering::Release);
                for _ in 0..(round % PHASE_SWEEP) {
                    spin_loop();
                }
                counting::reset_high_water();

                while done.load(Ordering::Acquire) < round {
                    spin_loop();
                }
                let live = counting::LIVE_BYTES.load(Ordering::Acquire);
                let peak = counting::PEAK_LIVE_BYTES.load(Ordering::Acquire);
                if peak < live - REAL_TRAFFIC_SLACK {
                    lost_rounds += 1;
                    worst_deficit = worst_deficit.max(live - peak);
                }
            }

            shutdown.store(true, Ordering::Relaxed);
        });

        assert_eq!(
            lost_rounds, 0,
            "re-arming the high-water mark dropped a block that was still \
             live when the re-arm completed: {lost_rounds} of {ROUNDS} rounds \
             ended with the mark below live bytes, worst deficit \
             {worst_deficit} bytes against a {STEP_BYTES}-byte step"
        );
    }

    /// Enabling starts a fresh epoch even when it finds the counters mid
    /// publication: BOTH the mark and live bytes are cleared, so no part of
    /// the previous state — and no half of an injected incoherent pair —
    /// survives into the new window.
    #[test]
    fn enabling_clears_an_injected_mid_publication_state() {
        /// The enable path and this test both allocate between the reset and
        /// the read, so "cleared" is a bound, not an equality.
        const EPOCH_SLACK: f64 = (16 * 1024 * 1024) as f64;

        let _window = audit_test_support::DisabledWindow::acquire();
        counting::PEAK_LIVE_BYTES.store(INJECTED_MARK, Ordering::Relaxed);
        counting::LIVE_BYTES.store(INJECTED_LIVE, Ordering::Relaxed);

        assert!(
            memory_audit_enable(None),
            "memoryAuditEnable must report the audit as enabled"
        );
        let snapshot = memory_audit_snapshot().expect("enabled audit must snapshot");
        assert!(
            snapshot.liveBytes.abs() < EPOCH_SLACK,
            "a fresh epoch must clear live bytes, not inherit the \
             {INJECTED_LIVE} injected by the previous state (liveBytes {})",
            snapshot.liveBytes
        );
        assert!(
            snapshot.peakLiveBytes < EPOCH_SLACK,
            "a fresh epoch must clear the high-water mark, not inherit the \
             {INJECTED_MARK} injected by the previous state (peakLiveBytes {})",
            snapshot.peakLiveBytes
        );
        assert!(
            snapshot.peakLiveBytes >= snapshot.liveBytes,
            "the first snapshot of a fresh epoch must still report a peak at \
             least its live bytes (peak {}, live {})",
            snapshot.peakLiveBytes,
            snapshot.liveBytes
        );
    }
}

/// Sampled allocation-site attribution. Arming goes through the
/// production `memoryAuditEnable` path; every window serialises on the
/// shared mutex and disarms on drop so armed intervals never leak into
/// sibling tests.
mod sampling_tests {
    use std::hint::black_box;

    use super::*;

    /// Named, never-inlined allocation site the tests look for by symbol
    /// name after lazy read-time resolution. Returns the allocations so
    /// the optimiser cannot elide them.
    #[inline(never)]
    fn allocate_probe_site_for_sampling(iterations: usize) -> Vec<Vec<u8>> {
        let mut keep = Vec::new();
        for _ in 0..iterations {
            keep.push(black_box(vec![0xABu8; 4096]));
        }
        keep
    }

    fn parse_sites(json: &str) -> Vec<serde_json::Value> {
        let value: serde_json::Value =
            serde_json::from_str(json).expect("memoryAuditSites must return valid JSON");
        value
            .as_array()
            .expect("memoryAuditSites must return a JSON array")
            .clone()
    }

    #[test]
    fn sites_are_null_while_sampling_is_not_armed() {
        let _window = audit_test_support::EnabledWindow::arm(0);
        assert!(
            memory_audit_sites(50).is_none(),
            "audit enabled with sampling NOT armed: memoryAuditSites() \
             must return null (callers treat it as 'no site data')"
        );
    }

    #[test]
    fn sampling_records_named_allocation_site_with_counts_and_bytes() {
        let window = audit_test_support::EnabledWindow::arm(1);
        const ITERATIONS: usize = 64;

        let keep = allocate_probe_site_for_sampling(ITERATIONS);
        let json =
            memory_audit_sites(4096).expect("armed sampling must produce a sites report, not null");
        drop(black_box(keep));

        let rows = parse_sites(&json);
        assert!(
            !rows.is_empty(),
            "interval=1 sampling over {ITERATIONS} probe allocations must \
             record at least one site"
        );
        assert!(
            rows.len() <= 4096,
            "the site table is capped at 4096 sites (got {})",
            rows.len()
        );

        let probe_row = rows
            .iter()
            .find(|row| {
                row["frames"].as_array().is_some_and(|frames| {
                    frames.iter().any(|frame| {
                        frame
                            .as_str()
                            .is_some_and(|name| name.contains("allocate_probe_site_for_sampling"))
                    })
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "no reported site resolved to allocate_probe_site_for_sampling; \
                     frames must attribute the sampled allocations to their caller. \
                     report: {json}"
                )
            });

        let count = probe_row["count"].as_u64().expect("count must be a u64");
        let bytes = probe_row["bytes"].as_u64().expect("bytes must be a u64");
        assert!(
            count >= ITERATIONS as u64,
            "interval=1 must sample every probe allocation (count {count} < {ITERATIONS})"
        );
        assert!(
            bytes >= (ITERATIONS * 4096) as u64,
            "sampled bytes must cover the probe payloads (bytes {bytes})"
        );
        assert_eq!(
            probe_row["estimatedTotalBytes"].as_u64(),
            Some(bytes),
            "interval=1: estimatedTotalBytes == bytes * 1"
        );
        let frames = probe_row["frames"].as_array().expect("frames array");
        assert!(
            !frames.is_empty() && frames.len() <= 8,
            "reported stacks are 1..=8 frames (got {})",
            frames.len()
        );
        assert!(
            frames.iter().all(|frame| {
                frame
                    .as_str()
                    .is_some_and(|name| !name.contains("memory_audit::sampling::"))
            }),
            "sampler-internal plumbing frames (module path \
             memory_audit::sampling::*) must be skipped from the reported \
             leading frames: {frames:?}"
        );
        drop(window);
    }

    #[test]
    fn estimated_total_bytes_scales_by_the_sampling_interval() {
        let window = audit_test_support::EnabledWindow::arm(3);

        let keep = allocate_probe_site_for_sampling(300);
        let json =
            memory_audit_sites(4096).expect("armed sampling must produce a sites report, not null");
        drop(black_box(keep));

        let rows = parse_sites(&json);
        assert!(
            !rows.is_empty(),
            "interval=3 over 300 allocations must sample"
        );
        for row in &rows {
            let bytes = row["bytes"].as_u64().expect("bytes must be a u64");
            assert_eq!(
                row["estimatedTotalBytes"].as_u64(),
                Some(bytes * 3),
                "estimatedTotalBytes must be bytes * interval (interval=3): {row}"
            );
        }
        drop(window);
    }

    #[test]
    fn concurrent_sampling_does_not_deadlock_or_recurse() {
        let window = audit_test_support::EnabledWindow::arm(1);

        // Sampling captures backtraces, and backtrace capture itself
        // allocates: without the recursion guard this loop would
        // self-sample unboundedly (stack overflow) or self-deadlock on
        // the site-table mutex. Completing across threads IS the
        // regression assertion; nonzero sites prove sampling stayed on.
        let threads: Vec<_> = (0..4)
            .map(|seed| {
                std::thread::spawn(move || {
                    let mut keep = Vec::new();
                    for index in 0..1_000usize {
                        keep.push(black_box(vec![seed as u8; 16 + (index % 512)]));
                    }
                    drop(black_box(keep));
                })
            })
            .collect();
        for thread in threads {
            thread
                .join()
                .expect("sampling worker thread must not panic");
        }

        // Read-time resolution also allocates while armed; returning
        // Some proves the read path tolerates an armed sampler too.
        let json =
            memory_audit_sites(5).expect("armed sampling must produce a sites report, not null");
        let rows = parse_sites(&json);
        assert!(
            !rows.is_empty(),
            "concurrent interval=1 allocation storm must record sites"
        );
        assert!(
            rows.len() <= 5,
            "topK=5 must cap the report (got {})",
            rows.len()
        );
        drop(window);
    }

    #[test]
    fn allocator_shim_anchor_matches_release_demangled_names() {
        // Release LTO builds demangle the allocator entry shims with a
        // crate-root prefix (`__rustc[<hash>]::__rust_alloc`); debug/test
        // builds resolve the bare exported name. The anchor predicate
        // must match BOTH, or release-mode site reports lead with
        // plumbing frames instead of the semantic caller.
        for name in [
            "__rust_alloc",
            "__rust_realloc",
            "__rust_alloc_zeroed",
            "__rustc[d9b87f19e823c0ef]::__rust_alloc",
            "__rustc[d9b87f19e823c0ef]::__rust_realloc",
            "__rustc[d9b87f19e823c0ef]::__rust_alloc_zeroed",
            "__rg_alloc",
            "_rdl_alloc",
        ] {
            assert!(
                sampling::is_allocator_entry_shim_for_tests(name),
                "allocator entry shim must be recognised: {name}"
            );
        }
        for name in [
            "verter_session::meta_resolve::materialize::field_types::reduce",
            "oxc_allocator::arena::alloc_impl::alloc_layout_slow",
            "<hashbrown::raw::RawTable<T,A> as core::clone::Clone>::clone",
        ] {
            assert!(
                !sampling::is_allocator_entry_shim_for_tests(name),
                "semantic caller frames must NOT be classified as shims: {name}"
            );
        }
    }

    #[test]
    fn sample_interval_parsing_rejects_zero_and_garbage() {
        assert_eq!(sampling::parse_interval("97").map(|n| n.get()), Some(97));
        assert_eq!(sampling::parse_interval(" 8 ").map(|n| n.get()), Some(8));
        assert_eq!(
            sampling::parse_interval("0"),
            None,
            "N=0 must stay disarmed"
        );
        assert_eq!(sampling::parse_interval(""), None);
        assert_eq!(sampling::parse_interval("prime"), None);
        assert_eq!(sampling::parse_interval("-3"), None);
    }
}
