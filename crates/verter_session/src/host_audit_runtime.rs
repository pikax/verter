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
//! `AuditConfig::audit_timing_capture` is enabled, holds
//! `Arc<SamplerState>` (never `Arc<HostAuditRuntime>`), ticks
//! every 50 ms, and writes `fetch_max(current_process_rss())`
//! into each in-flight request's per-request peak slot. The
//! registry holds a `Weak` to that COUNTER, not to the request
//! context: an upgraded context would reach the runtime through
//! its audit registration, so the sampler dropping the last one
//! would run the registration's `Drop` (write-locking the very
//! registry the sampler is read-locking) and then the runtime's
//! `Drop` (joining the sampler from the sampler). Owner
//! drop sets an exact stop flag and unparks the sampler, then
//! joins on the owner thread. WASM targets are gated off via
//! `#[cfg(not(target_arch = "wasm32"))]` — `process_rss_peak_bytes`
//! stays at `0` there regardless of flag state.

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

/// Sampler tick interval. 50 ms bounds peak under-reporting while keeping
/// sampler CPU overhead low (~0.1% of one core at this rate).
#[cfg(not(target_arch = "wasm32"))]
const SAMPLER_TICK: Duration = Duration::from_millis(50);

/// Per-runtime sampler state. The sampler thread holds
/// [`Arc<SamplerState>`] and never [`Arc<HostAuditRuntime>`], so
/// owner drop cannot run on the sampler thread.
struct SamplerState {
    /// PRIVATE active-request registry — the three crate-private
    /// methods on [`HostAuditRuntime`] mediate every access.
    ///
    /// The value is a `Weak` to the request's peak-RSS COUNTER, never
    /// to the `RequestContext`. An `AtomicU64` has no destructor, so an
    /// upgrade the sampler drops can neither re-enter this lock nor
    /// release the last `Arc<HostAuditRuntime>`.
    active_requests: RwLock<FxHashMap<u64, Weak<std::sync::atomic::AtomicU64>>>,
    #[cfg(not(target_arch = "wasm32"))]
    stop: AtomicBool,
    #[cfg(not(target_arch = "wasm32"))]
    parked_thread: parking_lot::Mutex<Option<std::thread::Thread>>,
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    sample_handshake: parking_lot::Mutex<Option<SampleHandshake>>,
}

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
struct SampleHandshake {
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<owner_release_proof::OwnerReleaseEntered>,
}

/// TEST-ONLY currency that releases a paused sampler.
///
/// The witness holds a private field, so in safe Rust it cannot be
/// constructed outside this module; the re-export makes it nameable in a
/// channel signature without making it forgeable. That is the whole of the
/// compiler-checked guarantee — it is on the currency, not on the release.
///
/// A witness arriving on a release channel does NOT establish that the owner
/// thread is inside a release. The probe and its `fire` are visible to the
/// enclosing module, so any code within `host_audit_runtime` — its test
/// submodules included — can mint one at any point, and firing only from a
/// release is a convention of this module rather than something the type
/// carries. The witness is zero-sized, so `transmute` or `zeroed` forges
/// one; that route is out of charter here rather than something this type
/// prevents.
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
mod owner_release_proof {
    use std::sync::mpsc::SyncSender;

    /// Witness that the owner thread entered a release.
    pub struct OwnerReleaseEntered(());

    /// The armed observer of one release point, if a test armed one. Empty
    /// in every unobserved runtime.
    #[derive(Default)]
    pub(super) struct OwnerReleaseProbe(
        parking_lot::Mutex<Option<SyncSender<OwnerReleaseEntered>>>,
    );

    impl OwnerReleaseProbe {
        /// Arm the observer. The next release hands it the witness.
        pub(super) fn arm(&self, observer: SyncSender<OwnerReleaseEntered>) {
            *self.0.lock() = Some(observer);
        }

