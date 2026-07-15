//! Runtime-gated deep memory audit for the native binding.
//!
//! One single binary: the wrapper [`std::alloc::GlobalAlloc`] over
//! [`std::alloc::System`] is ALWAYS compiled. A runtime gate decides the
//! cost:
//!
//! - **Disabled (the default):** every allocator call is a delegate to
//!   `System` plus exactly ONE cached relaxed atomic load + branch — no
//!   counter updates, no thread-local access, no locks, no env reads.
//! - **Enabled:** allocation/deallocation counts, total allocated bytes,
//!   live bytes, and a resettable live-bytes high-water mark are tracked
//!   with relaxed atomics.
//! - **Enabled + sampling armed (`sampleEvery = N`):** every Nth
//!   allocating call additionally captures an UNRESOLVED backtrace into a
//!   bounded call-site table; symbols resolve lazily at
//!   `memoryAuditSites()` read time.
//!
//! Enabling is runtime-only, via either:
//! - `memoryAuditEnable({ sampleEvery? })` — call before queries; or
//! - env `VERTER_MEMORY_AUDIT=1` (+ `VERTER_MEMORY_AUDIT_SAMPLE=N` to arm
//!   sampling; setting the sample var alone also enables). The
//!   environment is read ONCE per process, on the first memory-audit
//!   NAPI call — never on the allocator path.
//!
//! Enabling starts a FRESH counter epoch (counters and the site table
//! reset), so totals reflect post-enable activity only. `liveBytes` is
//! signed: blocks allocated before the epoch and freed after it drive
//! the value negative rather than corrupting the counters.
//!
//! The NAPI surface is always exported; `memoryAuditSnapshot()` /
//! `memoryAuditSites()` return `null` and `memoryAuditResetHighWater()`
//! returns `false` while the gate is disabled.

use napi_derive::napi;

/// Point-in-time counters from the counting global allocator.
///
/// All values are reported as `f64` for plain JS `number` interop; the
/// magnitudes involved (audit windows, bytes) stay far below 2^53.
#[napi(object)]
pub struct NapiMemoryAuditSnapshot {
    /// Total allocating calls (`alloc` / `alloc_zeroed` / `realloc`)
    /// observed since the audit was enabled.
    pub allocCount: f64,
    /// Total deallocating calls (`dealloc` / `realloc`) observed since
    /// the audit was enabled.
    pub deallocCount: f64,
    /// Total bytes requested by allocating calls since the audit was
    /// enabled (monotonic; never decremented on free).
    pub allocatedBytesTotal: f64,
    /// Live heap bytes relative to the enable epoch (allocated minus
    /// freed since enable). Can go NEGATIVE when blocks allocated before
    /// the epoch are freed after it.
    pub liveBytes: f64,
    /// High-water mark of `liveBytes` since enable or the last
    /// `memoryAuditResetHighWater()` call.
    pub peakLiveBytes: f64,
}

/// Options for `memoryAuditEnable`.
#[napi(object)]
pub struct NapiMemoryAuditEnableOptions {
    /// Arm allocation-site sampling: capture one call-site stack every
    /// `sampleEvery` allocating calls (a prime such as 97 is
    /// recommended). `0`/absent leaves sampling off (counters only).
    pub sampleEvery: Option<u32>,
}

/// Runtime gate + env arming. The allocator hot path reads ONLY the
/// [`ENABLED`] atomic (and, once enabled, the sampling-interval atomic);
/// the (allocating) env read happens exclusively on NAPI entry points.
mod runtime {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static ENV_INIT: OnceLock<()> = OnceLock::new();

