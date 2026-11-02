# Queued cancellation and slot behaviour

Observed 2026-11-02 from public `pikax/verter` Actions records. **Job-level slot behaviour is still unmeasured, so slot adoption stays held.** No workflow here declares a job-level `concurrency` group, and this research has no trial environment in which to run one.

What the records do show is workflow-level supersession. Every cancelled run sampled ended with a failing `CI Required`, and none read as passing. They also show one hazard that a slot layout would inherit: re-running an older run can cancel the newest push.

| Contract clause                                            | Result                                                                                                   |
| ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| A cancelled run's aggregate is not read as passing         | Held in every sampled run, at workflow level only                                                        |
| The latest run's aggregate reflects only its own jobs      | Held per attempt, but "latest" is unsafe unless keyed to the tested tree ([PR 1080](#older-tree-re-run)) |
| A cancelled or pending **slot** job is not read as passing | **Unmeasured**: no slot job exists to observe                                                            |
| Static slots leave acceptable idle capacity                | **Unmeasured**: no slot occupancy or account capacity records                                            |

This supports neither `slot-groups-safe` nor `supersession-only`, because both are claims about slot jobs. The recorded result is an undeclared discovery.

## Access

| Operation                                      | Result                                                                          |
| ---------------------------------------------- | ------------------------------------------------------------------------------- |
| `gh api repos/pikax/verter/actions/runs`       | HTTP 401 `Bad credentials`: the job is deliberately kept from writing to GitHub |
| Unauthenticated `GET` of runs, jobs and checks | HTTP 200: the repository is public, limited to 60 requests an hour per address  |
| Dispatching, cancelling or pushing a trial     | Not attempted: no authorised trial repository or credential was supplied        |

The candidate branch was not used as a trial environment. A trial workflow on it would run on production runners without authority, and could not produce controlled superseding pushes.

## Observed runs

Reproduce with the read-only driver. It exits non-zero if any record is unreadable or any aggregate misreports its own jobs:

```sh
node scripts/ci-concurrency-observe.mjs --repo pikax/verter --annotations \
  --run 40324254479 --run 40327765458 --run 40328263341 \
  --run 41349220820 --run 41286541386@1 --run 41286541386@2 --run 41286541386@3 \
  --run 40276061212 --run 40277406274
```

"Cancelled, ran" means the job held a runner when it was cancelled. "Cancelled, no runner" means it never reached one. Peak is the most jobs running at once in that attempt.

| Run and attempt | Pull request | Run conclusion | `CI Required` | Cancelled, ran | Cancelled, no runner | Peak | Cancel cause (group)      |
| --------------- | ------------ | -------------- | ------------- | -------------- | -------------------- | ---- | ------------------------- |
| 40324254479 #1  | 1032         | cancelled      | failure       | 2              | 0                    | 17   | `ci-refs/pull/1032/merge` |
| 40327765458 #1  | 1032         | cancelled      | failure       | 1              | 33                   | 1    | `ci-refs/pull/1032/merge` |
| 40328263341 #1  | 1032         | success        | success       | 0              | 0                    | 20   | none                      |
| 40276061212 #1  | 1030         | cancelled      | failure       | 1              | 29                   | 1    | `ci-refs/pull/1030/merge` |
| 40277406274 #1  | other ref    | cancelled      | failure       | 3              | 16                   | 6    | not read                  |
| 41286541386 #1  | 1080         | failure        | failure       | 0              | 0                    | 17   | none                      |
| 41286541386 #2  | 1080         | failure        | failure       | 0              | 0                    | 17   | none                      |
| 41286541386 #3  | 1080         | success        | success       | 0              | 0                    | 17   | none                      |
| 41349220820 #1  | 1080         | cancelled      | failure       | 0              | 34                   | 1    | `ci-refs/pull/1080/merge` |

The driver found no verdict violations in these attempts. Two other cancelled runs agree: 40098863892, a pull request run, and 41362800277, a push to `main`, both ended with a failing `CI Required`. This is a sample of the 333 cancelled pull request runs on record, not all of them, and none of it involves a job-level queue.

### Superseded pushes on one ref

Pull request 1032 went through three pushes in a row, covering both running and waiting jobs:

| Time (UTC, 2026-10-26) | Transition                                                                                |
| ---------------------- | ----------------------------------------------------------------------------------------- |
| 12:39:47               | First run created                                                                         |
| 13:06:32               | Second run created; the first still has two jobs running                                  |
| 13:07:39 to 13:07:44   | The first run's two running jobs end cancelled                                            |
| 13:09:38 to 13:09:41   | The first run's aggregate runs on a runner and fails: "at least one CI job was cancelled" |
| 13:09:42               | First run ends cancelled                                                                  |
| 13:09:45               | Second run's first job starts, 3m13s after the run was created                            |
| 13:10:11               | Third run created                                                                         |
| 13:10:25               | Second run ends its running job and its 33 waiting jobs as cancelled, not skipped         |
| 13:10:28 to 13:10:32   | Second run's aggregate fails, then the run ends cancelled                                 |
| 13:49:10               | Third run's aggregate succeeds                                                            |

Three things follow from these records:

- Each head's `CI Required` check reads its own run: failure, failure, success.
- A superseded run still takes a runner for its aggregate, because `if: always()` runs it after cancellation.
- Supersession is not instant. The newer run waited for the older run to finish, including that aggregate.

The waiting jobs here were waiting on `needs`, and the no-runner job in 41349220820 had not yet got a runner. Neither is a wait in a job-level concurrency queue, so these records do not predict how a slot queue behaves.

### Distinct-ref control

Pull request 1030's older run was cancelled at 06:05:23 on 2026-10-26. Run 40277406274, on a different ref, had one job running across that moment and started 23 more after it. Per-ref supersession did not reach the other ref. This is one control, and that run's own later cancellation cause was not read.

### Older-tree re-run

Pull request 1080 shows supersession running backwards, on 2026-11-01:

| Time (UTC) | Transition                                                                                                                               |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| 20:54:20   | Run 41349220820 created for the pull request's current head                                                                              |
| 20:54:21   | Attempt 3 of run 41286541386 starts, on an older head of the same pull request                                                           |
| 20:54:23   | The newest run's `detect-changes` is cancelled before reaching a runner: "higher priority waiting request for `ci-refs/pull/1080/merge`" |
| 20:54:28   | The newest run's `CI Required` fails                                                                                                     |
| 21:00:03   | The older head's `CI Required` succeeds                                                                                                  |

The API does not name the request that displaced a run. Attributing this one to attempt 3 is an inference from its timing and from it being the only other CI run active on that ref. The driver reports it as a candidate.

The check on the current head correctly reads failure, and the pull request was still open when observed, so nothing wrong landed. The hazard is in how results get read:

- **Keyed to the tested head:** reads failure, which is correct.
- **Keyed to the pull request or branch, taking the latest completed run:** reads success, from a tree that is no longer the head.

The current head is also left with no complete result until someone pushes or re-runs it. Any verdict reader has to key on the tested tree and the run attempt, and a re-run of a superseded head can stop current work.

### Other observations

- **`main` pushes supersede each other too.** The group is `ci-${{ github.ref }}`, so a landing can cancel the previous landing's run, which leaves that commit with a failing `CI Required` and no complete post-merge result. Run 41362800277 ended this way, though its cancel cause was not read.
- **Per-run demand is well above one slot.** Complete runs peaked at 17 to 20 running jobs. This is demand only. It says nothing about account capacity or idle slots.

## Documented queue rules

These come from GitHub's [concurrency documentation](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency), read 2026-11-02. They are documented behaviour, not measurements, and they shape what a trial has to test:

- A group has at most one running job. With the default `queue: single` it has at most one pending job, and a new arrival cancels and replaces the existing pending one.
- `queue: max` allows up to 100 pending jobs, handled first in, first out, though the documentation says ordering is not guaranteed.
- `queue: max` with `cancel-in-progress: true` is a workflow validation error.

Read against the current aggregate, which treats a cancelled dependency as failure, the default queue suggests a specific risk. Three pull requests contending for one slot could cancel the middle one's job, turning a run nobody superseded red. That would be a false failure and unfair progress rather than a false pass, and neither declared outcome covers it. It needs measuring, as does whether `queue: max` jobs are released promptly when their run is superseded.

## Trial needed to settle this

The repository maintainer must supply an isolated trial repository and a token for its run records, and the account owner must confirm its capacity. Production CI and required-check policy stay out of it. The layout under test, which has not been run:

```yaml
concurrency: # workflow level: per-ref supersession, unchanged
  group: trial-${{ github.ref }}
  cancel-in-progress: true
jobs:
  heavy:
    concurrency: # job level: the slot under test
      group: heavy-slot-1
      queue: max # repeat with the default, `single`
```

Coordinate jobs with an explicit release signal. Deadlines may only guard against hangs, and no fixed sleep or elapsed time may establish a transition.

1. **Superseded while waiting.** Occupy the slot, confirm a second job is waiting for it, then supersede that ref. Record both runs' terminal states and the aggregate's. Repeat with the job running.
2. **Contention across refs.** Have three refs contend for one slot under each queue setting, with no supersession, and record whether any job is displaced and what its aggregate reads. Keep a distinct-ref control.
3. **Older-tree re-run.** Re-run a superseded head's attempt while the newest run waits, and check which verdict each key reads.
4. **Idle capacity.** Compare contended static slots with an available-slot control under the same workload. Retain occupancy, runnable waiting demand and unused capacity together. A job waiting alone does not prove capacity is idle.
5. **Evidence.** Capture every run with `node scripts/ci-concurrency-observe.mjs --repo <trial> --token-env <NAME> --annotations --run ...`, polling while transitions are live. A record that cannot be read stays missing and never counts as success.

`slot-groups-safe` needs the whole contract observed. A demonstrated false pass or unacceptable idling supports `supersession-only`. What counts as unacceptable idling is the operator's call, once measurements exist.

## Disposition

Slot adoption stays held, and per-ref supersession remains the only configured concurrency. This research does not establish a cross-PR throttle, fair progress, or full-before-merge delivery, and it changes no production workflow or required-check policy.