        /// Hand the armed observer the witness, once. On a rendezvous
        /// channel this blocks until the observer takes it, which is what
        /// pins the release and the observer to the same instant.
        pub(super) fn fire(&self) {
            let observer = self.0.lock().take();
            if let Some(observer) = observer {
                let _ = observer.send(OwnerReleaseEntered(()));
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub use owner_release_proof::OwnerReleaseEntered;

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
    /// Sample-loop state, shared with the sampler thread. The
    /// sampler may hold `Arc<SamplerState>` and must never hold
    /// `Arc<HostAuditRuntime>` — otherwise owner drop during an
    /// in-flight sample can run `HostAuditRuntime::drop` on the
    /// sampler thread, which would `join` itself.
    sampler: Arc<SamplerState>,
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
    /// Test-support join observable, LAZILY allocated. Empty until a
    /// test first calls [`Self::sampler_join_observer`], which seeds it
    /// with a shared `Arc<AtomicBool>` and hands the caller a clone.
    /// THIS runtime's `Drop` impl flips it to `true` after — and only
    /// after — a CLEAN `JoinHandle::join()` on the owned sampler, but
    /// ONLY when the slot was seeded; a host whose observer was never
    /// requested allocates nothing and the `Drop` flip is skipped.
    /// Scoped to a single host: it discriminates a non-joining `Drop`
    /// (observable stays `false`) and a panicked sampler join (also
    /// `false`) from a clean join (observable flips `true`) for THIS
    /// host alone — immune to concurrent samplers spawned/joined by
    /// sibling tests. Gated to `test` / the opt-in `test-support` feature
    /// (the same support gate the test probe uses), so no production build
    /// — debug or release — carries the slot.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    sampler_join_observed: std::sync::OnceLock<Arc<AtomicBool>>,
    /// TEST-ONLY: fired at the start of `Drop`, before unpark/join, so a
    /// handshake test can prove owner drop entered while the sampler is
    /// still inside the sample.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    drop_entered: owner_release_proof::OwnerReleaseProbe,
    /// TEST-ONLY: fired at the start of `drop_active_request`, BEFORE it
    /// takes the registry write lock. That is the first instant the owner
    /// thread is provably inside the release, and it is reachable while a
    /// paused sampler still read-holds the registry — unlike `Drop`, which
    /// the release itself has to get past first.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    owner_release_entered: owner_release_proof::OwnerReleaseProbe,
}

impl std::fmt::Debug for HostAuditRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostAuditRuntime")
            .field("config", &self.config)
            .field("records", &"<AuditRecordsStore>")
            .field(
                "active_requests_count",
                &self.sampler.active_requests.read().len(),
            )
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
            sampler: Arc::new(SamplerState {
                active_requests: RwLock::new(FxHashMap::default()),
                #[cfg(not(target_arch = "wasm32"))]
                stop: AtomicBool::new(false),
                #[cfg(not(target_arch = "wasm32"))]
                parked_thread: parking_lot::Mutex::new(None),
                #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
                sample_handshake: parking_lot::Mutex::new(None),
            }),
            #[cfg(not(target_arch = "wasm32"))]
            sampler_started: AtomicBool::new(false),
            #[cfg(not(target_arch = "wasm32"))]
            sampler_thread: parking_lot::Mutex::new(None),
            #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
            sampler_join_observed: std::sync::OnceLock::new(),
            #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
            drop_entered: owner_release_proof::OwnerReleaseProbe::default(),
            #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
            owner_release_entered: owner_release_proof::OwnerReleaseProbe::default(),
        }
    }

    /// Test-only — `true` once THIS runtime has spawned its peak-RSS
    /// sampler thread (the `sampler_started` latch has fired). Used by
    /// the host-drop test to assert the sampler spawned for an
    /// audit-enabled host, reading only this runtime's per-host latch.
    /// Gated to `test` / the opt-in `test-support` feature, so it is not
    /// part of the public surface of any production build.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    #[must_use]
    pub fn sampler_spawned(&self) -> bool {
        self.sampler_started.load(Ordering::Acquire)
    }

    /// Test-only — a clone of THIS runtime's join observable, LAZILY
    /// allocated on first call. Clone it BEFORE dropping the host; after
    /// the host (and runtime) drop, the observable reads `true` iff this
    /// runtime's `Drop` joined its sampler thread cleanly. A non-joining
    /// or panicked `Drop` leaves it `false`. Seeding the observable here
    /// is what arms the `Drop`-side flip — a host whose observer is never
    /// requested allocates nothing and `Drop` skips the flip. Immune to
    /// concurrent samplers in sibling tests because it observes only this
    /// host's join. Gated to `test` / the opt-in `test-support` feature,
    /// so it is not part of the public surface of any production build.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    #[must_use]
    pub fn sampler_join_observer(&self) -> Arc<AtomicBool> {
        Arc::clone(
            self.sampler_join_observed
                .get_or_init(|| Arc::new(AtomicBool::new(false))),
        )
    }

