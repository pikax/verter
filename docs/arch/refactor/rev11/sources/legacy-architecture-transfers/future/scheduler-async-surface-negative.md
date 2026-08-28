# Scheduler async surface — negative finding

**Audit verdict (2026-07-22): NEGATIVE.** The scheduler has no production async-future surface that can reproduce this bloat or capacity-multiplication class.

## Symptom

Q1–Q3 of the async state-machine bloat class ask whether the **scheduler**
holds large futures in queues or maps, multiplies them by capacity, or polls a
deep async chain inline. The LSP pass noted the scheduler has no `async fn`
surface; this document confirms that for the whole crate and records it so
the class is not re-hunted there.

## Mechanism

`verter_scheduler` is a **native-thread** job system:

- CPU work: `SchedulerCpuPool` workers (sync closures).
- I/O work: `SchedulerIoPool` workers (sync load closures).
- Host batch coordination: `HostCpuPool` (sync).
- Admission / DAG / singleflight: locks + channels of **job descriptors**,
  not `impl Future` state machines.

There are **zero** production `async fn` definitions, **zero** `.await`
points, and **zero** uses of `FuturesUnordered` / `join_all` /
`buffer_unordered` holding futures in `crates/verter_scheduler/src`.

Comments that say "async staging" mean **asynchronous job progression**
(Source → Analysis → Artifact stages across threads), not Rust `async`/
`.await` futures.

## Reproduction

```text
rg -n "async fn|\.await|FuturesUnordered|join_all|buffer_unordered|Box::pin" \
  crates/verter_scheduler/src --glob "*.rs"
# → no matches for production async machinery
```

## Evidence

| check | result |
|---|---|
| `async fn` in production scheduler modules | **none** |
| Futures held in job queues / maps | **none** (jobs are sync descriptors + completion handles) |
| Capacity × future-size multiplication | **n/a** |
| Poll site for large futures | **n/a** |

## Why deferred

Nothing to fix. Recorded negative for the project-wide async-bloat pass.

## Proposed fix + falsifiable prediction

If a future change introduces `async fn` job bodies held unboxed in a
bounded queue of capacity `C`, measure `C × size_of_val(job_future)` before
landing. Prediction: any such design would re-open the LSP-class memory
finding; prefer spawn-or-box at the queue boundary.

## Blast radius

None today. Leaving the negative undocumented would waste the next pass
on the most-suspected "capacity × size" site.
