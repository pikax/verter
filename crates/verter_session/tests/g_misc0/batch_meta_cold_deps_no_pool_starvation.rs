//! Regression: the component-meta BATCH path must not deadlock when its
//! per-item closures trigger COLD cross-file `Load`/`Parse` work.
//!
//! Failure class this characterises (rayon pool starvation):
//! the batch outer coordinator fans out N component-meta jobs and
//! synchronously waits for all of them. If that outer wait runs on the
//! SAME finite executor that the scheduler's driver uses to dispatch
//! the cold `Parse` each job depends on, then enough simultaneously-
//! parked batch jobs saturate every worker on that pool, the driver's
//! spawned `Parse` has no free worker, and the whole batch hangs at 0%
//! CPU forever. The invariant being protected: an outer API fan-out may
//! block only on scheduler-owned work; it must never occupy the same
//! worker set that scheduler stage execution needs to make progress.
//!
//! Why this fixture provokes it where the sibling
//! `batch_api_shared_admissions.rs` tests do not: those tests
//! deliberately `upsert_base` every file FIRST (pre-warming the host
//! caches) so the batch closures never re-enter the scheduler for a
//! cold dependency. This fixture does the opposite — every file is
//! injected COLD into the workspace VFS (discoverable but unparsed) and
//! the thread count is sized to saturate the stage pool — so each owner
//! MUST drive a deep cross-file `Load → Parse` chain DURING the batch
//! dispatch, parking a worker while it waits.
//!
//! Watchdog discipline: the batch runs on a spawned worker thread and
//! the test thread blocks on a bounded `recv_timeout`. A regression
//! FAILS the test with a timeout assertion instead of hanging the whole
//! `cargo test` process forever. This is a discriminating test: against
//! the pre-fix code (outer wait on the scheduler's own `cpu_pool`) the
//! channel never receives and the watchdog fires; against the post-fix
//! code (outer wait on the host's dedicated coordinator pool) the batch
//! completes well within the timeout and every slot resolves.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use verter_session::meta::MetaProject;
use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Number of `.vue` owners fanned out in the batch. Chosen to match the
/// configured `cpu_threads` so the batch can saturate every worker on
/// the scheduler's stage pool with parked coordinator jobs.
const N_OWNERS: usize = 16;

/// Watchdog ceiling. The post-fix batch completes in well under a
/// second on any developer machine; 60s is a generous margin that still
/// fails fast relative to a true (infinite) hang.
const WATCHDOG: Duration = Duration::from_secs(60);

/// Build a host whose workspace holds, COLD (injected, not upserted):
///
/// - `N_OWNERS` `.vue` owners; owner `i` imports `Props{i}` from
///   `types-{i}.ts` and consumes it via `defineProps`.
/// - a CHAIN of `.ts` modules `types-0 → types-1 → … → types-{N-1} →
///   base.ts`, each interface extending the next, so resolving any one
///   owner's prop type forces a deep cross-file `Load`/`Parse` walk.
///   There is NO true cycle (`base.ts` terminates the chain) — true
///   recursive types are unsupported per the project rules.
///
/// The returned project is wrapped around a workspace-backed host so a
/// batch session resolves every owner cold (no `upsert_base` pre-warm).
fn build_cold_chain_project() -> Arc<MetaProject> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));

    // Terminal of the chain: a concrete base interface every link
    // ultimately extends. No further import — this stops the walk.
    workspace.inject_file(
        "/workspace/base.ts".into(),
        Arc::from(
            r#"export interface PropsBase {
  base_id: string;
  base_seq: number;
}
"#,
        ),
    );

    // The `.ts` chain: types-i extends types-(i+1); the last link
    // (types-(N-1)) extends PropsBase from base.ts. Each `Props{i}`
    // also adds a unique own member so a per-owner result is
    // distinguishable and non-empty.
    for i in 0..N_OWNERS {
        let (next_import, next_name) = if i + 1 < N_OWNERS {
            (format!("./types-{}", i + 1), format!("Props{}", i + 1))
        } else {
            ("./base".to_string(), "PropsBase".to_string())
        };
        let src = format!(
            r#"import type {{ {next_name} }} from '{next_import}'
export interface Props{i} extends {next_name} {{
  own_{i}: string;
}}
"#
        );
        workspace.inject_file(format!("/workspace/types-{i}.ts").into(), Arc::from(src));
    }

    // The `.vue` owners. Each imports its own `Props{i}` link and
    // consumes it through `defineProps` so cold component-meta
    // resolution must walk the full `types-{i} → … → base` chain.
    for i in 0..N_OWNERS {
        let src = format!(
            r#"<script setup lang="ts">
import type {{ Props{i} }} from './types-{i}'
defineProps<Props{i}>();
</script>
<template><div /></template>
"#
        );
        workspace.inject_file(format!("/workspace/Owner{i}.vue").into(), Arc::from(src));
    }

    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    // `cpu_threads: N_OWNERS` makes the scheduler's stage pool exactly
    // as wide as the batch fan-out. Pre-fix, the outer wait runs on
    // this same pool, so N parked coordinator jobs occupy all N workers
    // and the driver-spawned cold `Parse` starves.
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: N_OWNERS,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    );
    MetaProject::new(host)
}