    /// The single disabled-path cost: one cached relaxed load + branch.
    #[inline]
    pub(super) fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    /// Settle the env gate exactly once per process. Called from every
    /// memory-audit NAPI entry point and NEVER from the allocator path:
    /// reading the environment allocates, and those nested allocations
    /// observe the gate atomics only.
    ///
    /// Test builds ignore the environment entirely (hermeticity: an
    /// exported `VERTER_MEMORY_AUDIT` on a dev machine must not perturb
    /// unrelated lib tests); tests drive the same runtime setters the
    /// NAPI enable path uses.
    pub(super) fn ensure_env_init() {
        ENV_INIT.get_or_init(|| {
            #[cfg(not(test))]
            {
                let enabled_env =
                    std::env::var("VERTER_MEMORY_AUDIT").is_ok_and(|value| value.trim() == "1");
                let sample_env = std::env::var("VERTER_MEMORY_AUDIT_SAMPLE")
                    .ok()
                    .and_then(|raw| super::sampling::parse_interval(&raw));
                if enabled_env || sample_env.is_some() {
                    enable(sample_env.map(|n| n.get()));
                }
            }
        });
    }

    /// Flip the gate on. The false→true transition starts a FRESH epoch:
    /// counters, the site table, and the sampling tick reset so reported
    /// totals cover post-enable activity only. Re-enabling while already
    /// enabled only (re)applies `sample_every`.
    pub(super) fn enable(sample_every: Option<usize>) {
        if !ENABLED.load(Ordering::Relaxed) {
            super::counting::reset_epoch();
            super::sampling::reset_epoch();
            ENABLED.store(true, Ordering::Relaxed);
        }
        if let Some(every) = sample_every {
            super::sampling::set_interval(every);
        }
    }

