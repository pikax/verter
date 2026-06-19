#![deny(missing_docs)]
//! `HostAuditRuntime` — host-owned concrete audit runtime.
//!
//! Wraps the [`AuditRecordsStore`] instance, the `AuditConfig`
//! snapshot, and the active-request registry that
//! [`AuditRequestRegistration`] populates. The
//! `active_requests` field is **private** so callers cannot
//! mutate the map outside the three crate-private surface methods
//! `register_active_request`, `finalize_active_request`, and
//! `drop_active_request`. Tests observe the runtime via the public
//! read-only [`HostAuditRuntime::snapshot`] accessor.
//!
//! Each runtime owns at most ONE peak-RSS sampler thread on
//! native targets. The thread spawns lazily on the first
//! `AuditRequestRegistration::new` call when
//! `AuditConfig::audit_timing_capture` is enabled, holds a
//! `Weak<HostAuditRuntime>` to break the runtime↔thread cycle,
//! ticks every 50 ms, and writes
//! `fetch_max(current_process_rss())` into each in-flight
//! request's per-request peak slot. The runtime's `Drop` impl
//! joins the handle so dropped hosts do not leak threads. WASM
//! targets are gated off via `#[cfg(not(target_arch = "wasm32"))]`
//! — `process_rss_peak_bytes` stays at `0` there regardless of
//! flag state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use verter_audit::{AuditConfig, RequestAuditRecord};

use crate::component_meta_audit::AuditRecordsStore;
use crate::request_context::RequestContext;

#[cfg(not(target_arch = "wasm32"))]
use std::thread::JoinHandle;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// Sampler tick interval. 50 ms strikes the plan-§5 balance between
/// responsiveness (bounded peak under-reporting) and CPU cost
/// (~0.1% of one core at this rate).
#[cfg(not(target_arch = "wasm32"))]
const SAMPLER_TICK: Duration = Duration::from_millis(50);

/// Host-owned audit-runtime concrete type. Wraps the records store,
/// the audit-config snapshot, and the active-request registry.
///
/// The records store is consumer-visible via
/// [`Self::take_record`], [`Self::insert_record`], and
/// [`Self::audit_records_store`]. The active-request registry is
/// strictly behind crate-private surface methods so the
/// `AuditRequestRegistration` lifecycle remains the single
/// authority for inserts and removes.
///
/// On native targets the runtime also owns the at-most-one
/// peak-RSS sampler thread. The thread spawns lazily on the
/// first audit-enabled `AuditRequestRegistration::new` call
/// while `AuditConfig::audit_timing_capture` is on; the join
/// handle lives in `sampler_thread` and is taken+joined by the
/// `Drop` impl. Subsequent `AuditRequestRegistration::new` calls
/// short-circuit the spawn via the `sampler_started` flag (single
/// startup transition guarded by `compare_exchange`). On WASM the
/// sampler does not exist (`#[cfg(not(target_arch = "wasm32"))]`);
/// `process_rss_peak_bytes` stays at `0` regardless of flag state.
pub struct HostAuditRuntime {
    config: Arc<AuditConfig>,
    records: Arc<AuditRecordsStore>,
    /// PRIVATE — direct access from outside this module is impossible.
    /// The three crate-private methods below mediate every access.
    active_requests: RwLock<FxHashMap<u64, Weak<RequestContext>>>,
    /// One-shot start latch for the sampler thread. `false` means
    /// the runtime has not yet spawned a sampler. The first
    /// `compare_exchange` to `true` wins the spawn and stores the
    /// `JoinHandle` in `sampler_thread`.
    #[cfg(not(target_arch = "wasm32"))]
    sampler_started: AtomicBool,
    /// Optional `JoinHandle` for the host-owned sampler thread.
    /// `Some` after the latch transition succeeds; `None`
    /// otherwise. The `Drop` impl takes this and calls `join()`.
    #[cfg(not(target_arch = "wasm32"))]
    sampler_thread: parking_lot::Mutex<Option<JoinHandle<()>>>,
    /// Host-owned join observable. Set to `true` by THIS runtime's
    /// `Drop` impl after — and only after — a CLEAN `JoinHandle::join()`
    /// on the owned sampler. Held behind an `Arc` so a test can clone
    /// the observable (via [`Self::sampler_join_observer`]) BEFORE
    /// dropping the host and read it AFTER the host (and thus the
    /// runtime) is gone. Scoped to a single host: it discriminates a
    /// non-joining `Drop` (observable stays `false`) and a panicked
    /// sampler join (also `false`) from a clean join (observable flips
    /// `true`) for THIS host alone — immune to concurrent samplers
    /// spawned/joined by sibling tests.
    #[cfg(not(target_arch = "wasm32"))]
    sampler_join_observed: Arc<AtomicBool>,
}

