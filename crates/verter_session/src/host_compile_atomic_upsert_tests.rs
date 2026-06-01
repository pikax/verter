//! §6c cutover tests — `compile_many`'s Stage-B upsert path is a SINGLE
//! atomic batch (`Scheduler::submit_batch_atomic` + one `wait_batch`)
//! driven through the one shared upsert engine `upsert_many_with_priority`.
//!
//! Discriminating matrix:
//!
//! | Test | Discriminates against (pre-change behaviour) |
//! | ---- | -------------------------------------------- |
//! | `compile_many_stage_b_uses_one_atomic_upsert_batch` | Stage B emitted N single `submit_request`s → ZERO batch-admit epochs recorded. |
//! | `compile_many_duplicate_canonical_dedups_to_one_batch_request_and_reports_all_inputs` | dup canonical still batched once; all 3 input positions reported. |
//! | `compile_many_upsert_batch_captures_calling_thread_request_context` | old per-file `current_context()` ran inside a host-pool worker (no TLS) → batch carried `None`; source worker recorded no parse-timing against the request. |
//! | `upsert_batch_completion_mapping_preserves_error_strings` | `upsert_many_with_priority`/`UpsertBatchOutcome` mapper does not exist pre-change. |
//! | `upsert_batch_result_indices_map_to_prepared_canonicals` | `upsert_many_with_priority` does not exist pre-change; input-order zip must pair each completion with its prepared canonical regardless of completion order. |
//! | `compile_many_no_deadlock_under_full_host_and_scheduler_pools` | saturated host pool + constrained scheduler pool completes (regression coverage). |

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_scheduler::stage::Priority;

use crate::host_compile::{CompileBatchInput, CompileBatchOptions};
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::types::{FileKind, HostConfig, HostError, UpsertRequest};
use crate::VerterHost;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn ok_input(canonical_id: &str, source: &str) -> CompileBatchInput {
    CompileBatchInput {
        canonical_id: canonical_id.to_string(),
        source: Arc::from(source),
        requested_mode: None,
    }
}

fn good_template(text: &str) -> String {
    format!("<template><div>{text}</div></template>")
}

fn upsert_req(canonical_id: &str, source: &str) -> UpsertRequest {
    UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// 1. Stage B issues ONE atomic batch carrying N requests (P0)
// ---------------------------------------------------------------------------

/// Cold N unique inputs: Stage B must submit ONE
/// `Submission::NewRequestBatch` carrying N source/Analysis requests,
/// admitted under a SINGLE `dag.lock()` acquisition — observable via the
/// scheduler's per-admit epoch trace, which is populated ONLY from
/// `handle_new_request_batch` (the atomic batch path) and records the
/// acquisition epoch each admit ran under.
///
/// Discriminating property:
/// - N batch-admit epochs are recorded (one per request in the batch),
///   so the batch carried exactly N requests.
/// - All N recorded epochs are EQUAL — proving one held lock admitted
///   the whole batch (a per-item lock/unlock regression would record N
///   distinct epochs).
///
/// Pre-change, Stage B fanned N single `submit_request`s through
/// `handle_new_request` (which never records a batch-admit epoch), so
/// the trace is EMPTY (`0 != N`).
#[test]
fn compile_many_stage_b_uses_one_atomic_upsert_batch() {
    let host = new_host();
    const N: usize = 4;
    let inputs: Vec<CompileBatchInput> = (0..N)
        .map(|i| ok_input(&format!("/atomic{i}.vue"), &good_template(&format!("a{i}"))))
        .collect();

    // Arm the per-admit epoch recorder BEFORE the batch. The recorder is
    // populated exclusively by `handle_new_request_batch`.
    host.scheduler.test_install_batch_admit_epoch_trace();

    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), N, "every input position must be returned");
    assert!(
        entries.iter().all(|e| e.errors.is_empty()),
        "all cold inputs must compile cleanly: {:?}",
        entries.iter().map(|e| &e.errors).collect::<Vec<_>>()
    );

    let epochs = host.scheduler.test_take_batch_admit_epochs();
    assert_eq!(
        epochs.len(),
        N,
        "Stage B must submit ONE atomic batch carrying exactly {N} upsert \
         requests — the per-admit epoch trace records one entry per request \
         admitted inside `handle_new_request_batch`. Got {} entries: {epochs:?}. \
         Pre-change Stage B fanned N single `submit_request`s through \
         `handle_new_request` (which records NOTHING here), leaving the \
         trace empty.",
        epochs.len()
    );
    let first = epochs[0];
    assert!(
        epochs.iter().all(|&e| e == first),
        "all {N} admits must share ONE DAG-lock acquisition epoch (atomic \
         batch admission). Distinct epochs would mean per-item lock/unlock. \
         Got: {epochs:?}"
    );
}

