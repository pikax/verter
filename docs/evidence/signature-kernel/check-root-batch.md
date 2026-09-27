# Check-root batch API — design note

A design for a separate task, not implemented on this branch. It answers
the finding of the signature-kernel benchmark's `scheduler_scaling`
section: one caller gets no speedup from more host workers, because
every semantic query runs synchronously on the calling thread and the
scheduler's CPU workers do none of its work (CPU time equals wall time
at 1, 2, 4 and 8 workers). The only way a caller parallelises semantic
work today is to run several threads itself, which is what the
`full_check` section measures.

## Shape

```rust
impl VerterHost {
    /// Answer every root of `roots`, each an independent top-level
    /// demand, fanned out across the host's coordinator pool. Returns
    /// one outcome per root, in input order.
    pub fn check_roots(
        &self,
        roots: Vec<CheckRoot>,
        options: CheckBatchOptions,
    ) -> Vec<CheckRootOutcome>;
}

pub enum CheckRoot {
    /// One function's whole-return flow demand (what the benchmark's
    /// witnesses are).
    FlowReturn(FlowFunctionReturnIdentity),
    /// Every checkable root of one file: its exported declarations'
    /// flow returns and declared-type relations, as a project check
    /// enumerates them.
    File(String),
}

pub struct CheckBatchOptions {
    pub priority: Option<Priority>,
    /// Cancels every root of the batch that has not finished.
    pub cancellation: Option<CancellationToken>,
}
```

`CheckRootOutcome` carries the root's `AuditedResult` exactly as the
single-root entry point returns it, so a batch changes where roots run,
never what they answer.

## Where roots come from

- A caller that already knows its demands (an editor's visible
  functions, the benchmark's witnesses) passes `FlowReturn` roots.
- A project check passes `File` roots. The host expands a file root into
  the file's top-level check demands from the file's indexed declaration
  inventory — the same inventory the declaration-lowering workers
  already produce — in declaration order, so the expansion is
  deterministic and needs no new resident structure.

## How it uses the scheduler

- **Coarse grain only.** Each root is one item of
  `HostBatchCoordinator::run_batch` on the host-owned coordinator pool
  (`HostCpuPool`), the primitive `compile_many` already uses. An item
  runs the existing synchronous audited entry point for its root. Leaf
  queries stay synchronous on the item's worker: no semantic query is
  ever split across threads, so the dispatch transaction, the connected
  demand's budget and the reentry intercept keep their single-thread
  invariants.
- **No stage-pool waits.** Items run on the coordinator pool, never on
  the scheduler's stage `cpu_pool`, so an item that waits for a file's
  load or analysis stage cannot occupy the worker that stage needs (the
  coordinator's documented isolation rule).
- **Shared memo, single flight.** Two roots that demand the same
  sub-query meet in the existing single-flight memo: one computes, the
  other joins, exactly as two concurrent callers do today. The batch
  adds no second coalescing layer.
- **Determinism.** Answers do not depend on the schedule: each root's
  outcome is what its single-root call returns, and outcomes are
  returned in input order. The fingerprint check the benchmark runs
  across caller counts must hold across worker counts for a batch too.
- **Cancellation.** The batch token is installed as each item's request
  cancellation; items not yet started are skipped and report
  `Cancelled`.
- **Admission.** Items are submitted once per batch, with the batch's
  priority, through the coordinator's submission accounting.

## Benchmark

`scheduler_scaling` gains a batch variant beside the one-caller loop:
one caller submits every witness of the check corpus as one
`check_roots` batch on a fresh host per sample, at host workers
1/2/4/8, and reports wall time, speedup against one worker and CPU
utilisation, as the loop does. The loop stays as the control: the batch
is worth landing when its speedup at 4 and 8 workers approaches the
`full_check` section's (currently about 2.9× at 4 workers) with the same
fingerprints, and the loop's single-caller numbers do not regress.

## Prerequisite

A batch's items are concurrent callers of one host, so a batch inherits
that host's shared locks. The contention probe
(`signature_kernel_contention_probe`) found three and they are fixed
(the audit records store's eviction, the warm memo read's second lock
turn, the retention ledger's trim); the full check still flattens past
four workers on the semantic memo's single `entries` mutex, which every
cold publish takes. The batch API should follow sharding that map
([`warm-query-concurrency.md`](warm-query-concurrency.md)).