impl std::fmt::Debug for HostAuditRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostAuditRuntime")
            .field("config", &self.config)
            .field("records", &"<AuditRecordsStore>")
            .field("active_requests_count", &self.active_requests.read().len())
            .finish()
    }
}

impl HostAuditRuntime {
    /// Construct a new runtime. Each `VerterHost` owns one independent
    /// runtime; multiple hosts in one process do NOT share audit state.
    /// The host-owned sampler thread does NOT spawn here — it spawns
    /// lazily on the first `AuditRequestRegistration::new` call when
    /// `audit_timing_capture` is enabled, so a host that never runs an
    /// audited request never spends a thread.
    #[must_use]
    pub fn new(config: AuditConfig, records: Arc<AuditRecordsStore>) -> Self {
        Self {
            config: Arc::new(config),
            records,
            active_requests: RwLock::new(FxHashMap::default()),
            #[cfg(not(target_arch = "wasm32"))]
            sampler_started: AtomicBool::new(false),
            #[cfg(not(target_arch = "wasm32"))]
            sampler_thread: parking_lot::Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            sampler_join_observed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Test-only — `true` once THIS runtime has spawned its peak-RSS
    /// sampler thread (the `sampler_started` latch has fired). Used by
    /// the host-drop test to assert the sampler spawned for an
    /// audit-enabled host, reading only this runtime's per-host latch.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn sampler_spawned(&self) -> bool {
        self.sampler_started.load(Ordering::Acquire)
    }

    /// Test-only — a clone of THIS runtime's join observable. Clone it
    /// BEFORE dropping the host; after the host (and runtime) drop, the
    /// observable reads `true` iff this runtime's `Drop` joined its
    /// sampler thread. A non-joining `Drop` leaves it `false`. Immune
    /// to concurrent samplers in sibling tests because it observes only
    /// this host's join.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn sampler_join_observer(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.sampler_join_observed)
    }

    /// Borrow the audit-config snapshot. Read-only — the host
    /// updates the runtime as a whole when configuration changes.
    #[must_use]
    pub fn audit_config(&self) -> Arc<AuditConfig> {
        Arc::clone(&self.config)
    }

    /// Borrow the underlying records store. Consumers (NAPI / WASM /
    /// LSP) read records through this accessor; producers insert via
    /// [`Self::finalize_active_request`].
    #[must_use]
    pub fn audit_records_store(&self) -> &Arc<AuditRecordsStore> {
        &self.records
    }

    /// Public read-only snapshot of in-flight audit state. Tests
    /// probe lifecycle invariants by calling
    /// `host.host_audit_runtime().snapshot().contains_active_request(id)`.
    /// Mutation is impossible — the snapshot is owned data with no
    /// back-reference.
    #[must_use]
    pub fn snapshot(&self) -> AuditRuntimeSnapshot {
        let map = self.active_requests.read();
        let mut active_request_ids: Vec<u64> = map.keys().copied().collect();
        active_request_ids.sort_unstable();
        active_request_ids.dedup();
        let active_request_count = active_request_ids.len();
        let records_size = self.records.len();
        AuditRuntimeSnapshot {
            active_request_count,
            active_request_ids,
            records_store_size: records_size,
            records_store_capacity: crate::component_meta_audit::AUDIT_RECORDS_STORE_CAPACITY,
        }
    }

    /// Take the audit record published for `request_id`, removing it
    /// from the records store. Mirrors the existing
    /// [`AuditRecordsStore::take`] surface.
    #[must_use]
    pub fn take_record(&self, request_id: u64) -> Option<RequestAuditRecord> {
        self.records.take(request_id)
    }

