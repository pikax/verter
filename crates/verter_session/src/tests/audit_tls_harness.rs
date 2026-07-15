//! Reusable TLS observer-propagation test harness.
//!
//! `assert_observer_reaches` lets a test verify that the active
//! [`verter_audit::current_observer()`] reaches every thread the
//! supplied closure cares about. The closure runs while a
//! `RequestContextGuard` (or — for the `install_audit = false`
//! control case — no guard at all) is installed; the harness records
//! whether `verter_audit::current_observer().is_some()` was visible
//! on the calling thread and on any worker threads the closure
//! reported through [`report_worker_observer_presence`].
//!
//! Worker threads spawned bare via `std::thread::spawn` get a fresh
//! TLS slot by construction. A closure that wants to verify observer
//! propagation into a worker pool must either install the guard
//! again on the worker (rayon's `current_thread_index()` patterns)
//! or rely on a runtime that already plumbs `RequestContextGuard`
//! through to its workers (the production scheduler does this for
//! its rayon pool). The harness is the integration point for those
//! verifications; it does not itself spawn workers.
//!
//! ## Example
//!
//! ```ignore
//! use verter_session::tests::audit_tls_harness::assert_observer_reaches;
//!
//! #[test]
//! fn observer_visible_in_synchronous_call() {
//!     let report = assert_observer_reaches(true, || {
//!         // Inside the closure, current_observer() is Some(...).
//!         verter_audit::current_observer().is_some()
//!     });
//!     assert!(report.observer_seen_on_calling_thread);
//! }
//! ```

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use verter_audit::current_observer;

use crate::request_context::{RequestContext, RequestContextGuard};

/// One worker thread's observer-propagation observation, captured
/// via [`report_worker_observer_presence`]. Public so the
/// [`WorkerSinkHandle`] can be inspected by the harness internals
/// after the closure returns; callers do not construct values of
/// this type themselves.
#[derive(Debug, Clone)]
pub struct WorkerObservation {
    thread_id: ThreadId,
    thread_name: Option<String>,
    saw_observer: bool,
}

impl WorkerObservation {
    /// Concrete thread identity — distinguishes two SFCs failing the
    /// same gate.
    #[must_use]
    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }
    /// Best-effort thread name (workers spun up under
    /// `std::thread::Builder::name(...)` populate this).
    #[must_use]
    pub fn thread_name(&self) -> Option<&str> {
        self.thread_name.as_deref()
    }
    /// `true` when `current_observer().is_some()` was visible to the
    /// reporting worker.
    #[must_use]
    pub fn saw_observer(&self) -> bool {
        self.saw_observer
    }
}

/// Opaque handle around the harness's worker-observation sink. Cloned
/// across thread boundaries so the harness's caller can ferry the
/// sink onto worker threads that don't inherit TLS automatically;
/// install it on the worker via [`WorkerSinkHandle::install`] before
/// calling [`report_worker_observer_presence`].
#[derive(Debug, Clone)]
pub struct WorkerSinkHandle {
    sink: Arc<Mutex<Vec<WorkerObservation>>>,
}

impl WorkerSinkHandle {
    /// Install this handle as the calling thread's worker-observation
    /// sink, returning an RAII guard that restores the previous slot
    /// on drop. Workers that need to report through
    /// [`report_worker_observer_presence`] call this once before
    /// invoking the report.
    #[must_use]
    pub fn install(&self) -> WorkerSinkGuard {
        let prev = WORKER_SINK.with(|cell| cell.replace(Some(Arc::clone(&self.sink))));
        WorkerSinkGuard { prev }
    }
}

thread_local! {
    /// Per-thread sink that the active harness invocation registers
    /// before running its closure. Worker threads spawned by the
    /// closure that already inherit this TLS slot (e.g., via a
    /// runtime that re-installs the harness's collector on the
    /// worker) push their observation into the held `Mutex<Vec<...>>`.
    ///
    /// Bare `std::thread::spawn` workers get a fresh TLS — they will
    /// see `None` here and silently skip reporting; the harness's
    /// caller is responsible for re-installing the collector on
    /// worker threads it cares about (e.g. inside a rayon
    /// `install` block) or for confirming, via the calling-thread
    /// check, that no off-thread propagation was needed.
    static WORKER_SINK: std::cell::RefCell<Option<Arc<Mutex<Vec<WorkerObservation>>>>>
        = const { std::cell::RefCell::new(None) };
}

/// RAII guard for [`WorkerSinkHandle::install`]; restores the
/// previous TLS sink slot on drop.
pub struct WorkerSinkGuard {
    prev: Option<Arc<Mutex<Vec<WorkerObservation>>>>,
}

impl Drop for WorkerSinkGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        WORKER_SINK.with(|cell| {
            *cell.borrow_mut() = prev;
        });
    }
}

/// Return a clone of the calling thread's current worker-sink
/// handle, if one is installed. Used by the harness's closure to
/// ferry the sink onto worker threads — the closure clones the
/// handle, moves the clone into a `std::thread::spawn` body, and the
/// worker installs it before calling
/// [`report_worker_observer_presence`].
#[must_use]
pub fn current_worker_sink_handle() -> Option<WorkerSinkHandle> {
    WORKER_SINK.with(|cell| {
        cell.borrow().as_ref().map(|sink| WorkerSinkHandle {
            sink: Arc::clone(sink),
        })
    })
}

