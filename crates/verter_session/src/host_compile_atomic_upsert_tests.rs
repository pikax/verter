//! Atomic Stage-B upsert tests — `compile_many`'s Stage-B upsert path is a SINGLE
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

use crate::host_compile::{CompileBatchInput, CompileBatchOptions, CompileManyTarget};
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::types::{FileLanguage, HostConfig, HostError, UpsertRequest};
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
        component_id: None,
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
        file_language: FileLanguage::vue(),
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

    let entries = host.compile_many(
        inputs,
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
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
    let entries = host.compile_many(
        inputs,
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );

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
            CompileManyTarget::HostBacked,
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
/// the upsert engine:
///   - `Ready(_)`     → `finish_upsert_post_commit(...)`
///   - `Failed(e)`    → `HostError::Scheduler(e)`
///   - `Superseded`   → `HostError::Superseded`
///   - `Shutdown`     → `HostError::Shutdown`
///
/// This test drives a REAL batch through the ACTUAL mapper
/// (`UpsertBatchTxn::map_states`, the SAME code `finish` runs after
/// `wait_batch`) with a MIXED state vector covering all four arms, then
/// pins each per-index outcome:
///
///  - It builds the genuine transaction via `test_submit_upsert_batch_parts`
///    (real `submit_batch_atomic`, real committed source snapshots),
///    takes the genuine `Ready` states via `wait_batch`, then SPLICES in
///    synthetic `Failed` / `Superseded` / `Shutdown` states at chosen
///    indices and feeds the whole vector to `finish_from_states`.
///  - The Ready index therefore routes through the real
///    `finish_upsert_post_commit` against a really-committed source; the
///    three failure indices route through the real error arms. No mapping
///    logic is reconstructed in the test — only the *source* of the
///    non-Ready states is synthetic.
///
/// Discriminating properties (would FAIL on a regressed mapper that the
/// hand-built-`HostError` predecessor could not catch):
///  1. **No early-return.** All four indices are present in the output.
///  2. **Ready → Ok with its canonical.** A mapper that swapped the
///     Ready arm to an error, or attached the wrong canonical, fails.
///  3. **Each failure arm → EXACT `upsert failed: {e}` string.** A
///     transposed arm (e.g. `Superseded`↦`Shutdown`) changes the per-
///     index string and fails. The strings render through the SAME
///     `HostError::Display` Stage B uses
///     (`format!("upsert failed: {e}")`).
#[test]
fn upsert_batch_completion_mapping_preserves_error_strings() {
    use verter_scheduler::job::{CompletionState, RequestResult, SchedulerError};

    let host = new_host();

    // Index → canonical. Index 0 is the only Ready arm; 1..=3 are the
    // three non-Ready terminal arms.
    let reqs = vec![
        upsert_req("/map-ready.vue", &good_template("ready")),
        upsert_req("/map-failed.vue", &good_template("failed")),
        upsert_req("/map-superseded.vue", &good_template("superseded")),
        upsert_req("/map-shutdown.vue", &good_template("shutdown")),
    ];
    let ids: Vec<String> = reqs
        .iter()
        .map(|r| r.canonical_id.clone().unwrap())
        .collect();

    // Build the REAL transaction (real submit + committed sources) and
    // take the genuine states via the production `wait_batch`.
    let (prepared, batch) = host.test_submit_upsert_batch_parts(reqs, Priority::Background);
    let mut states: Vec<CompletionState<RequestResult>> = host.scheduler.wait_batch(&batch);
    assert_eq!(
        states.len(),
        4,
        "one completion state per submitted request"
    );
    assert!(
        matches!(states[0], CompletionState::Ready(_)),
        "index 0 must have genuinely committed to Ready so the Ready arm \
         exercises the real finish_upsert_post_commit against a committed \
         source; got {:?}",
        states[0]
    );

    // Splice synthetic non-Ready terminal states at 1..=3. The Ready
    // state at index 0 is left untouched (a real committed snapshot).
    states[1] = CompletionState::Failed(SchedulerError::FileNotFound {
        file_id: "/x.vue".to_string(),
    });
    states[2] = CompletionState::Superseded;
    states[3] = CompletionState::Shutdown;

    // Drive the REAL mapper. `finish_from_states` calls the same
    // `map_states` the production `finish` calls — only the state source
    // is controlled.
    let outcomes = crate::host_upsert::UpsertBatchTxn::finish_from_states(&host, prepared, states);

    // (1) No early-return: every index present, in input order.
    assert_eq!(
        outcomes.len(),
        4,
        "the mapper must map EVERY index — a partial failure must not \
         early-return"
    );
    for (i, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            outcome.canonical_id, ids[i],
            "outcome[{i}] must carry its prepared canonical"
        );
    }

    // (2) Ready arm → Ok carrying its canonical.
    match &outcomes[0].result {
        Ok(update) => assert_eq!(
            update.canonical_id, ids[0],
            "Ready arm must route through finish_upsert_post_commit and \
             carry the request's canonical"
        ),
        Err(e) => panic!("Ready arm must map to Ok, got error: {e}"),
    }

    // (3) Each failure arm → its EXACT `upsert failed: {e}` string,
    //     rendered through the same `HostError::Display` Stage B uses.
    let render = |r: &Result<crate::types::HostUpdateResult, HostError>| match r {
        Ok(_) => panic!("expected an Err arm"),
        Err(e) => format!("upsert failed: {e}"),
    };
    assert_eq!(
        render(&outcomes[1].result),
        "upsert failed: scheduler error: file not found: /x.vue",
        "Failed(SchedulerError) → HostError::Scheduler exact string"
    );
    assert_eq!(
        render(&outcomes[2].result),
        "upsert failed: request superseded by newer generation",
        "Superseded → HostError::Superseded exact string"
    );
    assert_eq!(
        render(&outcomes[3].result),
        "upsert failed: scheduler shut down",
        "Shutdown → HostError::Shutdown exact string"
    );
}

