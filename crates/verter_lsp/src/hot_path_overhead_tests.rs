//! Steady-state cost of the request-stability machinery on the HEALTHY path.
//!
//! The stability work that removed the session wedge added per-request
//! machinery: an import-set freshness memo, a per-document singleflight lock, an
//! ambient request-deadline scope, and a `tokio::time::timeout` wrapper. A
//! request that was already going to succeed must not pay measurably for any of
//! it — a lock that serializes an already-fast path is a regression regardless
//! of what it prevents.
//!
//! These measure the added steady-state cost directly and assert a per-request
//! ceiling far below the threshold at which a human could perceive it. The
//! ceilings are deliberately loose (microseconds against a sub-microsecond
//! measured cost) so the assertions fail on a structural regression — a lock
//! that starts contending, a memo that starts rebuilding — rather than on
//! ordinary scheduler noise.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::server::ImportSyncMemo;

/// Per-request ceiling for any single piece of the stability machinery.
///
/// The intent contract's bar is "well under 1ms per request". Each individual
/// component is held to 50us — twenty times tighter than the bar and still two
/// orders of magnitude above the measured cost, so the assertion discriminates a
/// structural regression without tracking machine speed.
const PER_REQUEST_CEILING: Duration = Duration::from_micros(50);

/// Iterations per measurement. Large enough that per-iteration cost resolves
/// above timer granularity, small enough to stay negligible in the suite.
const ITERATIONS: u32 = 20_000;

fn per_iteration(total: Duration, iterations: u32) -> Duration {
    total / iterations
}

/// Report a measurement so a perf run shows the actual cost, not just a verdict.
fn report(label: &str, each: Duration) {
    eprintln!("[hot-path] {label}: {each:?} per request");
}

/// The warm memo path — the one a request storm on an unchanged document takes —
/// must cost effectively nothing. This is the lookup that replaced a full
/// import-graph BFS re-walk per request, so it runs on every navigation request.
#[test]
fn the_warm_import_set_memo_lookup_is_free_on_the_healthy_path() {
    let memo = ImportSyncMemo::default();
    let canonical = "/workspace/src/App.vue";
    memo.record_delivered(canonical.to_string(), (7, 3));

    // Warm the map so the measurement excludes first-insert cost.
    for _ in 0..1_000 {
        std::hint::black_box(memo.is_fresh_at(canonical, (7, 3)));
    }

    let started = Instant::now();
    for _ in 0..ITERATIONS {
        std::hint::black_box(memo.is_fresh_at(canonical, (7, 3)));
    }
    let each = per_iteration(started.elapsed(), ITERATIONS);
    report("warm memo lookup", each);

    assert!(
        each < PER_REQUEST_CEILING,
        "the warm memo lookup costs {each:?} per request, over the {PER_REQUEST_CEILING:?} \
         ceiling — the memo is rebuilding rather than hitting"
    );
}

/// Acquiring and releasing an UNCONTENDED per-document singleflight lock is the
/// steady-state case: one user, one document, no storm. It must not cost more
/// than the memo lookup it guards.
#[tokio::test]
async fn the_uncontended_singleflight_lock_costs_nothing_per_request() {
    let memo = ImportSyncMemo::default();
    let canonical = "/workspace/src/App.vue";

    for _ in 0..1_000 {
        let lock = memo.lock_for(canonical);
        drop(lock.lock().await);
    }

    let started = Instant::now();
    for _ in 0..ITERATIONS {
        let lock = memo.lock_for(canonical);
        drop(lock.lock().await);
    }
    let each = per_iteration(started.elapsed(), ITERATIONS);
    report("uncontended singleflight", each);

    assert!(
        each < PER_REQUEST_CEILING,
        "acquiring the per-document singleflight costs {each:?} per request, over the \
         {PER_REQUEST_CEILING:?} ceiling"
    );
}

