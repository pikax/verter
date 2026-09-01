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
//!   with lock-free atomics. Live bytes and the high-water mark are two
//!   separate counters advanced in two steps, so the reader — not the
//!   record path — is what makes the reported pair coherent: every
//!   snapshot satisfies `peakLiveBytes >= liveBytes`. Re-arming the mark
//!   carries in every block still live when it completes; a block
//!   allocated AND freed entirely inside the re-arm is a transient that
//!   two separate counters cannot recover, and audit windows are driven
//!   by a single JS caller, so that case is not a live concern.
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
///
/// The pair `(peakLiveBytes, liveBytes)` is COHERENT: every returned
/// snapshot satisfies `peakLiveBytes >= liveBytes`, whatever the
/// allocator was doing on other threads while it was taken.
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
    ///
    /// ALWAYS at least the `liveBytes` of the same snapshot. The reader
    /// folds the live value it observed into the mark, so this holds even
    /// when the read lands between the record path's two publication
    /// steps. The reported value is always a live-bytes total the process
    /// actually reached — normally one from the current window, though a
    /// read whose fold lands after a concurrent re-arm can carry a value
    /// over from the preceding one.
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
        // `Release` publishes the live value that produced this mark: a thread
        // that acquire-reads the mark is then guaranteed to also observe the
        // `fetch_add` above. It is defence in depth, not the load-bearing
        // part — the re-arm's guarantee comes from re-reading live through a
        // read-modify-write, which needs no pairing — but it keeps the
        // re-arm's case analysis local to that function.
        PEAK_LIVE_BYTES.fetch_max(live, Ordering::Release);
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

    /// Point-in-time read of the counters.
    ///
    /// The record path advances live bytes and the high-water mark in two
    /// separate atomic steps, so a reader can land between them and find a
    /// mark the live value has already overtaken. Reading the two counters
    /// independently — in either order — therefore cannot produce a coherent
    /// pair: it can only make the gap rarer, never impossible.
    ///
    /// Instead the observed live value is folded INTO the mark and the pair is
    /// reported as `(max(mark, live), live)`. That satisfies
    /// `peakLiveBytes >= liveBytes` by construction, for every interleaving,
    /// with no window at all — the relationship is arithmetic on two values
    /// this thread already holds, not a bet on how the two loads interleaved.
    /// The reported mark is still a value live bytes actually reached: either
    /// an earlier high-water mark, or the live value published alongside it.
    ///
    /// The fold is written back, so reading MUTATES: it heals the stored mark
    /// at the instant of the read, and the next allocation reopens the gap
    /// again between its `fetch_add` and its `fetch_max`. The write-back needs
    /// no release ordering — nothing precedes it here but loads, so it has
    /// nothing to publish, and it stays inside any release sequence the record
    /// path headed.
    pub(super) fn snapshot() -> super::NapiMemoryAuditSnapshot {
        let alloc_count = ALLOC_COUNT.load(Ordering::Relaxed);
        let dealloc_count = DEALLOC_COUNT.load(Ordering::Relaxed);
        let allocated_bytes_total = ALLOCATED_BYTES_TOTAL.load(Ordering::Relaxed);
        let live = LIVE_BYTES.load(Ordering::Relaxed);
        let peak = PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed).max(live);
        super::NapiMemoryAuditSnapshot {
            allocCount: alloc_count as f64,
            deallocCount: dealloc_count as f64,
            allocatedBytesTotal: allocated_bytes_total as f64,
            liveBytes: live as f64,
            peakLiveBytes: peak as f64,
        }
    }

    /// Re-arm the high-water mark at the current live-bytes level so the
    /// next window measures its own peak.
    ///
    /// What the call guarantees, exactly: when it returns, the mark is at
    /// least the live-bytes total its trailing fold observed, so every block
    /// still live at that point is inside the new window.
    ///
    /// It does NOT guarantee that the new window's mark reaches every height
    /// live bytes touched while the re-arm ran. A block allocated AND freed
    /// entirely inside the re-arm raises live only transiently: its own
    /// `fetch_max` can no-op against the not-yet-lowered pre-reset mark —
    /// leaving nothing for the exchange below to detect — and by the time the
    /// fold reads live the block is already gone, so its height is lost.
    /// Recovering it would mean carrying live bytes and the mark in ONE
    /// atomic, updated together on the allocator hot path; this module does
    /// not make that trade. Audit windows are driven by a single JS caller,
    /// so a re-arm concurrent with allocation is already the exceptional case.
    ///
    /// Within that guarantee, this cannot be a `PEAK.store(LIVE.load())`. The
    /// counters are separate atomics, so a plain load/store pair has two ways
    /// to drop a still-live block out of the new window: it can read a live
    /// value that predates an allocation the mark has already absorbed and
    /// then overwrite that allocation's height, and it can overwrite a mark an
    /// allocation raised between the load and the store. The exchange loop
    /// closes both, and the trailing fold covers the third case:
    ///
    /// - the live value is read with a read-modify-write, which by definition
    ///   returns the last value in `LIVE_BYTES`' modification order, so the
    ///   new baseline can never be a stale live value;
    /// - the mark is replaced with `compare_exchange`, so an allocation that
    ///   raised it between the read and the exchange forces a retry against
    ///   the fresher mark instead of being clobbered;
    /// - the trailing `fetch_max` re-reads live through the same
    ///   read-modify-write, so a still-live allocation whose own `fetch_max`
    ///   no-opped against the pre-reset mark still enters the new window. This
    ///   fold is what actually carries the guarantee: an RMW against
    ///   `LIVE_BYTES`' modification order needs no pairing with the record
    ///   path to be current.
    ///
    /// The loop is lock-free rather than bounded: an exchange fails only when
    /// another thread changed the mark, so every failure is another thread's
    /// progress. The mark does not move only upward — this function lowers it,
    /// and a fresh epoch stores zero — so there is no monotonic bound to
    /// appeal to.
    pub(super) fn reset_high_water() {
        let mut mark = PEAK_LIVE_BYTES.load(Ordering::Acquire);
        loop {
            let live = current_live_bytes();
            match PEAK_LIVE_BYTES.compare_exchange(mark, live, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(current) => mark = current,
            }
        }
        PEAK_LIVE_BYTES.fetch_max(current_live_bytes(), Ordering::Release);
    }

    /// Live bytes read as a read-modify-write rather than a load: an RMW
    /// always reads the last value in the counter's modification order, so the
    /// caller cannot act on a live value an already-recorded allocation has
    /// superseded. Only the high-water re-arm needs this; the allocator hot
    /// path never calls it.
    ///
    /// The zero addend is the cheapest way to spell that RMW, and `AcqRel` is
    /// chosen because release-or-stronger is the one class LLVM's
    /// idempotent-RMW fold will not rewrite into a plain load. On the pinned
    /// toolchain no ordering is folded today, so that choice guards against a
    /// future optimiser rather than describing a current one. Keep it an RMW.
    #[inline]
    fn current_live_bytes() -> i64 {
        LIVE_BYTES.fetch_add(0, Ordering::AcqRel)
    }

    /// Fresh-epoch reset, run on the disabled→enabled transition.
    pub(super) fn reset_epoch() {
        ALLOC_COUNT.store(0, Ordering::Relaxed);
        DEALLOC_COUNT.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES_TOTAL.store(0, Ordering::Relaxed);
        // Clearing is safe because of WHERE this runs, not because of the
        // order of these two stores: `runtime::enable` calls it on the
        // false→true transition only, BEFORE the gate is switched on, so in
        // the ordinary case no record call is in flight against the counters
        // being cleared. That check-then-act is not atomic, so two concurrent
        // enables can both clear; the fold below is what makes even that
        // coherent. Both
        // stores are relaxed and target different atomics, so nothing forces
        // an observer to see them in this order — the mark-then-live sequence
        // is defence in depth, not a closed window — and if a straggler ever
        // did land, `snapshot`'s fold keeps the reported pair coherent anyway.
        PEAK_LIVE_BYTES.store(0, Ordering::Relaxed);
        LIVE_BYTES.store(0, Ordering::Relaxed);
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
///
/// Reading MUTATES: `peakLiveBytes` is the stored high-water mark folded
/// with the `liveBytes` of the same read, and that fold is written back,
/// so a snapshot can advance the mark it reports.
#[napi]
pub fn memory_audit_snapshot() -> Option<NapiMemoryAuditSnapshot> {
    runtime::ensure_env_init();
    if !runtime::enabled() {
        return None;
    }
    Some(counting::snapshot())
}

/// Reset the live-bytes high-water mark to the current live-bytes level,
/// starting a fresh measurement window. Every block still live when the
/// call returns is carried into the new window rather than dropped from
/// it; a block allocated and freed entirely while the re-arm runs can
/// still be missed (see `counting::reset_high_water`). Returns `false`
/// while the runtime audit gate is disabled.
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

#[cfg(test)]
#[path = "memory_audit_tests.rs"]
mod memory_audit_tests;