// ---------------------------------------------------------------------------
// 5. Completion indices map to prepared canonicals regardless of order (P0)
// ---------------------------------------------------------------------------

/// The mapper zips `state[i]` with `prepared[i]`, and that pairing must
/// be the IDENTITY zip — `state[i] ↔ prepared[i]` for every i. The
/// predecessor of this test used N all-`Ready` requests where
/// `finish_upsert_post_commit` reads back by the prepared canonical, so a
/// reversed `state[i]` list still produced the right per-index canonical
/// and the test could NOT detect a transposition.
///
/// This version makes a transposition OBSERVABLE by giving each index a
/// DISTINCT terminal state through the real mapper
/// (`UpsertBatchTxn::finish_from_states` → `map_states`): one `Ready`
/// arm and three distinct error arms (`Failed`/`Superseded`/`Shutdown`),
/// each error carrying a per-index-unique payload. Because every index's
/// expected outcome is unique, swapping ANY two completion states changes
/// at least one per-index outcome and trips an assertion. The test
/// proves this directly: it asserts the in-order mapping, then re-runs
/// the mapper with two states swapped and asserts the swapped outcome no
/// longer matches the in-order expectation.
#[test]
fn upsert_batch_result_indices_map_to_prepared_canonicals() {
    use verter_scheduler::job::{CompletionState, RequestResult, SchedulerError};

    let host = new_host();
    let reqs = vec![
        upsert_req("/zip-ready.vue", &good_template("zr")),
        upsert_req("/zip-failed.vue", &good_template("zf")),
        upsert_req("/zip-superseded.vue", &good_template("zs")),
        upsert_req("/zip-shutdown.vue", &good_template("zd")),
    ];
    let ids: Vec<String> = reqs
        .iter()
        .map(|r| r.canonical_id.clone().unwrap())
        .collect();

    // Per-index DISTINCT terminal states. Index 0 stays the genuine
    // `Ready` (committed source); 1..=3 are distinct error arms.
    let build_states =
        |ready: CompletionState<RequestResult>| -> Vec<CompletionState<RequestResult>> {
            vec![
                ready,
                CompletionState::Failed(SchedulerError::FileNotFound {
                    file_id: "/zip-failed-payload.vue".to_string(),
                }),
                CompletionState::Superseded,
                CompletionState::Shutdown,
            ]
        };

    // The per-index expected error string (None ⇒ the Ready/Ok index).
    let expected_err: [Option<&str>; 4] = [
        None,
        Some("upsert failed: scheduler error: file not found: /zip-failed-payload.vue"),
        Some("upsert failed: request superseded by newer generation"),
        Some("upsert failed: scheduler shut down"),
    ];

    // ── In-order run: each state at its own index ──
    let (prepared, batch) = host.test_submit_upsert_batch_parts(reqs, Priority::Interactive);
    let mut states = host.scheduler.wait_batch(&batch);
    assert!(
        matches!(states[0], CompletionState::Ready(_)),
        "index 0 must commit to Ready"
    );
    let ready0 = std::mem::replace(&mut states[0], CompletionState::Superseded);
    let in_order = build_states(ready0);

    let outcomes =
        crate::host_upsert::UpsertBatchTxn::finish_from_states(&host, prepared, in_order);
    assert_eq!(outcomes.len(), 4, "one outcome per index");

    for (i, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            outcome.canonical_id, ids[i],
            "outcome[{i}].canonical_id must equal the i-th prepared canonical"
        );
        match (&outcome.result, expected_err[i]) {
            (Ok(update), None) => assert_eq!(
                update.canonical_id, ids[i],
                "the Ready post-commit result for index {i} must carry the \
                 SAME canonical — a transposed zip would attach the result \
                 to the wrong prepared entry"
            ),
            (Err(e), Some(want)) => assert_eq!(
                format!("upsert failed: {e}"),
                want,
                "index {i} must map to its OWN terminal state's error string"
            ),
            (got, want) => panic!(
                "index {i}: outcome/expectation mismatch — got {got:?}, \
                 expected error {want:?}"
            ),
        }
    }

    // ── Transposition is observable: swap states[1]↔states[2] and prove
    //    the per-index outcome changes (the identity zip is load-bearing).
    //    A mapper that ignored index alignment would yield the SAME
    //    per-index outcomes as the in-order run; here index 1 must flip
    //    from the `Failed` string to the `Superseded` string and index 2
    //    vice-versa.
    let (prepared2, batch2) = host.test_submit_upsert_batch_parts(
        vec![
            upsert_req("/zip-ready.vue", &good_template("zr")),
            upsert_req("/zip-failed.vue", &good_template("zf")),
            upsert_req("/zip-superseded.vue", &good_template("zs")),
            upsert_req("/zip-shutdown.vue", &good_template("zd")),
        ],
        Priority::Interactive,
    );
    let mut states2 = host.scheduler.wait_batch(&batch2);
    let ready0b = std::mem::replace(&mut states2[0], CompletionState::Superseded);
    let mut swapped = build_states(ready0b);
    swapped.swap(1, 2);

    let swapped_outcomes =
        crate::host_upsert::UpsertBatchTxn::finish_from_states(&host, prepared2, swapped);
    // Index 1 now holds the `Superseded` state; index 2 the `Failed` one.
    assert_eq!(
        match &swapped_outcomes[1].result {
            Err(e) => format!("upsert failed: {e}"),
            Ok(_) => panic!("index 1 must be an error after the swap"),
        },
        "upsert failed: request superseded by newer generation",
        "after swapping states[1]↔states[2], index 1 MUST reflect the \
         state now at position 1 (Superseded) — proving the mapper pairs \
         state[i] with prepared[i] positionally, not by content"
    );
    assert_eq!(
        match &swapped_outcomes[2].result {
            Err(e) => format!("upsert failed: {e}"),
            Ok(_) => panic!("index 2 must be an error after the swap"),
        },
        "upsert failed: scheduler error: file not found: /zip-failed-payload.vue",
        "after the swap, index 2 MUST reflect the Failed state now at \
         position 2"
    );
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
            let entries = host.compile_many(
                inputs,
                CompileBatchOptions::default(),
                CompileManyTarget::HostBacked,
            );
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

    assert!(
        host.compile_one_call_count.load(Ordering::Relaxed) >= N,
        "every unique canonical must have been compiled at least once"
    );
}