/// **Deadlock regression.** A batch component-meta query over
/// `N_OWNERS` cold owners, each pulling a deep cross-file type chain,
/// MUST complete (no pool-starvation hang) and resolve every slot.
///
/// Discriminator (both directions):
/// - Pre-fix (outer fan-out installed on the scheduler's own
///   `cpu_pool`): the batch jobs park waiting for cold `Parse`, the
///   driver cannot dispatch `Parse` onto the saturated pool, the result
///   channel never receives, and the `recv_timeout` watchdog FAILS the
///   test. (Confirmed by running this test against the unfixed tree
///   before the coordinator primitive landed.)
/// - Post-fix (outer fan-out installed on the host's dedicated
///   coordinator pool): the scheduler's `cpu_pool` workers stay free for
///   `Parse`, the batch completes promptly, and every one of the
///   `N_OWNERS` slots carries a `Some(analysis)` with non-empty props.
#[test]
fn batch_meta_cold_cross_file_deps_does_not_starve_scheduler_pool() {
    let project = build_cold_chain_project();

    let canonical_ids: Vec<String> = (0..N_OWNERS)
        .map(|i| format!("/workspace/Owner{i}.vue"))
        .collect();

    // Run the batch on a dedicated worker thread and watchdog the
    // result via a bounded channel so a regression fails fast instead
    // of wedging the whole test binary.
    let (tx, rx) = mpsc::channel();
    let worker_project = Arc::clone(&project);
    let worker_ids = canonical_ids.clone();
    let handle = std::thread::Builder::new()
        .name("batch-meta-deadlock-probe".to_string())
        .spawn(move || {
            // Batch execution mode is the path that fans N component-meta
            // jobs out through the coordinator — exactly the surface the
            // deadlock lived on.
            let session = worker_project
                .open_session_batch()
                .expect("open batch session");
            let results = session.get_component_meta_batch(&worker_ids);
            // Ignore send failure: if the watchdog already fired the
            // receiver is gone, and there is nothing left to assert on.
            let _ = tx.send(results);
        })
        .expect("spawn batch worker thread");

    let results = match rx.recv_timeout(WATCHDOG) {
        Ok(results) => results,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "DEADLOCK: batch component-meta over {N_OWNERS} cold owners did not complete within \
             {WATCHDOG:?}. The outer batch fan-out is starving the scheduler's stage pool — its \
             coordinator wait must run on the host's dedicated coordinator pool, not on the same \
             `cpu_pool` the driver uses to dispatch cold `Parse`."
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("batch worker thread dropped the result channel without sending (panicked?)")
        }
    };

    // The worker finished; join it so a panic inside the batch surfaces
    // as a test failure rather than a silently-detached thread.
    handle.join().expect("batch worker thread panicked");

    let results = results.expect("batch dispatch should return Ok (project alive)");

    // Negative assertion: NO slot may be missing. Every owner must
    // resolve to `Some(analysis)` with at least one published prop —
    // a `None` or an empty-props slot would mean the cold cross-file
    // walk silently dropped a result even though the batch returned.
    assert_eq!(
        results.len(),
        N_OWNERS,
        "batch must return one slot per input (got {}, expected {N_OWNERS})",
        results.len(),
    );
    for (i, slot) in results.iter().enumerate() {
        let analysis = slot
            .as_ref()
            .unwrap_or_else(|err| panic!("slot {i} (Owner{i}.vue) failed: {err:?}"))
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "slot {i} (Owner{i}.vue) returned None — cold cross-file resolution dropped \
                     a result"
                )
            });
        assert!(
            !analysis.props.is_empty(),
            "slot {i} (Owner{i}.vue) must publish at least the chained props (got empty props \
             — the cross-file type chain did not resolve)",
        );
    }
}