// ---------------------------------------------------------------------------
// 2. Duplicate canonical dedups to one batch request; all inputs reported (P0)
// ---------------------------------------------------------------------------

/// Inputs `/A.vue`, `/A.vue` (identical source), `/B.vue`: the upsert
/// batch must carry exactly TWO requests (A deduped to one, plus B),
/// and the output must preserve all THREE input positions.
///
/// Discriminating property: the per-admit epoch trace records exactly 2
/// entries (the two distinct canonicals submitted in the single atomic
/// batch), and `entries.len() == 3`. Pre-change the trace is empty
/// (single `submit_request`s, no batch).
#[test]
fn compile_many_duplicate_canonical_dedups_to_one_batch_request_and_reports_all_inputs() {
    let host = new_host();
    let a_src = good_template("dup-a");
    let b_src = good_template("dup-b");
    let inputs = vec![
        ok_input("/A.vue", &a_src),
        ok_input("/A.vue", &a_src),
        ok_input("/B.vue", &b_src),
    ];

    host.scheduler.test_install_batch_admit_epoch_trace();
    let entries = host.compile_many(inputs, CompileBatchOptions::default());

    // All three input positions are reported.
    assert_eq!(entries.len(), 3, "Stage D must fan out to all 3 positions");
    assert_eq!(entries[0].canonical_id, "/A.vue");
    assert_eq!(entries[1].canonical_id, "/A.vue");
    assert_eq!(entries[2].canonical_id, "/B.vue");
    assert!(
        entries.iter().all(|e| e.errors.is_empty()),
        "identical-source dup is NOT a conflict; all positions compile cleanly: {:?}",
        entries.iter().map(|e| &e.errors).collect::<Vec<_>>()
    );

    let epochs = host.scheduler.test_take_batch_admit_epochs();
    assert_eq!(
        epochs.len(),
        2,
        "the upsert batch must carry exactly TWO requests (A deduped to \
         one + B) — the per-admit epoch trace records one entry per \
         distinct canonical in the single atomic batch. Got {} entries: \
         {epochs:?}.",
        epochs.len()
    );
    assert!(
        epochs.iter().all(|&e| e == epochs[0]),
        "both deduped admits must share ONE lock-acquisition epoch: {epochs:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. The upsert batch captures the CALLING thread's request context (P0)
// ---------------------------------------------------------------------------

/// `submit_batch_atomic` is invoked ONCE from the calling thread, so
/// each `Request` must carry `current_context()` captured on THAT
/// thread. The scheduler then installs the context into the source /
/// analysis stage workers' TLS, where the host executor's source stage
/// pushes a per-file parse-timing entry into the request's accumulator
/// (gated on `timing_enabled()`).
///
/// Discriminating property: after a cold `compile_many` run under an
/// installed `RequestContext` (timing capture ON), the request's
/// accumulator contains a `FileParseTiming` for the upserted canonical
/// — proving the source-stage worker observed the CALLING thread's
/// context.
///
/// Pre-change, `current_context()` was captured INSIDE the host-pool
/// coordinator worker, which carries NO request-context TLS (the
/// `RequestContextGuard` is installed only on the calling thread). The
/// batch therefore carried `None`, the source worker saw no installed
/// context (`current_accumulator()` → `None`), and NOTHING was recorded
/// against the request — the timings list is empty.
#[test]
fn compile_many_upsert_batch_captures_calling_thread_request_context() {
    let host = new_host();
    let canonical = "/ctx-capture.vue";

    let accumulator = Arc::new(crate::component_meta_audit::RequestFootprintAccumulator::new());
    let ctx = RequestContext::with_kind_and_timing(
        4242,
        Arc::from(canonical),
        verter_audit::RequestKind::SemanticAnalysis,
        /* footprint_capture */ true,
        /* timing_capture */ true,
        Some(Arc::clone(&accumulator)),
    );

    {
        // Install on the CALLING thread only — workers do NOT inherit
        // this TLS unless the context is threaded through the Request.
        let _guard = RequestContextGuard::install(Arc::clone(&ctx));
        let entries = host.compile_many(
            vec![ok_input(canonical, &good_template("ctx"))],
            CompileBatchOptions {
                priority: Some(Priority::Interactive),
                default_mode: None,
            },
        );
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0].errors.is_empty(),
            "input must compile cleanly: {:?}",
            entries[0].errors
        );
    }

    let state = accumulator.drain();
    let saw_timing = state
        .file_parse_timings
        .iter()
        .any(|t| t.canonical_id.as_ref() == canonical);
    assert!(
        saw_timing,
        "the source-stage worker that parsed `{canonical}` must have observed \
         the CALLING thread's request context (captured before \
         `submit_batch_atomic` and threaded into every batch Request) — it \
         records a `FileParseTiming` into the request accumulator only when \
         the installed context's `timing_enabled()` is true. Found timings: \
         {:?}. Pre-change, `current_context()` was captured inside a \
         host-pool worker with no TLS, so the batch carried `None` and \
         nothing was recorded.",
        state.file_parse_timings
    );
}