/// Concurrent requests on DIFFERENT documents must not contend. If the
/// per-document lock ever collapsed to a shared one, throughput would fall off a
/// cliff exactly when the editor is busiest — many documents, many requests.
///
/// Measures wall-clock for N tasks each holding its own document's lock across
/// an await. Serialized, that is N x the hold time; concurrent, it is ~1x.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_on_different_documents_do_not_contend() {
    const DOCUMENTS: usize = 8;
    const HOLD: Duration = Duration::from_millis(60);

    let memo = Arc::new(ImportSyncMemo::default());
    let started = Instant::now();

    let mut tasks = Vec::new();
    for index in 0..DOCUMENTS {
        let memo = Arc::clone(&memo);
        tasks.push(tokio::spawn(async move {
            let canonical = format!("/workspace/src/Doc{index}.vue");
            let lock = memo.lock_for(&canonical);
            let _guard = lock.lock().await;
            tokio::time::sleep(HOLD).await;
        }));
    }
    for task in tasks {
        task.await.expect("no task may panic");
    }
    let elapsed = started.elapsed();

    let serialized = HOLD * DOCUMENTS as u32;
    assert!(
        elapsed < serialized / 2,
        "{DOCUMENTS} documents each holding their own singleflight took {elapsed:?}; \
         serialized would be {serialized:?}. The per-document lock is contending \
         across documents"
    );
}

/// The ambient request-deadline scope wraps every audited handler body. Opening
/// it, and reading the remaining time from underneath a provider hop, must both
/// be free.
#[tokio::test]
async fn the_ambient_deadline_scope_costs_nothing_to_open_or_read() {
    // Warm.
    for _ in 0..1_000 {
        verter_type_runtime::deadline::with_deadline(Duration::from_secs(1), async {
            std::hint::black_box(verter_type_runtime::deadline::remaining());
        })
        .await;
    }

    let started = Instant::now();
    for _ in 0..ITERATIONS {
        verter_type_runtime::deadline::with_deadline(Duration::from_secs(1), async {
            std::hint::black_box(verter_type_runtime::deadline::remaining());
        })
        .await;
    }
    let each = per_iteration(started.elapsed(), ITERATIONS);
    report("ambient deadline scope", each);

    assert!(
        each < PER_REQUEST_CEILING,
        "opening the ambient deadline scope and reading it costs {each:?} per request, \
         over the {PER_REQUEST_CEILING:?} ceiling"
    );
}

/// A handler that completes well inside its deadline must return as soon as it
/// has an answer. The timeout wrapper is a bound, not a barrier: it must add no
/// wait of its own to a request that already succeeded.
#[tokio::test]
async fn a_succeeding_request_never_waits_for_its_own_deadline() {
    let deadline = Duration::from_secs(5);

    let started = Instant::now();
    let result: tower_lsp_server::jsonrpc::Result<u8> =
        crate::audit_harness::run_with_deadline(deadline, async { Ok(7u8) }).await;
    let elapsed = started.elapsed();

    assert_eq!(result.expect("the body succeeds"), 7);
    assert!(
        elapsed < Duration::from_millis(5),
        "a request that already had its answer waited {elapsed:?} against a {deadline:?} \
         deadline — the bound is being treated as a settle interval"
    );
}

/// The deadline wrapper must not add measurable per-request cost to a body that
/// returns immediately. This is the whole healthy path: the timeout is armed and
/// disarmed on every single request the editor makes.
#[tokio::test]
async fn arming_the_request_deadline_costs_nothing_when_the_body_succeeds() {
    let deadline = Duration::from_secs(5);

    for _ in 0..1_000 {
        let _: tower_lsp_server::jsonrpc::Result<u8> =
            crate::audit_harness::run_with_deadline(deadline, async { Ok(7u8) }).await;
    }

    let started = Instant::now();
    for _ in 0..ITERATIONS {
        let result: tower_lsp_server::jsonrpc::Result<u8> =
            crate::audit_harness::run_with_deadline(deadline, async { Ok(7u8) }).await;
        std::hint::black_box(result.ok());
    }
    let each = per_iteration(started.elapsed(), ITERATIONS);
    report("deadline arm+disarm", each);

    assert!(
        each < PER_REQUEST_CEILING,
        "arming and disarming the request deadline costs {each:?} per request, over the \
         {PER_REQUEST_CEILING:?} ceiling"
    );
}