    #[cfg(test)]
    pub(super) fn disable_for_tests() {
        ENABLED.store(false, Ordering::Relaxed);
        super::sampling::set_interval(0);
    }
}

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

    pub(super) static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
    pub(super) static DEALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
    pub(super) static ALLOCATED_BYTES_TOTAL: AtomicU64 = AtomicU64::new(0);
    /// Signed: enabling mid-process means pre-epoch blocks get freed
    /// after the epoch starts, legitimately driving live below zero.
    pub(super) static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
    pub(super) static PEAK_LIVE_BYTES: AtomicI64 = AtomicI64::new(0);

    /// Always-installed wrapper over the system allocator. Runtime
    /// disabled (the default): delegate + one relaxed load + branch —
    /// nothing else. Enabled: successful allocating calls bump the
    /// counters and advance the high-water mark via `fetch_max`; failed
    /// allocations (null return) are not recorded so `LIVE_BYTES` stays
    /// exact.
    struct CountingAllocator;

    #[inline]
    fn record_alloc(bytes: usize) {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES_TOTAL.fetch_add(bytes as u64, Ordering::Relaxed);
        let live = LIVE_BYTES.fetch_add(bytes as i64, Ordering::Relaxed) + bytes as i64;
        PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
        super::sampling::maybe_sample(bytes);
    }

    #[inline]
    fn record_dealloc(bytes: usize) {
        DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        LIVE_BYTES.fetch_sub(bytes as i64, Ordering::Relaxed);
    }

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if super::runtime::enabled() && !ptr.is_null() {
                record_alloc(layout.size());
            }
            ptr
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if super::runtime::enabled() && !ptr.is_null() {
                record_alloc(layout.size());
            }
            ptr
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if super::runtime::enabled() {
                record_dealloc(layout.size());
            }
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
            if super::runtime::enabled() && !new_ptr.is_null() {
                // Model a successful realloc as one alloc of the new size
                // plus one dealloc of the old size so `LIVE_BYTES` stays
                // exact and both event counters advance.
                record_alloc(new_size);
                record_dealloc(layout.size());
            }
            new_ptr
        }
    }

    #[global_allocator]
    static GLOBAL: CountingAllocator = CountingAllocator;

    pub(super) fn snapshot() -> super::NapiMemoryAuditSnapshot {
        super::NapiMemoryAuditSnapshot {
            allocCount: ALLOC_COUNT.load(Ordering::Relaxed) as f64,
            deallocCount: DEALLOC_COUNT.load(Ordering::Relaxed) as f64,
            allocatedBytesTotal: ALLOCATED_BYTES_TOTAL.load(Ordering::Relaxed) as f64,
            liveBytes: LIVE_BYTES.load(Ordering::Relaxed) as f64,
            peakLiveBytes: PEAK_LIVE_BYTES.load(Ordering::Relaxed) as f64,
        }
    }

    /// Re-arm the high-water mark at the current live-bytes level so the
    /// next window measures its own peak. Best-effort under concurrency
    /// (audit windows are driven by a single JS caller).
    pub(super) fn reset_high_water() {
        PEAK_LIVE_BYTES.store(LIVE_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    /// Fresh-epoch reset, run on the disabled→enabled transition.
    pub(super) fn reset_epoch() {
        ALLOC_COUNT.store(0, Ordering::Relaxed);
        DEALLOC_COUNT.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES_TOTAL.store(0, Ordering::Relaxed);
        LIVE_BYTES.store(0, Ordering::Relaxed);
        PEAK_LIVE_BYTES.store(0, Ordering::Relaxed);
    }
}

/// Sampled allocation-site attribution.
///
/// With sampling armed (`sampleEvery = N` via `memoryAuditEnable` or
/// `VERTER_MEMORY_AUDIT_SAMPLE`), every Nth allocating call captures an
/// UNRESOLVED backtrace and folds it into a bounded per-process site
/// table keyed by a hash of the raw frame ips. Symbols are resolved
/// lazily at `memoryAuditSites()` read time — capture stays cheap and
/// in-process, which is what macOS `malloc_history` cannot give us on
/// these deep resolver stacks.
///
/// Cost contract:
/// - Audit disabled: this module is never reached (the allocator gate
///   returns first).
/// - Enabled, sampling off: one relaxed load + branch per allocation on
///   top of the counters.
/// - Sampling armed: one additional relaxed `fetch_add` + divisibility
///   check per allocation; only every Nth call takes the capture slow
///   path.
///
/// Recursion guard: backtrace capture and site-table mutation allocate,
/// and those nested allocations re-enter the counting allocator. A
/// thread-local `IN_SAMPLER` flag makes them count-only (never
/// re-sample), preventing unbounded self-sampling and same-thread mutex
/// re-entry.
mod sampling {
    use std::cell::Cell;
    use std::hash::{Hash, Hasher};
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use rustc_hash::{FxHashMap, FxHasher};

    /// Bounded site-table capacity: once 4096 distinct sites exist, NEW
    /// sites are dropped (existing hot sites keep accumulating), keeping
    /// sampler memory bounded on pathological stack diversity.
    const MAX_SITES: usize = 4096;
    /// Raw instruction pointers captured per sampled stack.
    const MAX_RAW_FRAMES: usize = 32;
    /// Resolved frames reported per site after plumbing-prefix skipping.
    const MAX_REPORTED_FRAMES: usize = 8;

    /// Sampling interval; `0` = sampling off. Runtime-settable through
    /// `memoryAuditEnable` / the once-per-process env arming.
    static SAMPLE_INTERVAL: AtomicUsize = AtomicUsize::new(0);

    /// Global allocation tick for Nth-call selection (armed mode only).
    static SAMPLE_TICK: AtomicUsize = AtomicUsize::new(0);

    struct SiteRecord {
        count: u64,
        bytes: u64,
        /// Raw ips from the FIRST sample that discovered this site;
        /// resolved lazily at read time.
        stack: Vec<usize>,
    }

    /// `None` until the first sample; `Mutex` is only ever taken on the
    /// sampled slow path and the read path, never on unsampled calls.
    static SITES: Mutex<Option<FxHashMap<u64, SiteRecord>>> = Mutex::new(None);

    thread_local! {
        static IN_SAMPLER: Cell<bool> = const { Cell::new(false) };
    }

    /// RAII sampler suppression for the current thread. `enter` returns
    /// `None` when the thread is already inside the sampler (recursion)
    /// or its TLS is being torn down — callers must skip sampling then.
    struct SamplerSuppressGuard;

    impl SamplerSuppressGuard {
        fn enter() -> Option<Self> {
            let entered = IN_SAMPLER
                .try_with(|flag| {
                    if flag.get() {
                        false
                    } else {
                        flag.set(true);
                        true
                    }
                })
                .unwrap_or(false);
            // Explicit branch, NOT `entered.then_some(SamplerSuppressGuard)`:
            // `then_some` constructs its argument EAGERLY and drops it when
            // the bool is false — and this guard's Drop clears the flag, so
            // the eager-drop would cancel the suppression an OUTER holder
            // still relies on. That exact footgun deadlocked the sampler on
            // the site-table mutex (nested allocation inside the locked
            // insert passed a guard its parent believed was still armed).
            if entered {
                Some(SamplerSuppressGuard)
            } else {
                None
            }
        }
    }

    impl Drop for SamplerSuppressGuard {
        fn drop(&mut self) {
            let _ = IN_SAMPLER.try_with(|flag| flag.set(false));
        }
    }

    pub(super) fn parse_interval(raw: &str) -> Option<NonZeroUsize> {
        raw.trim().parse::<usize>().ok().and_then(NonZeroUsize::new)
    }

    pub(super) fn set_interval(interval: usize) {
        SAMPLE_INTERVAL.store(interval, Ordering::Relaxed);
    }

    /// Fresh-epoch reset, run on the disabled→enabled transition.
    pub(super) fn reset_epoch() {
        SAMPLE_TICK.store(0, Ordering::Relaxed);
        let mut guard = SITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = None;
    }

    #[inline]
    fn armed_interval() -> Option<NonZeroUsize> {
        NonZeroUsize::new(SAMPLE_INTERVAL.load(Ordering::Relaxed))
    }

    /// Hot-path hook — called from `record_alloc` for every successful
    /// allocating call while the audit is ENABLED. See the module docs
    /// for the exact cost contract.
    #[inline]
    pub(super) fn maybe_sample(bytes: usize) {
        let Some(interval) = armed_interval() else {
            return;
        };
        let tick = SAMPLE_TICK.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        if !tick.is_multiple_of(interval.get()) {
            return;
        }
        sample_slow(bytes);
    }

    #[cold]
    #[inline(never)]
    fn sample_slow(bytes: usize) {
        let Some(_suppress) = SamplerSuppressGuard::enter() else {
            return;
        };

        // Fixed-size raw capture: no heap allocation before the guard
        // matters, and deep stacks truncate instead of growing.
        let mut ips = [0usize; MAX_RAW_FRAMES];
        let mut len = 0usize;
        backtrace::trace(|frame| {
            ips[len] = frame.ip() as usize;
            len += 1;
            len < MAX_RAW_FRAMES
        });
        let stack = &ips[..len];

        let mut hasher = FxHasher::default();
        stack.hash(&mut hasher);
        let key = hasher.finish();

        let mut guard = SITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let table = guard.get_or_insert_with(FxHashMap::default);
        if let Some(record) = table.get_mut(&key) {
            record.count += 1;
            record.bytes += bytes as u64;
        } else if table.len() < MAX_SITES {
            table.insert(
                key,
                SiteRecord {
                    count: 1,
                    bytes: bytes as u64,
                    stack: stack.to_vec(),
                },
            );
        }
    }

    /// One reported site row. Serialized field names are the wire
    /// contract consumed by `packages/benchmark` (`.profile.json` sites).
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SiteReportRow {
        count: u64,
        bytes: u64,
        /// `bytes * interval`: the unbiased estimate of the total bytes
        /// this site allocated, given uniform every-Nth sampling.
        estimated_total_bytes: u64,
        frames: Vec<String>,
    }

    /// Resolve + report the top-`top_k` sites by sampled bytes. `None`
    /// when sampling is not armed — callers treat that identically to a
    /// disabled audit for this surface.
    pub(super) fn sites_json(top_k: u32) -> Option<String> {
        let interval = armed_interval()?;

        // Suppress self-sampling for the WHOLE read BEFORE touching the
        // table lock: the copy-out below allocates while HOLDING the
        // lock, and an armed sampler hitting its Nth tick there would
        // re-enter the mutex on this same thread (deadlock — caught by
        // the sampling tests). `enter()` returning `None` (this thread
        // is already inside the sampler) is equally safe to proceed
        // under: `sample_slow` checks the same thread-local flag.
        let _suppress = SamplerSuppressGuard::enter();

        // Copy the table out under the lock, then resolve symbols
        // OUTSIDE it: symbolication is the expensive allocating part
        // and must not extend the sampler's critical section.
        let mut rows: Vec<(u64, u64, Vec<usize>)> = {
            let guard = SITES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match guard.as_ref() {
                None => Vec::new(),
                Some(table) => table
                    .values()
                    .map(|record| (record.count, record.bytes, record.stack.clone()))
                    .collect(),
            }
        };
        rows.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        rows.truncate(top_k as usize);

        let n = interval.get() as u64;
        let report: Vec<SiteReportRow> = rows
            .into_iter()
            .map(|(count, bytes, stack)| SiteReportRow {
                count,
                bytes,
                estimated_total_bytes: bytes.saturating_mul(n),
                frames: resolve_display_frames(&stack),
            })
            .collect();
        serde_json::to_string(&report).ok()
    }

    /// Lazy read-time resolution: symbolicate every captured ip, cut the
    /// LEADING plumbing (mid-stack frames are never filtered), and cap
    /// the report at [`MAX_REPORTED_FRAMES`]. Unresolvable ips degrade
    /// to hex.
    ///
    /// The cut is anchored on the deepest allocator ENTRY SHIM frame
    /// (`__rust_alloc*` and friends — stable exported names in every
    /// build): everything at or below it is sampler/allocator plumbing
    /// by construction, because nested allocations never capture (the
    /// sampler is suppressed while one sample is in flight), so at most
    /// one shim run exists per stack. A predicate pass then drops the
    /// remaining run of alloc-internal frames (`raw_vec`, `alloc_impl`,
    /// ...). Anchor missing (fully inlined shim) ⇒ predicate pass only.
    fn resolve_display_frames(stack: &[usize]) -> Vec<String> {
        let mut resolved: Vec<String> = Vec::with_capacity(stack.len());
        for &ip in stack {
            let mut name: Option<String> = None;
            backtrace::resolve(ip as *mut std::ffi::c_void, |symbol| {
                if name.is_none() {
                    name = symbol.name().map(|n| n.to_string());
                }
            });
            resolved.push(name.unwrap_or_else(|| format!("{ip:#x}")));
        }
        let after_anchor = resolved
            .iter()
            .rposition(|frame| is_allocator_entry_shim(frame))
            .map_or(0, |index| index + 1);
        let start = resolved
            .iter()
            .enumerate()
            .skip(after_anchor)
            .find(|(_, frame)| !is_allocator_plumbing_frame(frame))
            .map_or(after_anchor, |(index, _)| index);
        resolved
            .into_iter()
            .skip(start)
            .take(MAX_REPORTED_FRAMES)
            .collect()
    }

    /// Allocator entry shims — the innermost stable anchor of every
    /// sampled stack. CONTAINS matching, not prefix: release LTO builds
    /// demangle the shims with a crate-root prefix
    /// (`__rustc[<hash>]::__rust_alloc`) while debug builds resolve the
    /// bare exported name; no semantic caller frame legitimately
    /// contains these reserved double-underscore names.
    fn is_allocator_entry_shim(name: &str) -> bool {
        name.contains("__rust_alloc")
            || name.contains("__rust_realloc")
            || name.contains("__rg_")
            || name.contains("_rdl_")
    }

    #[cfg(test)]
    pub(super) fn is_allocator_entry_shim_for_tests(name: &str) -> bool {
        is_allocator_entry_shim(name)
    }

    /// Frames that are allocation plumbing rather than attribution-
    /// relevant call sites. Bare names are EXACT matches because macOS
    /// DWARF resolution can drop module paths for local functions;
    /// deliberately NOT matching this crate's test modules or
    /// `alloc::vec::` (a `Vec::push`/`from_elem` caller IS attribution).
    fn is_allocator_plumbing_frame(name: &str) -> bool {
        matches!(
            name,
            "trace"
                | "sample_slow"
                | "maybe_sample"
                | "record_alloc"
                | "record_dealloc"
                | "alloc"
                | "alloc_zeroed"
                | "realloc"
                | "dealloc"
                | "alloc_impl"
                | "grow_impl"
        ) || name.starts_with("trace<")
            || is_allocator_entry_shim(name)
            || name.contains("backtrace::")
            // Module-path form (trailing `::`) so `sampling_tests` /
            // `counting`-adjacent TEST module frames never classify as
            // plumbing.
            || name.contains("memory_audit::sampling::")
            || name.contains("memory_audit::counting::")
            || name.contains("CountingAllocator")
            || name.contains("std::alloc::")
            || name.contains("alloc::alloc::")
            || name.contains("alloc::raw_vec::")
    }
}

/// Enable the runtime memory audit (idempotent). The disabled→enabled
/// transition starts a fresh counter epoch. Pass `{ sampleEvery: N }`
/// (N > 0; a prime such as 97 is recommended) to also arm allocation-
/// site sampling for `memoryAuditSites()`. Returns `true` (the audit is
/// enabled after this call). Call BEFORE the workload of interest —
/// enabling mid-flight is safe but the counters then cover a partial
/// window.
#[napi]
pub fn memory_audit_enable(options: Option<NapiMemoryAuditEnableOptions>) -> bool {
    runtime::ensure_env_init();
    runtime::enable(options.and_then(|options| options.sampleEvery.map(|every| every as usize)));
    true
}

/// Return the current allocator counters, or `null` while the runtime
/// audit gate is disabled (the default — one cached branch of overhead).
#[napi]
pub fn memory_audit_snapshot() -> Option<NapiMemoryAuditSnapshot> {
    runtime::ensure_env_init();
    if !runtime::enabled() {
        return None;
    }
    Some(counting::snapshot())
}

/// Reset the live-bytes high-water mark to the current live-bytes level.
/// Returns `false` while the runtime audit gate is disabled.
#[napi]
pub fn memory_audit_reset_high_water() -> bool {
    runtime::ensure_env_init();
    if !runtime::enabled() {
        return false;
    }
    counting::reset_high_water();
    true
}

/// JSON report of the top-`top_k` sampled allocation sites by sampled
/// bytes — `[{count, bytes, estimatedTotalBytes, frames}, ...]` where
/// `estimatedTotalBytes = bytes * N` (the armed `sampleEvery` interval)
/// and `frames` is a short resolved stack (≤ 8 frames, leading
/// allocator/backtrace plumbing skipped). Returns `null` while the audit
/// is disabled OR while sampling is not armed (no `sampleEvery` /
/// `VERTER_MEMORY_AUDIT_SAMPLE`).
#[napi]
pub fn memory_audit_sites(top_k: u32) -> Option<String> {
    runtime::ensure_env_init();
    if !runtime::enabled() {
        return None;
    }
    sampling::sites_json(top_k)
}

/// Shared serialization + gate windows for the memory-audit tests. The
/// runtime gate, counters, and site table are process-global, so every
/// test that touches them serialises on one mutex and restores the
/// disabled state on drop.
#[cfg(test)]
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
#[cfg(test)]
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

#[cfg(test)]
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

/// Sampled allocation-site attribution. Arming goes through the
/// production `memoryAuditEnable` path; every window serialises on the
/// shared mutex and disarms on drop so armed intervals never leak into
/// sibling tests.
#[cfg(test)]
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