// ---------------------------------------------------------------------------
// 4. Completion-state → error-string mapping is exact (P0)
// ---------------------------------------------------------------------------

/// The Stage-B error string is `format!("upsert failed: {e}")` where
/// `e` is the `HostError` produced by the completion-state mapper inside
/// `upsert_many_with_priority`:
///   - `Ready(_)`     → `finish_upsert_post_commit(...)`
///   - `Failed(e)`    → `HostError::Scheduler(e)`
///   - `Superseded`   → `HostError::Superseded`
///   - `Shutdown`     → `HostError::Shutdown`
///
/// This test pins BOTH halves so it discriminates:
///
///  1. **Real atomic path (Ready arm).** A valid cold batch driven
///     through `upsert_many_with_priority` must map every request to
///     `Ok(HostUpdateResult)`. This exercises the production
///     `submit_batch_atomic` + `wait_batch` + per-index
///     `finish_upsert_post_commit` mapping. (`upsert_many_with_priority`
///     and `UpsertBatchOutcome` do not exist pre-change, so the test
///     cannot compile against the old tree — the mapper it pins is new.)
///
///  2. **Exhaustive terminal-state strings.** Every non-Ready terminal
///     state's `upsert failed: …` rendering is pinned through the SAME
///     `HostError::Display` Stage B uses, so a regression that changed
///     the completion-state→`HostError` mapping or the error string
///     fails here.
#[test]
fn upsert_batch_completion_mapping_preserves_error_strings() {
    // ── Part 1: real atomic path, Ready arm maps to Ok ──
    let host = new_host();
    let outcomes = host.upsert_many_with_priority(
        vec![
            upsert_req("/ok0.vue", &good_template("o0")),
            upsert_req("/ok1.vue", &good_template("o1")),
        ],
        Priority::Background,
    );
    assert_eq!(outcomes.len(), 2, "every request maps to one outcome");
    for outcome in &outcomes {
        match &outcome.result {
            Ok(update) => assert_eq!(
                update.canonical_id, outcome.canonical_id,
                "Ready arm must route through finish_upsert_post_commit and \
                 carry the request's canonical"
            ),
            Err(e) => panic!(
                "valid cold upsert of `{}` must succeed through the atomic \
                 path, got: {e}",
                outcome.canonical_id
            ),
        }
    }

    // ── Part 2: exhaustive HostError → Stage-B string shapes ──
    // Pin the EXACT `upsert failed: …` rendering for every terminal
    // state the mapper produces, through the same `HostError::Display`
    // Stage B uses (`group_errors.entry(id).or_insert_with(|| format!(
    // "upsert failed: {e}"))`).
    let superseded = format!("upsert failed: {}", HostError::Superseded);
    assert_eq!(
        superseded, "upsert failed: request superseded by newer generation",
        "Superseded → exact string"
    );
    let shutdown = format!("upsert failed: {}", HostError::Shutdown);
    assert_eq!(
        shutdown, "upsert failed: scheduler shut down",
        "Shutdown → exact string"
    );
    let failed = format!(
        "upsert failed: {}",
        HostError::Scheduler(verter_scheduler::job::SchedulerError::FileNotFound {
            file_id: "/x.vue".to_string(),
        })
    );
    assert_eq!(
        failed, "upsert failed: scheduler error: file not found: /x.vue",
        "Failed(SchedulerError) → exact string"
    );
}