    /// Test-only — the sampler's real tick interval. Exposed so a
    /// shutdown-promptness assertion can be tied to the ACTUAL production
    /// constant that bounds it (the sampler checks `weak.upgrade()` once
    /// per tick, so `Drop`'s join can never take meaningfully longer than
    /// one tick), rather than an arbitrary, disconnected wall-clock guess.
    /// Gated to `test` / the opt-in `test-support` feature, so it is not
    /// part of the public surface of any production build.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    #[must_use]
    pub const fn sampler_tick_for_test() -> Duration {
        SAMPLER_TICK
    }

    /// TEST-ONLY — arm a handshake that fires once the sampler is
    /// inside a sample and then blocks until the test releases it.
    /// Used to prove owner drop during an in-flight sample joins on
    /// the owner thread rather than self-joining the sampler.
    ///
    /// Releasing the sampler costs one [`OwnerReleaseEntered`], which safe
    /// Rust cannot forge outside that type's own module — see it for what a
    /// witness does and does not establish.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    pub fn arm_sample_handshake(
        &self,
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<OwnerReleaseEntered>,
    ) {
        *self.sampler.sample_handshake.lock() = Some(SampleHandshake { entered, release });
    }

    /// TEST-ONLY — observe the start of the active-request release. The
    /// production release fires this probe from inside `drop_active_request`,
    /// before it takes the registry write lock; on a rendezvous channel that
    /// send blocks until the observer takes the witness, which is what pins
    /// firer and observer to the same instant. Wiring a paused sampler's
    /// release end here couples the sampler's resume to that instant. See
    /// [`OwnerReleaseEntered`] for what a witness does and does not establish.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    pub fn arm_owner_release_entered(
        &self,
        observer: std::sync::mpsc::SyncSender<OwnerReleaseEntered>,
    ) {
        self.owner_release_entered.arm(observer);
    }

    /// TEST-ONLY — fire `entered` at the start of `Drop`, before unpark/join.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    pub fn arm_drop_entered(&self, observer: std::sync::mpsc::SyncSender<OwnerReleaseEntered>) {
        self.drop_entered.arm(observer);
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
        let map = self.sampler.active_requests.read();
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
    /// to insert a `Weak` to the request's peak-RSS slot into the
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
        let mut map = self.sampler.active_requests.write();
        map.insert(request_id, Arc::downgrade(&ctx.process_rss_peak_bytes));
    }

    /// Crate-private. Called ONLY by
    /// `AuditRequestRegistration::finalize` to atomically remove the
    /// entry from the active-request registry AND publish the
    /// finalised record into the records store.
    pub(crate) fn finalize_active_request(&self, request_id: u64, record: RequestAuditRecord) {
        let mut map = self.sampler.active_requests.write();
        map.remove(&request_id);
        drop(map); // release before insertion to avoid lock-order coupling
        self.records.insert(record);
    }

    /// Crate-private. Called ONLY by `AuditRequestRegistration::drop`
    /// (defensive cleanup on panic / cancellation paths). Removes the
    /// entry from the active-request registry; does NOT publish a
    /// record — the absence of a record is itself observable.
    pub(crate) fn drop_active_request(&self, request_id: u64) {
        // Announce BEFORE the write: a sampler paused mid-sample holds the
        // read guard, so the lock below is exactly where this thread waits
        // for it. Announcing after would only be reachable once the sampler
        // had already been released.
        #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
        self.owner_release_entered.fire();
        let mut map = self.sampler.active_requests.write();
        map.remove(&request_id);
    }

    /// Spawn the host-owned peak-RSS sampler thread (native only).
    ///
    /// Called by `AuditRequestRegistration::new` whenever an
    /// `Active` registration is constructed AND the audit-config
    /// has `audit_timing_capture = true`. The first call wins the
    /// `compare_exchange` on `sampler_started`, spawns the
    /// thread, and stores the `JoinHandle`. Subsequent calls
    /// short-circuit. The thread holds `Arc<SamplerState>` and
    /// never `Arc<HostAuditRuntime>`. Owner drop sets `stop` and
    /// unparks the sampler, then joins on the owner thread.
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
        let state = Arc::clone(&self.sampler);
        let handle = std::thread::Builder::new()
            .name("verter-audit-rss-sampler".to_string())
            .spawn(move || sampler_loop(state))
            .expect("spawning the verter-audit-rss-sampler thread must succeed");
        // The `sampler_started` latch above is THIS host's spawn signal,
        // read back per-host via `sampler_spawned()`.
        let mut slot = self.sampler_thread.lock();
        verter_debug_assert!(
            slot.is_none(),
            "sampler_started latch must guarantee a single spawn",
        );
        *slot = Some(handle);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for HostAuditRuntime {
    fn drop(&mut self) {
        // Exact stop + unpark, then join on THIS thread. The
        // sampler never holds `Arc<HostAuditRuntime>`, so this
        // Drop cannot run on the sampler thread.
        #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
        self.drop_entered.fire();
        self.sampler.stop.store(true, Ordering::Release);
        if let Some(thread) = self.sampler.parked_thread.lock().take() {
            thread.unpark();
        }
        let handle = self.sampler_thread.lock().take();
        if let Some(handle) = handle {
            // join() returns Err only if the thread panicked. A
            // panicked sampler does not threaten the host, and there is
            // no way to surface the error from Drop, so we do not
            // propagate it. The join itself is the production behaviour;
            // its outcome additionally gates the test-support observable
            // below.
            let _joined_cleanly = handle.join().is_ok();
            // Flip the host-owned observable AFTER — and only after — a
            // CLEAN join, so a test holding a clone (taken before the
            // host dropped) can confirm THIS host joined its sampler
            // without a thread panic. `Release` pairs with the test's
            // `Acquire` load. A non-joining Drop never reaches this
            // store, and a panicked sampler (`Err`) leaves it `false`
            // too — the observable asserts a clean shutdown, not merely
            // that join returned (discrimination preserved on both
            // legs). The flip only happens when a test SEEDED the
            // observable via `sampler_join_observer()` (the slot is
            // `Some`); an un-observed host allocated nothing and is
            // skipped. Whole block is gated to `test` / the opt-in
            // `test-support` feature so a production host carries neither
            // the slot nor this flip.
            #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
            if _joined_cleanly {
                if let Some(observed) = self.sampler_join_observed.get() {
                    observed.store(true, Ordering::Release);
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn sampler_loop(state: Arc<SamplerState>) {
    // The sampler loop ticks every 50 ms while `stop` is clear.
    // Owner drop stores `stop` and unparks this thread, so
    // shutdown does not wait for the next periodic tick. The
    // sample-once-per-tick discipline means N in-flight requests
    // share the same RSS value on the same tick.
    *state.parked_thread.lock() = Some(std::thread::current());
    loop {
        if state.stop.load(Ordering::Acquire) {
            return;
        }
        let now = verter_audit::current_process_rss();
        {
            let map = state.active_requests.read();
            // Keep one upgrade alive across the handshake below. Pausing
            // ABOVE this block would let the shutdown test pass without
            // ever entering the only window in which an owner drop racing
            // the sampler could run a destructor on this thread: registry
            // read guard held, registered value upgraded.
            #[cfg(any(test, feature = "test-support"))]
            let mut held: Option<Arc<std::sync::atomic::AtomicU64>> = None;
            for weak in map.values() {
                if let Some(slot) = weak.upgrade() {
                    slot.fetch_max(now, Ordering::Relaxed);
                    #[cfg(any(test, feature = "test-support"))]
                    if held.is_none() {
                        held = Some(slot);
                    }
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            if let Some(handshake) = state.sample_handshake.lock().take() {
                let _ = handshake.entered.send(());
                // A disconnected release is NOT a release. Discarding the
                // error would resume the sample whenever the sender was
                // merely dropped, which hands back the witness-free release
                // the witness type exists to forbid. Panicking here fails the
                // sampler's own join, so the test that let this happen cannot
                // read the resulting shutdown as clean.
                handshake
                    .release
                    .recv()
                    .expect("a paused sample resumes only on an owner-release witness");
            }
            #[cfg(any(test, feature = "test-support"))]
            drop(held);
        }
        if state.stop.load(Ordering::Acquire) {
            return;
        }
        std::thread::park_timeout(SAMPLER_TICK);
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

/// The sampler must survive an owner drop that races a sample which has
/// the registry read guard held AND a registered value upgraded. That is
/// the only window in which a destructor could ever run on the sampler
/// thread, and both hazards it used to carry are reachable from it.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod sampler_ownership_tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    /// Registering a `Weak<RequestContext>` instead of a `Weak` to the
    /// bare peak-RSS counter makes the sampler a transitive owner of the
    /// runtime: `RequestContext` holds `Arc<AuditRequestRegistration>`,
    /// whose `Active` arm holds `Arc<HostAuditRuntime>`. This test drives
    /// exactly that chain.
    ///
    /// Sequence: the sampler pauses mid-sample holding the read guard and
    /// its upgrade; a side thread then drops the request context and the
    /// host; the active-request release hands the paused sampler its witness
    /// directly. This thread never holds the release end, so it cannot
    /// resume the sampler early even by accident — the release and the paused
    /// sample overlap by construction rather than by being spawned in that
    /// order.
    ///
    /// Against a registry of `Weak<RequestContext>` the sampler's release
    /// destroys the last context, which runs `ActiveRegistration::drop` →
    /// `drop_active_request` → `active_requests.write()` while this same
    /// thread holds the read guard (`parking_lot::RwLock` is not
    /// reentrant), and then `HostAuditRuntime::drop` → `join()` on the
    /// sampler thread itself. Either way the runtime never finishes
    /// dropping and the join observable stays `false` — verified by
    /// planting that registry shape and watching this test go red.
    #[test]
    fn owner_drop_during_a_sample_holding_a_registered_value_joins_cleanly() {
        let host = Arc::new(crate::VerterHost::new_standalone(crate::HostConfig {
            audit_enabled: true,
            audit_timing_capture: true,
            footprint_capture: true,
            ..crate::HostConfig::default()
        }));

        let (entered_tx, entered_rx) = mpsc::sync_channel(0);
        // The release channel is wired from the active-request release
        // STRAIGHT to the paused sampler; this thread keeps no end of it and
        // so has no way to release the sampler itself. The release mints the
        // witness and hands it over as a rendezvous, which pins the owner
        // thread inside the release to the instant the sampler resumes — the
        // overlap is produced by the wiring rather than waited for.
        let (release_tx, release_rx) = mpsc::sync_channel(0);
        host.host_audit_runtime()
            .arm_sample_handshake(entered_tx, release_rx);
        host.host_audit_runtime()
            .arm_owner_release_entered(release_tx);

        // A live registration: it enters the registry, spawns the
        // sampler, and — installed on the context — makes the context a
        // transitive owner of the runtime.
        let ctx = RequestContext::new(1, Arc::<str>::from("/sampler_owner.vue"), false, None);
        let registration = AuditRequestRegistration::new(&host, Arc::clone(&ctx));
        assert!(
            matches!(registration, AuditRequestRegistration::Active(_)),
            "the default consumer filter must admit ComponentMeta"
        );
        ctx.install_audit_registration(Arc::new(registration))
            .expect("the context has no registration yet");
        assert!(
            host.host_audit_runtime().sampler_spawned(),
            "an active registration under audit_timing_capture must spawn the sampler"
        );

        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the sampler must pause inside a sample holding a registered value");

        let join_observer = host.host_audit_runtime().sampler_join_observer();
        let (dropped_tx, dropped_rx) = mpsc::sync_channel(0);
        let dropper = thread::spawn(move || {
            drop(ctx);
            drop(host);
            let _ = dropped_tx.send(());
        });

        // Nothing to do but wait for the drop: the sampler is released by the
        // release itself. Under the prohibited registry shape the sampler
        // holds a context of its own, so the dropper's `drop(ctx)` is not the
        // last one, no release is ever entered, the sampler is never handed a
        // witness, and the runtime is never dropped — which the join
        // observable below reports as the failure it is, causally rather than
        // by waiting for a clock.
        dropped_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("owner drop racing an active sample must complete (outer watchdog)");
        dropper.join().expect("the dropper thread must not panic");

        assert!(
            join_observer.load(Ordering::Acquire),
            "the runtime must have joined its sampler on the OWNER thread; a \
             registry that hands the sampler a transitive owner leaves this \
             false — the destructor chain either self-joins or blocks on the \
             registry write lock the sampler is read-holding"
        );
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
        // Discrimination contract: with no sampler thread, the only
        // writer of the per-request peak slot is the registration seed.
        // If `register_active_request` failed to seed, the slot would
        // stay at exactly 0 and the `> 0` assertion would fail; the seed
        // writes the start-of-request RSS, so `> 0` passes.
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
             (a 0 here means nothing wrote the slot at registration)",
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
