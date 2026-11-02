# Current CI baseline v1

**Status: no measured baseline exists.** This document is reproducible missing-data evidence, not a baseline. It satisfies no before/after comparison, does not confirm the provisional 12-minute push-to-green p50 or the six-runner limit, and supplies no spend figure. Budget declarations, target confirmation, replay calibration, build-sharing measurement and any CI success claim stay held until a collection reports `measured`.

- **Driver:** [`scripts/ci-baseline.mjs`](https://github.com/pikax/verter/blob/main/scripts/ci-baseline.mjs), with its [tests](https://github.com/pikax/verter/blob/main/scripts/ci-baseline.test.mjs) in `test:scripts:ci`.
- **Inputs and metric definitions:** the [change corpus](ci-change-corpus.v1.md) and its [collection protocol](ci-change-corpus-collection.v1.md), read only. Percentiles, runner intervals and windows mean what the protocol says.

Results, timings and raw records never enter this repository. Each collection keeps them in its own directory and reports them to whoever ordered it.

## What a baseline needs

`report` says `measured` only when all of these hold. Anything else is `missing-data`, whatever the window's runs look like.

- Every telemetry family below is complete, with no denied, partial or skipped read.
- Every corpus `change` sample has five repetitions bound to pull requests whose changed files exactly equal the sample's. The rows the versioned corpus itself rules out stay listed under `unavailableByCorpus`: `CC01` and `CC02`, which name their missing seam, and `CH17/landed-release-commit`, because no version may be published to collect one. A row with some bindings gets no exemption. A collector cannot excuse a row of its own.
- A live required context was read.
- Paid spend, hosted allocation, ready-to-landed and exact-tree-proven landings/day all have records.

## Access attempts

Read-only, on 2026-11-02. Each one reproduces with the command shown; `collect` retains the same receipt for every read.

| Read                                                                                                      | Result                                         | Consequence                                                                |
| --------------------------------------------------------------------------------------------------------- | ---------------------------------------------- | -------------------------------------------------------------------------- |
| `gh api --method GET --paginate --slurp repos/pikax/verter/actions/runs?created=<since>..<until>`         | HTTP 401, `Bad credentials`                    | Worker jobs hold no read token. This is how the job is set up, not a fault |
| The same `gh api` read of `.../rules/branches/main`, `.../activity`, `users/pikax/settings/billing/usage` | HTTP 401, `Bad credentials`                    | As above                                                                   |
| Anonymous `GET` of any read, first attempt                                                                | HTTP 403, rate limit exceeded                  | The 60 reads an hour per address were already spent by other clients       |
| Anonymous `GET .../actions/runs?created=<since>..<until>&per_page=100`                                    | HTTP 200                                       | Run records are public                                                     |
| Anonymous `GET .../actions/runs/<id>/attempts/<n>/jobs`                                                   | HTTP 200 for 18 runs, then HTTP 403 rate limit | Job records are public; 60 reads cannot cover one busy day                 |
| Anonymous `GET .../activity?per_page=100`                                                                 | HTTP 200                                       | Source push timestamps are public                                          |
| Anonymous `GET .../rules/branches/main`                                                                   | HTTP 200, naming no required status check      | The live required contexts remain unknown                                  |
| Anonymous `GET users/pikax/settings/billing/usage`                                                        | HTTP 401, `Requires authentication`            | Spend needs the account owner                                              |

To repeat them, with a directory per attempt:

```sh
node scripts/ci-baseline.mjs collect --out <dir> --repo pikax/verter --since <iso> --until <iso> --landing-ref origin/main
node scripts/ci-baseline.mjs collect --out <dir> --repo pikax/verter --since <iso> --until <iso> --landing-ref origin/main --anonymous --max-requests 50
node scripts/ci-baseline.mjs report --in <dir> --out <report.json>
```

A denied or rate-limited read is the result, so `collect` exits 0 once its receipts exist. It never retries or waits out a limit, and it stops reading at the first rate limit. `report --require-measured` exits 1 on missing data, which is how a comparison should refuse it.

## Metric availability today

| Metric                                 | Source                                      | Today                                                                                | Who can close it                                 |
| -------------------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------ |
| Push-to-green p50/p95                  | Push activity, job records                  | Observational only: public, but no run is bound to a corpus scenario                 | Whoever may submit the corpus changes            |
| Queue and execution, separately        | Job records                                 | As above, and the split is computed: see below                                       | As above                                         |
| Peak runners and runner-minutes per PR | Job records                                 | As above. Elapsed minutes, not billed minutes                                        | As above                                         |
| Peak share of the runner pool          | Job records of every repository in the pool | Repository pool only, labelled as such                                               | Account owner, for account-wide telemetry        |
| Ready-to-landed p50/p95                | Ready and landing events                    | Unavailable. A pull request's `updated_at` is not ready time                         | Landing controller owner, by `ready-events.json` |
| Landings/day                           | Exact-tree-proven landing events            | Observational: first-parent commits or `merged_at`, with no proof of the landed tree | Landing controller owner, by `ready-events.json` |
| Paid spend                             | Billing usage                               | Unavailable: HTTP 401                                                                | Account billing owner                            |
| Hosted allocation                      | Account settings                            | Unavailable: no read endpoint is collected, and nothing records it                   | Account billing owner, by `allocation.json`      |
| Required contexts                      | Repository rules, branch protection         | `CI Required` from the workflow source; the readable rules name none                 | Repository admin                                 |
| Cold and warm cells                    | The job that restores each cache            | `unknown` per cell, as the corpus requires                                           | Cache measurement work                           |

Scenario runs are the decisive gap. Unmatched window runs are not a baseline under the protocol, and collecting scenario runs means opening pull requests that carry the corpus changes. That is outward-facing hosted execution a worker job cannot do and nobody has authorized here.

## What the public records settle

The driver was run against real records as well as fixtures, and three facts changed its design:

- **A job's `created_at` is its eligibility.** In the sampled records every dependent job's `created_at` equals its dependency's `completed_at`, to the API's one-second resolution. So `started_at - created_at` is runner wait without dependency wait, and walking back from the verdict job gives the protocol's split into pre-admission, dependency wait, runnable queue and execution, which sum to push-to-green. GitHub does not document this meaning; a collection should recheck it, and a record without the timestamps gets no split.
- **`pull_requests` is often empty on a pull request's own runs.** Streams therefore follow the head branch, and a push-triggered workflow on a pull request's branch counts as that pull request's work.
- **Most runs in a window are wholly skipped `issue_comment` runs.** They hold no runner, so their jobs are not read.

The inventory's unavailable run telemetry describes the job's token. It remains correct about spend, allocation and admission, which stay unreadable.

## Separate populations

Grouping by workflow name would compare unlike things, so `report` keeps these apart:

| Population            | Meaning                                                                                                                       |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `pull-request`        | Every workflow on a pull request's head branch                                                                                |
| `push-executed`       | A `ci.yml` push where a lane held a runner, or which did not succeed                                                          |
| `push-no-lanes`       | A successful `ci.yml` push where only `detect-changes` and `CI Required` ran: a landed push reusing its proof looks like this |
| `push-other-workflow` | A push to another workflow, such as tagging                                                                                   |
| `push-unclassified`   | A `ci.yml` push whose jobs were not read                                                                                      |
| `other`               | Schedules, comments and the like                                                                                              |

All of these are `observational`. Only runs bound to a corpus sample report as `measured`. Provider test timings are excluded by construction: they time streamed test execution alone and cannot stand in for any of these metrics.

## Owner exports

`collect --export <dir>` copies these verbatim and answers their family without an API call. A missing file is a receipt naming its owner.

| File                | Shape                                                                                                              |
| ------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `bindings.json`     | `[{scenario, sample, repetition, pullRequest}]`, plus `[{stream, repetition, pullRequests: {<role>: <number>}}]`   |
| `ready-events.json` | `[{pullRequest, head, readyAt, landedAt, exactTreeProven}]`. A record without `exactTreeProven` counts as censored |
| `allocation.json`   | The owner's record of hosted capacity, kept as given                                                               |
| `billing.json`      | The usage report's shape, `{usageItems: [{product, netAmount}]}`                                                   |
| `push-events.json`  | `[{after, timestamp}]`, when activity is not readable                                                              |

## Limitations

- **Observed, not specified.** The eligibility reading of `created_at` and the walk back through it come from sampled records. A chain broken by a gap over two seconds attributes the rest to pre-admission, and a rerun's copied jobs can do the same.
- **Bounded reads.** The runs API returns at most 1,000 results per query, so a busy window needs narrower windows. Activity retention is finite. The report states when a window was read in part.
- **Shared streams.** A branch reused by two pull requests reads as one stream.
- **Partial pools.** With job records for only some runs, peak share and peak runners are lower bounds. The report names how many runs were read.
- **Clocks.** All timestamps come from GitHub at one-second resolution, so very short intervals round down.
- **Local plants and fixtures** stay in the corpus replay. Nothing here shows they would behave the same in a hosted run.

## Why each mechanism exists

| Mechanism                                  | Concern     | Prevents                                                                                 | Not covered because                                                     |
| ------------------------------------------ | ----------- | ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Receipts for every read, including denials | process     | Certain confusion of "could not read" with "nothing happened", or with a fast CI         | Run and job records say nothing about reads that failed                 |
| `measured` only with bound repetitions     | correctness | Likely promotion of window runs to a baseline, which the protocol forbids                | Nothing else relates hosted runs to corpus scenarios                    |
| Changed-file check on each binding         | correctness | A wrong pull request silently measuring another change                                   | A binding is hand-supplied input                                        |
| Absent values instead of zeros             | correctness | An unobserved push or clock inversion reading as instant, certain with naive subtraction | Job success says nothing about timing validity                          |
| Separate push populations                  | performance | Proof-only runs flattering the gate, likely when grouped by workflow name                | The landed fast path optimizes execution and does not label its samples |
| Critical-path split from eligibility       | performance | Attributing runner contention to slow jobs, or summing parallel jobs into wall time      | Job durations alone cannot separate queue from dependency wait          |