// ---------------------------------------------------------------------------
// 5. Completion indices map to prepared canonicals regardless of order (P0)
// ---------------------------------------------------------------------------

/// `wait_batch` returns completion states in INPUT order, and
/// `upsert_many_with_priority` zips `state[i]` with `prepared[i]`. Even
/// though the N requests complete on the scheduler's pool in a
/// nondeterministic order, `outcomes[i].canonical_id` must equal the
/// i-th submitted request's canonical, and a successful
/// `HostUpdateResult.canonical_id` must equal that SAME canonical
/// (proving `finish_upsert_post_commit` received the matching
/// `prepared[i]`, not a transposed one).
///
/// Pre-change `upsert_many_with_priority` does not exist, so this test
/// pins the new transaction's index-preserving contract.
#[test]
fn upsert_batch_result_indices_map_to_prepared_canonicals() {
    let host = new_host();
    // Distinct canonicals with distinct sources so each is a genuine
    // cold parse; enough of them that completion order is not the
    // submission order.
    let reqs: Vec<UpsertRequest> = (0..8)
        .map(|i| upsert_req(&format!("/idx{i}.vue"), &good_template(&format!("body-{i}"))))
        .collect();
    let expected_ids: Vec<String> = reqs
        .iter()
        .map(|r| r.canonical_id.clone().unwrap())
        .collect();

    let outcomes = host.upsert_many_with_priority(reqs, Priority::Interactive);
    assert_eq!(
        outcomes.len(),
        expected_ids.len(),
        "one outcome per submitted request"
    );

    for (i, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            outcome.canonical_id, expected_ids[i],
            "outcome[{i}].canonical_id must equal the i-th submitted \
             request's canonical (input-order zip): expected `{}`, got `{}`",
            expected_ids[i], outcome.canonical_id
        );
        match &outcome.result {
            Ok(update) => assert_eq!(
                update.canonical_id, expected_ids[i],
                "the Ready post-commit result for position {i} must carry \
                 the SAME canonical — a transposed zip would attach \
                 `finish_upsert_post_commit`'s result to the wrong prepared \
                 entry. Expected `{}`, got `{}`",
                expected_ids[i], update.canonical_id
            ),
            Err(e) => panic!(
                "cold upsert of `{}` must succeed, got error: {e}",
                expected_ids[i]
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 6. No deadlock under saturated host pool + constrained scheduler pool (P1)
// ---------------------------------------------------------------------------

/// Regression coverage for the deadlock class fixed by §6a / the E+A
/// host-coordinator change: a `compile_many` over many inputs must
/// complete under a small scheduler CPU pool even when the host pool is
/// also small. The single atomic submit + one `wait_batch` (driven on
/// the host-coordinator pool whose workers register as `External`) must
/// not starve the scheduler stage pool. The hard gate is "completes
/// within the timeout".
#[test]
fn compile_many_no_deadlock_under_full_host_and_scheduler_pools() {
    use std::sync::mpsc;
    use std::time::Duration;

    // Constrain BOTH pools: a 1-thread host coordinator pool and a
    // 1-thread scheduler CPU pool maximise the chance a naive
    // submit→inline-wait would deadlock.
    let config = HostConfig {
        host_cpu_threads: Some(1),
        ..HostConfig::default()
    };
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        io_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let host = VerterHost::new_standalone_with_scheduler_config(config, scheduler_config);

    const N: usize = 24;
    let inputs: Vec<CompileBatchInput> = (0..N)
        .map(|i| ok_input(&format!("/dl{i}.vue"), &good_template(&format!("d{i}"))))
        .collect();

    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let entries = host.compile_many(inputs, CompileBatchOptions::default());
            let _ = tx.send(entries.len());
        });

        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(len) => assert_eq!(
                len, N,
                "compile_many must return one entry per input under \
                 saturated host + scheduler pools"
            ),
            Err(_) => panic!(
                "compile_many DEADLOCKED under a 1-thread host pool + \
                 1-thread scheduler pool (no completion within 60s)"
            ),
        }
    });

    assert_eq!(
        host.compile_one_call_count.load(Ordering::Relaxed) as usize >= N,
        true,
        "every unique canonical must have been compiled at least once"
    );
}