// ---------------------------------------------------------------------------
// 7. Duplicate canonicals NEVER reach submit_batch_atomic (P0-1)
// ---------------------------------------------------------------------------

/// The engine's canonical-uniqueness check must EXIST, must be computed
/// BEFORE the atomic submission, and must run BEFORE every per-request side
/// effect. A source-updating batch carrying two requests for the SAME
/// canonical would bump that node's generation twice under the single
/// `dag.lock()` acquisition inside `submit_batch_atomic`, self-superseding
/// the earlier admit and corrupting the batch — `submit_batch_atomic` does
/// not dedup.
///
/// SCOPE. This is a DEBUG-profile test: it drives the scheduler's per-admit
/// epoch trace (`test_install_batch_admit_epoch_trace` /
/// `test_take_batch_admit_epochs`), whose hooks are gated `#[cfg(any(test,
/// debug_assertions))]` in the scheduler crate, so a `--release` test run
/// does NOT compile this test. It therefore proves the EXISTENCE and
/// ORDERING of the check in the debug profile; it does NOT, and cannot,
/// prove the check is RELEASE-ACTIVE. Release-activeness (the check must be
/// a real `assert!`/`panic!`, never a `debug_assert!` that a release build
/// compiles out) is enforced statically by
/// `tests/cases/g_misc0/uniqueness_check_release_active.rs`, which extracts the
/// `assert_canonicals_unique` fn body and fails on a `debug_assert*!`
/// downgrade.
///
/// Discriminating properties (this test FAILS against the pre-fix tree,
/// where the check was a `debug_assert!` that ran AFTER the per-request
/// side-effect loop AND after building the scheduler request list — in the
/// DEBUG profile the old `debug_assert!` still fired, but it fired in the
/// WRONG ORDER, which properties 2 and 4 below detect):
///
///  1. **The call panics.** Driving `upsert_many_with_priority` with a
///     duplicated canonical unwinds (caught here), proving the check
///     EXISTS and fires in this debug build. (This says nothing about
///     release — see SCOPE above; the static guard pins the release form.)
///  2. **No batch was admitted.** The per-admit epoch trace — populated
///     EXCLUSIVELY by `handle_new_request_batch` (the body of
///     `submit_batch_atomic`) — is EMPTY after the panic, proving the
///     check fired BEFORE submission. A regression that moved the check
///     back after `submit_batch_atomic` would record ≥1 epoch here.
///  3. **No source was committed.** The scheduler holds NO source
///     snapshot for the duplicated canonical afterwards, corroborating
///     that the atomic submission never ran for this batch.
///  4. **The check runs BEFORE the per-request side-effect loop.** The
///     `#[cfg(test)]` `last_upsert_priority` observable is written
///     INSIDE that loop (once per request). After the panic it must
///     still be `None` — proving the uniqueness check fired before ANY
///     per-request side effect. The pre-fix `debug_assert!` ran AFTER
///     the loop, so the first duplicate had already written
///     `Some(priority)` here; this assertion fails against that pre-fix
///     ordering in the debug test build.
#[test]
fn upsert_duplicate_canonical_panics_before_submit_batch_atomic() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let host = new_host();
    let dup = "/dup-engine.vue";

    // The side-effect-ordering observable starts unset on a fresh host.
    assert!(
        host.last_upsert_priority.lock().is_none(),
        "precondition: fresh host has no recorded upsert priority"
    );

    // Arm the per-admit epoch recorder BEFORE the (expected-to-panic)
    // call. It is populated only from inside `submit_batch_atomic`'s
    // `handle_new_request_batch`, so an empty trace afterward proves the
    // uniqueness check fired before any admission.
    host.scheduler.test_install_batch_admit_epoch_trace();

    // Two source-updating requests for ONE canonical in a single batch.
    let reqs = vec![
        upsert_req(dup, &good_template("first")),
        upsert_req(dup, &good_template("second")),
    ];

    let result = catch_unwind(AssertUnwindSafe(|| {
        host.upsert_many_with_priority(reqs, Priority::Interactive)
    }));

    assert!(
        result.is_err(),
        "a duplicate-canonical batch must PANIC through the uniqueness \
         assertion before reaching `submit_batch_atomic` (proven here in the \
         debug profile). The production check MUST be release-active so the \
         duplicate also cannot silently corrupt the batch in a release build \
         where a `debug_assert!` would be compiled out — that release form is \
         pinned statically by `uniqueness_check_release_active.rs`, since this \
         test does not compile under `--release`"
    );

    let epochs = host.scheduler.test_take_batch_admit_epochs();
    assert!(
        epochs.is_empty(),
        "the uniqueness check must fire BEFORE `submit_batch_atomic` — the \
         per-admit epoch trace (populated only inside \
         `handle_new_request_batch`) must be EMPTY, proving NO batch was \
         admitted. Got {} admit epoch(s): {epochs:?}. A non-empty trace \
         means the duplicate reached the atomic admission path.",
        epochs.len()
    );

    assert!(
        host.scheduler.try_get_source(dup).is_none(),
        "no source snapshot must have been committed for the duplicated \
         canonical `{dup}` — the panic must precede the atomic submission \
         that would commit it"
    );

    // Ordering: the uniqueness check fired BEFORE the per-request
    // side-effect loop, so the in-loop `last_upsert_priority` observable
    // was never written. The pre-fix post-loop `debug_assert!` would have
    // left this `Some(Priority::Interactive)` (the first duplicate ran
    // the loop body before the panic).
    assert!(
        host.last_upsert_priority.lock().is_none(),
        "the uniqueness check must run BEFORE any per-request side effect — \
         `last_upsert_priority` (written inside the per-request loop) must \
         still be None after the panic. A recorded priority means the \
         check ran AFTER the side-effect loop (the pre-fix ordering)."
    );
}
