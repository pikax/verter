//! Hot-path concurrency-fitness discriminator for the family memo
//! warm-read path.
//!
//! Asserts that [`SemanticGraphStore::try_warm_hit_fast_path`] (and the
//! sister `get_validated`) snapshot the candidate list under the
//! `entries` mutex and validate OUTSIDE the mutex — so a peer thread
//! can acquire `entries.try_lock()` while a worker thread is inside
//! `MemoEntry::validate`.
//!
//! **Discriminator.**
//! - Pre-fix tree: the warm-read path held the single global `entries`
//!   mutex across `validate`. A peer thread's `try_lock` during
//!   validate returned `None`; under the multi-candidate cap-4
//!   substrate the validate walk is non-trivial and serialised every
//!   unrelated warm read / cold publish.
//! - Post-fix tree: snapshot under lock, validate outside, brief
//!   reacquire for LRU bookkeeping only. A peer thread's `try_lock`
//!   during validate succeeds.
//!
//! The probe is a process-global `validate_running_probe` armed by
//! [`SemanticGraphStore::arm_validate_running_probe_for_tests`]; the
//! probe is invoked from inside [`family::MemoEntry::validate`] on
//! every validate call.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use verter_session::for_tests::{
    ReadSetSignature, SemanticGraphStore, VALIDATE_RUNNING_PROBE_TEST_LOCK,
};
use verter_session::semantic_query::{
    ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey,
};
use verter_session::{HostConfig, VerterHost};

/// The warm-read fast path snapshots candidates under `entries`,
/// releases the lock, and ONLY THEN calls `MemoEntry::validate` on each
/// snapshotted candidate. A peer thread that races a warm read can
/// acquire `entries.try_lock()` WHILE the worker is inside `validate`.
#[test]
fn warm_read_validates_outside_entries_mutex() {
    let _serialise = VALIDATE_RUNNING_PROBE_TEST_LOCK.lock();

    let host = VerterHost::new_standalone(HostConfig::default());
    let graph = host.project_type_store().semantic_graph();

    // Publish ONE candidate so the warm path scans + validates it.
    let canonical = "/warm_read_lock_test/owner.ts";
    let key = SemanticQueryKey::Instantiate {
        base: verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical),
            Arc::from("Foo"),
        ),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: verter_session::semantic_query::InstantiateContext::non_file_for_tests(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
        ),
    };
    let value = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Boolean,
    ));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value),
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        0,
    );

    // Arm the validate probe with a barrier — the worker thread will
    // park inside validate so the test thread can attempt to acquire
    // `entries.try_lock()` during that window. Two participants:
    // worker + test.
    //
    // The probe (`VALIDATE_RUNNING_PROBE`) is process-global and fires
    // from EVERY `MemoEntry::validate` in the process, not just this
    // test's worker. `VALIDATE_RUNNING_PROBE_TEST_LOCK` serialises only
    // the probe ARMERS — it does NOT stop an unrelated test that runs a
    // warm read concurrently (in the consolidated single-binary suite)
    // from invoking the armed barrier closure on its own thread and
    // deadlocking on a `Barrier::new(2)` it was never meant to join. So
    // the probe is scoped to THIS test's worker thread: it barrier-waits
    // only when invoked on the worker, and is an inert no-op on every
    // other thread (e.g. another component-meta test's resolution).
    let barrier = Arc::new(Barrier::new(2));
    let barrier_for_probe = Arc::clone(&barrier);
    let probe_seen = Arc::new(AtomicBool::new(false));
    let probe_seen_clone = Arc::clone(&probe_seen);
    let worker_thread_id: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
    let worker_thread_id_for_probe = Arc::clone(&worker_thread_id);
    let _guard = SemanticGraphStore::arm_validate_running_probe_for_tests(move || {
        // Fire ONLY on this test's worker thread. The worker stores its
        // id before issuing the warm read, so any validate it triggers
        // sees a matching id; validates from any other thread (a
        // co-resident test's resolution) skip the barrier entirely.
        let is_worker = worker_thread_id_for_probe
            .lock()
            .unwrap()
            .is_some_and(|id| id == thread::current().id());
        if !is_worker {
            return;
        }
        probe_seen_clone.store(true, Ordering::SeqCst);
        // Two-phase wait so the test thread can interleave its
        // try_lock attempt between the two waits.
        barrier_for_probe.wait();
        barrier_for_probe.wait();
    });

    // Spawn the warm-read worker. It calls `get_validated` (the
    // semantic_query_memo public warm-read entry) which goes through
    // the same snapshot+outside-lock-validate path as
    // `try_warm_hit_fast_path`.
    let graph_for_worker: Arc<SemanticGraphStore> = Arc::clone(graph);
    let graph_for_probe: Arc<SemanticGraphStore> = Arc::clone(graph);
    let key_for_worker = key.clone();
    let host_arc = Arc::new(host);
    let host_for_worker = Arc::clone(&host_arc);
    let worker_done = Arc::new(AtomicBool::new(false));
    let worker_done_clone = Arc::clone(&worker_done);
    let worker_thread_id_for_worker = Arc::clone(&worker_thread_id);
    let worker = thread::spawn(move || {
        // Publish this thread's id BEFORE the warm read so the armed
        // probe recognises (and only fires for) this worker's validate
        // calls. Set first — before any `validate` can run.
        *worker_thread_id_for_worker.lock().unwrap() = Some(thread::current().id());
        // Drive the warm-read path via the concrete VerterHost —
        // mirrors the production `try_warm_hit_fast_path` callers.
        let _ = graph_for_worker
            .get_validated_with_host_for_tests(&key_for_worker, host_for_worker.as_ref());
        worker_done_clone.store(true, Ordering::SeqCst);
    });

    // Wait for the worker to enter validate (first barrier sync).
    barrier.wait();
    assert!(
        probe_seen.load(Ordering::SeqCst),
        "fixture invariant: validate probe must have fired by now"
    );

    // The discriminating assertion. While the worker is parked
    // inside `MemoEntry::validate`, attempt `entries.try_lock()` on
    // the store. Under the FIX (snapshot + outside-lock validate),
    // the mutex is NOT held and try_lock succeeds within a generous
    // window. Under the BUG (validate-under-mutex), the mutex IS
    // held and try_lock returns None for the entire window.
    let try_lock_window = Duration::from_millis(150);
    let start = Instant::now();
    let mut try_lock_succeeded = false;
    while start.elapsed() < try_lock_window {
        if graph_for_probe.try_lock_entries_for_tests() {
            try_lock_succeeded = true;
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    // Release the worker.
    barrier.wait();
    worker.join().expect("warm-read worker should not panic");
    assert!(
        worker_done.load(Ordering::SeqCst),
        "worker thread must have completed its warm read"
    );

    assert!(
        try_lock_succeeded,
        "POST-FIX: `try_warm_hit_fast_path` / `get_validated` must \
         release the `entries` mutex BEFORE calling `MemoEntry::validate`. \
         A peer thread should acquire `entries.try_lock()` while the \
         worker is parked inside validate. PRE-FIX (validate-under-mutex): \
         the try_lock would have FAILED for the entire window because \
         the warm-read worker held the lock across validate."
    );
}