    /// Crate-private. Called ONLY by `AuditRequestRegistration::new`
    /// to insert a `Weak<RequestContext>` into the active-request
    /// registry. The architecture guard
    /// `audit_request_registration_lifecycle` enforces the single
    /// in-tree call site.
    pub(crate) fn register_active_request(&self, request_id: u64, ctx: &Arc<RequestContext>) {
        // Seed the per-request peak-RSS slot with one immediate sample
        // at registration. The 50ms-cadence sampler thread (see
        // `sampler_loop`) only RAISES the slot via `fetch_max`, so a
        // trivial request that finishes inside a sampler gap would
        // otherwise report a peak of `0`. Seeding here initializes the
        // slot to the RSS at the request's start; the sampler later
        // raises it if memory grows. We reuse the SAME RSS primitive
        // the sampler uses and the SAME `fetch_max` write, so there is
        // no double-count and no cross-request misattribution — the
        // slot is per-request and `fetch_max` is idempotent under
        // re-sampling.
        //
        // The seed is gated on the SAME flag that governs the sampler
        // (`audit_timing_capture`): the seed and the sampler are one
        // peak-RSS feature, both off the request's hot path when the
        // flag is disabled. This preserves the documented contract that
        // `process_rss_peak_bytes` stays `0` when `audit_timing_capture`
        // is off (see `RequestContext::process_rss_peak_bytes` doc and
        // `memory_peak_rss_zero_when_flag_off`), and the zero-cost path
        // for hosts that don't opt into timing capture. On `wasm32` the
        // sampler is gated off entirely; `current_process_rss()` also
        // returns `0` there, so the seed would be a no-op regardless.
        if self.config.audit_timing_capture {
            ctx.process_rss_peak_bytes.fetch_max(
                verter_audit::current_process_rss(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        let mut map = self.active_requests.write();
        map.insert(request_id, Arc::downgrade(ctx));
    }

    /// Crate-private. Called ONLY by
    /// `AuditRequestRegistration::finalize` to atomically remove the
    /// entry from the active-request registry AND publish the
    /// finalised record into the records store.
    pub(crate) fn finalize_active_request(&self, request_id: u64, record: RequestAuditRecord) {
        let mut map = self.active_requests.write();
        map.remove(&request_id);
        drop(map); // release before insertion to avoid lock-order coupling
        self.records.insert(record);
    }

    /// Crate-private. Called ONLY by `AuditRequestRegistration::drop`
    /// (defensive cleanup on panic / cancellation paths). Removes the
    /// entry from the active-request registry; does NOT publish a
    /// record — the absence of a record is itself observable.
    pub(crate) fn drop_active_request(&self, request_id: u64) {
        let mut map = self.active_requests.write();
        map.remove(&request_id);
    }

    /// Crate-private. Sampler-internal accessor — invokes `f` on
    /// every live `Arc<RequestContext>` currently in the
    /// active-request registry. Skips `Weak` slots whose strong
    /// count has dropped to zero. Used by the host-owned peak-RSS
    /// sampler thread to advance each in-flight request's
    /// `process_rss_peak_bytes` slot via `fetch_max`.
    ///
    /// The closure runs while a read-lock is held, so it MUST NOT
    /// re-enter the registry. The sampler intentionally only does
    /// `fetch_max` on a per-context atomic — that operation is
    /// lock-free and can never deadlock.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn for_each_active_request<F>(&self, mut f: F)
    where
        F: FnMut(&Arc<RequestContext>),
    {
        let map = self.active_requests.read();
        for weak in map.values() {
            if let Some(ctx) = weak.upgrade() {
                f(&ctx);
            }
        }
    }

    /// Spawn the host-owned peak-RSS sampler thread (native only).
    ///
    /// Called by `AuditRequestRegistration::new` whenever an
    /// `Active` registration is constructed AND the audit-config
    /// has `audit_timing_capture = true`. The first call wins the
    /// `compare_exchange` on `sampler_started`, spawns the
    /// thread, and stores the `JoinHandle`. Subsequent calls
    /// short-circuit. The thread holds a `Weak<HostAuditRuntime>`
    /// so the runtime↔thread cycle is broken — the runtime can
    /// drop, the next `weak.upgrade()` returns `None`, and the
    /// thread terminates. The `Drop` impl explicitly joins the
    /// handle to avoid leaking threads across host drops.
    ///
    /// On WASM this method is gated off via
    /// `#[cfg(not(target_arch = "wasm32"))]`; the WASM target
    /// has no host-owned sampler.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn ensure_sampler_started(self: &Arc<Self>) {
        if !self.config.audit_timing_capture {
            return;
        }
        // Single-shot start latch: the first compare_exchange
        // winner spawns; everyone else short-circuits.
        if self
            .sampler_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let weak: Weak<HostAuditRuntime> = Arc::downgrade(self);
        let handle = std::thread::Builder::new()
            .name("verter-audit-rss-sampler".to_string())
            .spawn(move || sampler_loop(weak))
            .expect("spawning the verter-audit-rss-sampler thread must succeed");
        // The `sampler_started` latch above is THIS host's spawn signal,
        // read back per-host via `sampler_spawned()`.
        let mut slot = self.sampler_thread.lock();
        debug_assert!(
            slot.is_none(),
            "sampler_started latch must guarantee a single spawn",
        );
        *slot = Some(handle);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for HostAuditRuntime {
    fn drop(&mut self) {
        // Take the join handle (if any) and explicitly join it. By
        // the time `Drop` runs, the strong count on `Arc<Self>` is
        // zero, so the sampler's `Weak::upgrade()` on its next
        // iteration returns `None` and the thread breaks out of
        // its loop. The join just waits for that natural
        // termination.
        let handle = self.sampler_thread.lock().take();
        if let Some(handle) = handle {
            // join() returns Err only if the thread panicked. A
            // panicked sampler does not threaten the host, and there is
            // no way to surface the error from Drop, so we do not
            // propagate it. We DO discriminate on it: the join outcome
            // gates the observable below.
            let joined_cleanly = handle.join().is_ok();
            // Flip the host-owned observable AFTER — and only after — a
            // CLEAN join, so a test holding a clone (taken before the
            // host dropped) can confirm THIS host joined its sampler
            // without a thread panic. `Release` pairs with the test's
            // `Acquire` load. A non-joining Drop never reaches this
            // store, and a panicked sampler (`Err`) leaves it `false`
            // too — the observable asserts a clean shutdown, not merely
            // that join returned (discrimination preserved on both
            // legs).
            if joined_cleanly {
                self.sampler_join_observed.store(true, Ordering::Release);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn sampler_loop(weak: Weak<HostAuditRuntime>) {
    // The sampler loop ticks every 50 ms while the runtime is
    // alive. On each tick it samples `current_process_rss()` once
    // and writes `fetch_max` into every in-flight request's
    // per-request peak slot. The sample-once-per-tick discipline
    // means N in-flight requests share the same value on the
    // same tick, which matches the contract: each request's
    // `process_rss_peak_bytes` is the highest sample taken
    // anywhere in its in-flight window, regardless of which
    // sibling request was concurrent.
    loop {
        let runtime = match weak.upgrade() {
            Some(r) => r,
            None => return, // host dropped — exit cleanly.
        };
        let now = verter_audit::current_process_rss();
        runtime.for_each_active_request(|ctx| {
            ctx.process_rss_peak_bytes.fetch_max(now, Ordering::Relaxed);
        });
        // Drop the upgraded `Arc` BEFORE we sleep so the
        // strong-count lifecycle of the runtime is bounded by
        // the host. If we held the Arc across the sleep, the
        // host's drop would block until the next iteration.
        drop(runtime);
        std::thread::sleep(SAMPLER_TICK);
    }
}

/// Read-only snapshot of in-flight audit state. Returned by
/// [`HostAuditRuntime::snapshot`]; safe for tests to assert against
/// without holding any lock.
#[derive(Debug, Clone)]
pub struct AuditRuntimeSnapshot {
    /// Number of in-flight registrations at sample time.
    pub active_request_count: usize,
    /// Sorted, deduped list of active request ids at sample time.
    pub active_request_ids: Vec<u64>,
    /// Number of records currently held in the records store.
    pub records_store_size: usize,
    /// Bound on the records store size (FIFO eviction at capacity).
    pub records_store_capacity: usize,
}

impl AuditRuntimeSnapshot {
    /// `true` if `request_id` was present in the active-request
    /// registry at the moment the snapshot was taken.
    #[must_use]
    pub fn contains_active_request(&self, request_id: u64) -> bool {
        self.active_request_ids.binary_search(&request_id).is_ok()
    }
}

/// Logical-request-scoped registration object.
///
/// `Active(...)` captures a slot in the host's active-request
/// registry; `Noop` means the audit-config filter rejected the kind
/// at registration time and downstream emits no record.
///
/// Constructed via [`Self::new`]. Finalised by [`Self::finalize`]
/// (idempotent). Defensive `Drop` cleans up the active-request
/// registry entry on panic / cancellation paths.
#[derive(Debug)]
pub enum AuditRequestRegistration {
    /// Active registration — the request will produce a record on
    /// finalize.
    Active(ActiveRegistration),
    /// No-op registration — the audit-config filter rejected the
    /// request kind. No record will be produced.
    Noop,
}

impl AuditRequestRegistration {
    /// Construct a new registration. Reads the audit-config filter
    /// ONCE; if the filter rejects the request's kind, returns the
    /// `Noop` variant without entering the active-request registry.
    /// Otherwise inserts into the registry and returns `Active(...)`.
    /// On native targets with `audit_timing_capture` enabled, the
    /// host-owned peak-RSS sampler thread is spawned lazily on the
    /// first such `Active` registration via
    /// [`HostAuditRuntime::ensure_sampler_started`].
    pub fn new(host: &crate::VerterHost, ctx: Arc<RequestContext>) -> Self {
        let runtime = host.host_audit_runtime();
        let cfg = runtime.audit_config();
        if !cfg.consumer_filter.allows(&ctx.kind()) {
            return Self::Noop;
        }
        runtime.register_active_request(ctx.request_id, &ctx);
        // Fetch a strong handle to the runtime — both for the
        // sampler-spawn call below (which needs `Arc<Self>`) and
        // for the Active registration's owned runtime field.
        let runtime_arc = host.host_audit_runtime_arc();
        // Lazy sampler spawn: short-circuits when
        // audit_timing_capture is off or the latch already won.
        // WASM targets are gated off — `ensure_sampler_started`
        // does not exist there.
        #[cfg(not(target_arch = "wasm32"))]
        runtime_arc.ensure_sampler_started();
        Self::Active(ActiveRegistration {
            request_id: ctx.request_id,
            runtime: runtime_arc,
            finalized: AtomicBool::new(false),
        })
    }

    /// Idempotent finalisation. Returns `true` on the first call
    /// against an `Active` registration (the record is stored and
    /// the active-request entry is removed); `false` on subsequent
    /// calls or on `Noop`.
    pub fn finalize(&self, record: RequestAuditRecord) -> bool {
        match self {
            Self::Noop => false,
            Self::Active(active) => active.finalize(record),
        }
    }

    /// Test-only: borrow the underlying request id when the
    /// registration is `Active`. Used by the discriminating tests
    /// to probe lifecycle membership in the active-request
    /// registry.
    #[must_use]
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Noop => None,
            Self::Active(active) => Some(active.request_id),
        }
    }
}

/// `Active` arm of the registration enum. Owns the request id, an
/// `Arc<HostAuditRuntime>` for the finalize / drop path, and a
/// `finalized` flag so finalize is idempotent.
#[derive(Debug)]
pub struct ActiveRegistration {
    request_id: u64,
    runtime: Arc<HostAuditRuntime>,
    finalized: AtomicBool,
}

impl ActiveRegistration {
    /// Idempotent finalize. Returns `true` only on the first call.
    pub fn finalize(&self, record: RequestAuditRecord) -> bool {
        if self.finalized.swap(true, Ordering::Relaxed) {
            return false;
        }
        self.runtime
            .finalize_active_request(self.request_id, record);
        true
    }
}

impl Drop for ActiveRegistration {
    fn drop(&mut self) {
        // Defensive cleanup on panic / cancellation paths. If
        // `finalize` already ran the flag is set and we leave the
        // (already-removed) registry alone; otherwise we strip the
        // entry without publishing a record.
        if !self.finalized.load(Ordering::Relaxed) {
            self.runtime.drop_active_request(self.request_id);
        }
    }
}

impl crate::VerterHost {
    /// Borrow the host's audit runtime. Consumers (tests, NAPI,
    /// WASM, LSP) call this to reach the records store, the
    /// audit-config snapshot, and the public snapshot accessor.
    #[must_use]
    pub fn host_audit_runtime(&self) -> &HostAuditRuntime {
        self.host_audit_runtime.as_ref()
    }

    /// Reference-counted handle to the audit runtime — needed by
    /// `AuditRequestRegistration::new` so the registration owns a
    /// runtime handle for its `finalize` / `drop` paths.
    #[must_use]
    pub fn host_audit_runtime_arc(&self) -> Arc<HostAuditRuntime> {
        Arc::clone(&self.host_audit_runtime)
    }

    /// Test-only: swap the runtime's `AuditConfig` snapshot with
    /// `config`. Used by integration tests that need to drive a
    /// non-default consumer filter (e.g. deny-all) without
    /// reaching across the privacy boundary on
    /// `HostAuditRuntime::active_requests`.
    ///
    /// Allocates a fresh `Arc<HostAuditRuntime>` carrying the new
    /// config and swaps the host's slot. The previous runtime's
    /// records-store `Arc` is reused so existing records survive
    /// the swap, but the new runtime starts with an empty
    /// active-request map — callers MUST call this before driving
    /// any audited request.
    pub fn replace_host_audit_runtime_for_test(&mut self, config: AuditConfig) {
        let store = Arc::clone(self.host_audit_runtime.audit_records_store());
        self.host_audit_runtime = Arc::new(HostAuditRuntime::new(config, store));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod seed_tests {
    //! Deterministic coverage for the registration-time peak-RSS seed
    //! (`register_active_request`). These tests drive the production
    //! seed primitive DIRECTLY — they do NOT go through
    //! `AuditRequestRegistration::new`, so `ensure_sampler_started` is
    //! never called and NO sampler thread spawns. The per-request peak
    //! slot therefore has exactly ONE possible writer (the seed), which
    //! makes the discrimination race-free: there is no sampler tick to
    //! win or lose against.

    use super::*;

    fn fresh_ctx(request_id: u64) -> Arc<RequestContext> {
        RequestContext::new(
            request_id,
            std::sync::Arc::<str>::from("/seed_probe.vue"),
            // No footprint capture / accumulator needed — the seed
            // touches only `process_rss_peak_bytes`.
            false,
            None,
        )
    }

    #[test]
    fn register_active_request_seeds_peak_rss_when_timing_capture_on() {
        // audit_timing_capture ON → the seed fires synchronously inside
        // register_active_request. No sampler thread exists (we bypass
        // ensure_sampler_started), so a non-zero slot can come ONLY from
        // the seed.
        //
        // Discrimination contract:
        // - Pre-fix tree (register_active_request does not seed): the
        //   slot stays at exactly 0 → the `> 0` assertion FAILS.
        // - Post-fix tree: the seed writes the start-of-request RSS →
        //   `> 0` PASSES.
        let runtime = HostAuditRuntime::new(
            AuditConfig {
                audit_timing_capture: true,
                ..AuditConfig::default()
            },
            Arc::new(AuditRecordsStore::default()),
        );
        let ctx = fresh_ctx(1);
        assert_eq!(
            ctx.process_rss_peak_bytes.load(Ordering::Relaxed),
            0,
            "fresh RequestContext must start with a zero peak slot",
        );

        runtime.register_active_request(ctx.request_id, &ctx);

        let seeded = ctx.process_rss_peak_bytes.load(Ordering::Relaxed);
        assert!(
            seeded > 0,
            "register_active_request must seed the per-request peak slot with an \
             immediate RSS sample when audit_timing_capture is on; got {seeded} \
             (pre-fix this is 0 because nothing writes the slot at registration)",
        );
        // The seed reads real process RSS, so it must land in a
        // realistic range for a debug test process — not a sentinel.
        const ONE_MB: u64 = 1024 * 1024;
        const SIXTEEN_GB: u64 = 16u64 * 1024 * 1024 * 1024;
        assert!(
            seeded > ONE_MB && seeded < SIXTEEN_GB,
            "seeded peak {seeded} must fall in the realistic RSS range (1 MB .. 16 GB)",
        );
    }

    #[test]
    fn register_active_request_does_not_seed_when_timing_capture_off() {
        // audit_timing_capture OFF → the seed is gated off (same flag
        // that governs the sampler). The slot MUST stay 0, preserving
        // the documented "peak stays 0 when audit_timing_capture is off"
        // contract (see `RequestContext::process_rss_peak_bytes` and the
        // `memory_peak_rss_zero_when_flag_off` integration test). This
        // pins the gate so a future change that seeds unconditionally
        // (which would regress that contract) fails here.
        let runtime = HostAuditRuntime::new(
            AuditConfig {
                audit_timing_capture: false,
                ..AuditConfig::default()
            },
            Arc::new(AuditRecordsStore::default()),
        );
        let ctx = fresh_ctx(2);

        runtime.register_active_request(ctx.request_id, &ctx);

        assert_eq!(
            ctx.process_rss_peak_bytes.load(Ordering::Relaxed),
            0,
            "with audit_timing_capture off the seed must NOT fire — the peak slot \
             stays 0 (zero-cost path; matches the sampler's own gate)",
        );
    }
}