/// Report the calling worker thread's observer-propagation state to
/// the harness invocation that installed the sink. No-op when no
/// sink is installed (the worker was spawned bare and lost the TLS
/// inheritance).
pub fn report_worker_observer_presence() {
    let saw = current_observer().is_some();
    let tid = std::thread::current().id();
    let name = std::thread::current().name().map(str::to_owned);
    WORKER_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            if let Ok(mut vec) = sink.lock() {
                vec.push(WorkerObservation {
                    thread_id: tid,
                    thread_name: name,
                    saw_observer: saw,
                });
            }
        }
    });
}

/// One call site at which TLS propagation failed. Populated by
/// [`TlsReachReport`] for every thread that was expected to see the
/// observer but did not.
#[derive(Debug, Clone)]
pub struct OrphanCallSite {
    /// `module::function::name` of the probe site.
    pub function_path: &'static str,
    /// SFC path being processed; `Some` for production paths that
    /// thread a canonical id, `None` for synthetic harness self-tests.
    pub canonical_id: Option<String>,
    /// Best-effort thread name.
    pub thread_name: Option<String>,
    /// Concrete thread identity for distinguishing two SFCs failing
    /// the same gate.
    pub thread_id: ThreadId,
}

/// Result of one [`assert_observer_reaches`] invocation.
#[derive(Debug, Clone)]
pub struct TlsReachReport {
    /// `true` if the calling thread saw a populated TLS observer slot
    /// at the harness's check point. The closure's calling thread is
    /// always the one that ran the closure synchronously.
    pub observer_seen_on_calling_thread: bool,
    /// One entry per worker thread that opted into observation via
    /// [`report_worker_observer_presence`]. Bare `std::thread::spawn`
    /// workers do NOT appear here — they would have been observed as
    /// orphans (no TLS), and the closure would have to re-install
    /// the sink on the worker for the observation to register.
    pub observer_seen_on_worker_threads: Vec<(ThreadId, bool)>,
    /// Threads that reported through the sink but did NOT see an
    /// observer in TLS. Distinguishes between "worker never reported"
    /// (absent from `observer_seen_on_worker_threads`) and "worker
    /// reported and was missing the observer" (present here).
    pub orphaned_call_sites: Vec<OrphanCallSite>,
}

impl TlsReachReport {
    /// Panic if the calling thread or any worker thread that reported
    /// to the harness saw `current_observer()` as `None`. The panic
    /// message lists every offending thread + orphan call site so the
    /// failing test points at the exact propagation gap; including
    /// the `canonical_id` per orphan distinguishes two SFCs failing
    /// the same gate.
    pub fn assert_full_propagation(&self) -> ! {
        let mut msg = String::from(
            "TlsReachReport::assert_full_propagation failed — current_observer() did not reach every probe site:\n",
        );
        if !self.observer_seen_on_calling_thread {
            msg.push_str(
                "  - calling thread saw None (RequestContextGuard not propagating into the harness's invocation thread)\n",
            );
        }
        for (tid, saw) in &self.observer_seen_on_worker_threads {
            if !saw {
                msg.push_str(&format!("  - worker thread {tid:?} saw None\n"));
            }
        }
        for orphan in &self.orphaned_call_sites {
            msg.push_str(&format!(
                "  - orphan call site at {}: thread {:?} (name={:?}, canonical_id={:?})\n",
                orphan.function_path, orphan.thread_id, orphan.thread_name, orphan.canonical_id,
            ));
        }
        panic!("{msg}");
    }
}

pub fn assert_observer_reaches<F, T>(install_audit: bool, f: F) -> TlsReachReport
where
    F: FnOnce() -> T,
{
    // Synthetic request_id / canonical_id — harness self-tests don't
    // care which request they're attributed to. Production-path tests
    // that exercise real audited entry-points reach for real ones via
    // those entry-points; this harness is the verification primitive
    // for those tests, never the data source.
    let sink: Arc<Mutex<Vec<WorkerObservation>>> = Arc::new(Mutex::new(Vec::new()));
    let handle = WorkerSinkHandle {
        sink: Arc::clone(&sink),
    };
    let _sink_guard = handle.install();

    // Track observer presence on the calling thread; capture the
    // observation INSIDE the guard's lifetime so we measure the
    // populated state, not the post-drop state.
    let observer_seen_on_calling_thread = if install_audit {
        let ctx: Arc<RequestContext> = RequestContext::new(
            // Synthetic request id; harness probes only check is_some().
            u64::MAX,
            Arc::from("/__harness_synthetic__.vue"),
            false,
            None,
        );
        let _guard = RequestContextGuard::install(Arc::clone(&ctx));
        let _result = f();
        current_observer().is_some()
        // _guard drops here, restoring TLS.
    } else {
        // Control case: no guard installed.
        let _result = f();
        current_observer().is_some()
    };

    // Drain reported worker observations.
    let observations = sink.lock().map(|v| v.clone()).unwrap_or_default();
    let observer_seen_on_worker_threads: Vec<(ThreadId, bool)> = observations
        .iter()
        .map(|o| (o.thread_id, o.saw_observer))
        .collect();
    let orphaned_call_sites: Vec<OrphanCallSite> = observations
        .iter()
        .filter(|o| !o.saw_observer)
        .map(|o| OrphanCallSite {
            function_path:
                "verter_session::tests::audit_tls_harness::report_worker_observer_presence",
            canonical_id: None,
            thread_name: o.thread_name.clone(),
            thread_id: o.thread_id,
        })
        .collect();

    TlsReachReport {
        observer_seen_on_calling_thread,
        observer_seen_on_worker_threads,
        orphaned_call_sites,
    }
}
