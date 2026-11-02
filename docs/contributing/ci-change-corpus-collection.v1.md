# CI change corpus v1: collection and replay

Read with the [scenario definitions](ci-change-corpus.v1.md). Results belong to the experiment that collects them, not to this repository. This protocol does not authorize live submissions, buy capacity or change publication permissions.

Compare the same versioned scenarios before and after. Historical point durations, inventory timeout limits, estimated lane cost, local fixture runtime and unmatched hosted runs are not the measured baseline.

## Local replay

Run from the root of the tree under test, one directory per arm and repetition. A directory that already holds a run is refused.

```sh
node scripts/ci-change-corpus.mjs replay --out <results>/baseline-1 --arm baseline --repetition 1 --corpus <archived-scenarios.json>
node scripts/ci-change-corpus.mjs replay --out <results>/candidate-1 --arm candidate --repetition 1 --corpus <archived-scenarios.json>
node scripts/ci-change-corpus.mjs compare --baseline <results>/baseline-1 --candidate <results>/candidate-1 --out <results>/comparison-1.json
```

- `replay` writes `results.json` plus one TAP log per fixture scenario. It exits 1 on a violated expectation, an effective-plant failure, or a fixture that did not really run. Its `notRun` and `unavailable` lists name what it did not answer, and why.
- `compare` exits 1 when the arms read different corpus bytes or definitions, when one arm lacks a sample, or when the candidate violates an expectation. It reports selection changes without judging them; a narrowing inside the floor is a difference to explain, not a failure.
- Pass `--metadata <file>` to reuse one captured `cargo metadata --format-version=1 --all-features --locked` document. Without it, the replay reads the tree's own graph, which is what CI does.

A manifest records the corpus and definition digests, arm, repetition, head, tree, whether the tree was dirty, the crate-graph digest, Node version, platform, architecture, and digests of the workflows, selection code, build configuration and lockfiles that decide an answer. A dirty tree is recorded, not refused, so a local trial stays possible; a reported comparison uses clean trees.

### Failing changes

Plants change files for the length of one obligation, so run them in a clean disposable worktree. A dirty tree is refused.

```sh
node scripts/ci-change-corpus.mjs plant PL01 --worktree <disposable-worktree>
node scripts/ci-change-corpus.mjs plant PL02 --worktree <disposable-worktree>
node scripts/ci-change-corpus.mjs plant PL03 --worktree <disposable-worktree>
pnpm install --frozen-lockfile  # once per worktree, before PL04
node scripts/ci-change-corpus.mjs plant PL04 --worktree <disposable-worktree>
```

The verdicts are `failed-as-expected` (exit 0, the rejection the scenario wants), or `plant-not-applied`, `obligation-passed`, `failed-for-another-reason` and `dirty-tree` (all exit 1). Retain the output.

### Existing authorities, directly

```sh
node --test scripts/ci-impact.test.mjs scripts/lib/crate-graph.spec.mjs
node --test scripts/check-ci-aggregate.test.mjs scripts/release-lanes.spec.mjs scripts/release-proof.spec.mjs
node --test scripts/ci-junit-metadata.test.mjs scripts/perturbation.test.mjs
node scripts/check-ci-aggregate.mjs
node scripts/ci-impact.mjs --audit
```

To answer one array outside the corpus, or a real patch, use the command `detect-changes` runs:

```sh
node scripts/ci-impact.mjs --changed-files changed-files.json --github-output selection.txt --summary summary.md
node scripts/ci-impact.mjs --range <base>..<head>
```

### Test identities and run configuration

The selected build and test command comes from the captured workflow, job and step with its expanded matrix, including environment and working directory. Record it verbatim; do not rebuild it from a lane label. Reuse the source's JUnit metadata steps in the step that runs nextest, with the actual argument vector in place of the placeholder:

```sh
node scripts/ci-junit-metadata.mjs snapshot-env --out env.json
node scripts/ci-junit-metadata.mjs write --junit junit.xml --out metadata.json --env-snapshot env.json --profile ci --argv-json '<cargo argv as a JSON array>'
node scripts/ci-junit-metadata.mjs check --junit junit.xml --metadata metadata.json
```

`check` requires the nextest step's own environment and tools, so it fails outside a real test step. Never dump credentials or the whole environment. Keep semantic test IDs, answer classes, selected obligation sets, compile-unit and cache counts where exposed, and the actual failure and cancellation dispositions. Test counts alone do not show equivalence.

## Authorized hosted collection

The inventory records live access as unavailable. With read access or equivalent exports from the telemetry owner, these read-only commands collect raw records. Bind `REPO`, `RUN_ID`, `ATTEMPT`, `PR_NUMBER` and `HEAD` to the observed run; `HEAD` is a runtime identity of the tested head.

```sh
gh api --paginate --slurp "repos/$REPO/actions/runs?head_sha=$HEAD&per_page=100" > head-runs.json
gh api "repos/$REPO/actions/runs/$RUN_ID" > run.json
gh api --paginate --slurp "repos/$REPO/actions/runs/$RUN_ID/attempts/$ATTEMPT/jobs?per_page=100" > jobs-attempt-$ATTEMPT.json
gh api --paginate --slurp "repos/$REPO/actions/runs/$RUN_ID/artifacts?per_page=100" > artifacts.json
gh run view "$RUN_ID" --repo "$REPO" --log > run.log
gh api "repos/$REPO/pulls/$PR_NUMBER" > pull.json
```

- **Enumerate every run.** Repeat the jobs command for every attempt through `run_attempt`, and collect every relevant workflow for each pull request and head, not only the first successful `ci.yml` run. Reruns and overlapping workflows must not be deduplicated by display name.
- **Keep the failures.** Collect creation, start, completion, result and cancellation timestamps for failed and canceled attempts too.
- **Keep artifacts intact.** Download existing JUnit and metadata artifacts under their inventory names, preserve their bytes, and validate them with `check`. Note retention expiry and missing records.
- **Access failures are evidence.** Retain a 401 or 403 and mark that telemetry `unavailable`. It never means that no runs existed.

Run and job timestamps cannot supply source push time, fleet admission, ready time, final-slot ownership or every merge event. Request timestamped exports from their owners, with the clock source and event IDs. A pull request's `updated_at` is not ready time. `merged_at` is landing time only once the landed tree is confirmed as the tree the required obligations proved.

Commands for submitting a live stream depend on the fleet admission contract the inventory records as unavailable. Do not invent an acquire or release API, or use privileged workflow dispatch as a substitute; consumers record the exact commands when that seam exists.

## Metrics

Timestamps are UTC with recorded precision. Durations are in seconds and runner consumption is in minutes. Keep raw timestamps so an aggregate can be recomputed. Invalid order, a missing field or uncertainty across clocks gives `unavailable` or `inconclusive`, never zero. Define the observation window before replay and include its outstanding work at close.

| Metric                | Definition                                                                                                                                                                                                                                                                                                                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Job queue             | `started_at - eligible_at`, where eligible means dependencies are terminal and the scheduler accepts the job. With only run `created_at`, report `started_at - created_at` as dispatch-to-start, which includes dependency wait and is not pure queue.                                                                                                                                                    |
| Job execution         | `completed_at - started_at` per job and attempt, including failed or canceled work until it stops. A skipped job consumes no observed runner interval, but a skipped required obligation is incomplete coverage.                                                                                                                                                                                          |
| Push-to-green         | For a head that was not superseded: the time the exact tree's required verdict completed, across all required workflows, minus the source push event. An optional lane finishing is not green. The first complete success is the green event; failed attempts still consume resources.                                                                                                                    |
| Queue/execution split | Build the required-job graph from eligibility, start and completion events, and attribute the longest causal path from push to verdict to pre-admission wait, dependency wait, runnable queue and execution. The parts sum to push-to-green; concurrent job durations are not added into wall time. Without eligibility events, report execution plus dispatch-to-start and mark the split `unavailable`. |
| PR peak runners       | The maximum number of active runner intervals `[start, completion)` across all workflows and attempts for that pull request, including superseded heads. At equal timestamps, process completions before starts.                                                                                                                                                                                          |
| PR peak runner share  | At each event boundary, the pull request's active runners over all active runners in the observed account pool; take the maximum where the denominator is non-zero. Record the pool's identity. Without account-wide telemetry, report corpus-only share and label it so. Report peak over allocated capacity only when that capacity is known.                                                           |
| Runner-minutes        | The sum of observed job execution seconds over 60, per pull request and per scenario, including retries, failed plants and canceled superseded work. Keep elapsed minutes distinct from billed minutes and OS multipliers; paid spend needs billing evidence.                                                                                                                                             |
| Ready-to-landed       | Landing time minus the moment the exact head first met the ready contract for landing admission, including final-slot queue, rebase, revalidation and merge waits. A changed head starts a new sample and keeps its predecessor's work. No ready event or no landing is `unavailable` or censored, never zero.                                                                                            |
| Landings/day          | Exact-tree proven landings in the declared window, divided by the window's length in days of 86,400 seconds. Report drain time as a separate batch-throughput figure; do not shorten the denominator to the span between the first and last success.                                                                                                                                                      |
| Work comparison       | Paired answer classes, obligation and test identities, compile-unit counts, cache hit and miss counts, and admission and bypass counts where exposed. Compare candidate over baseline with explicit denominators. A zero baseline makes the ratio undefined; report absolute counts instead.                                                                                                              |

Compute p50 and p95 by nearest rank: sort the eligible samples and take the one-based index `ceil(p * n)`. Report n, failures, supersessions, missing or censored samples and unavailable cells beside each percentile, so a success-only percentile cannot hide unsuccessful heads. Do not average per-cell percentiles into a corpus percentile; pool the raw samples under a declared weighting and also publish each cell. Parallel runner-minutes and serial elapsed time answer different questions.

## Paired experiment and report

1. **Freeze the inputs.** Before collecting, archive the scenario file and these documents with the expanded obligation matrix, source digests and command vectors. Record machine, toolchain and configuration for both arms. Baseline and candidate tree identities belong in the experiment's report, not in this repository.
2. **Repeat, paired and interleaved.** For each available scenario, cell and cache condition, run five paired repetitions on the same worker in the same session: baseline first on odd repetitions, candidate first on even ones. Five is a reproducibility policy, not a statistical claim, and p95 at n=5 is the maximum. Never change the population after seeing durations. Keep warm-up producers out of the scored warm runs, and report their cost separately.
3. **Replay each stream.** Replay `CC01` and `CC02` five times in the same signal order. Record an unavailable seam rather than substituting serial fixture timings, and never use sleeps to manufacture queue contention. When hosted scheduling cannot keep both arms on one worker and session, label the timing comparison observational; answers and work counts still hold.
4. **Compare selected with full results.** Run both on equivalent inputs and compatible cache state through the consumer's authorized validation lane, keeping every platform and feature exclusion. A local worker turn does not run the full gate, and selection answers alone cannot show equivalence with the full suite.
5. **Report each cell.** Give each arm's answers and work counts, queue, execution, green and landing metrics, resource consumption, failures and missing evidence. Report concurrent progress and supersession separately. Report a landing-latency regression as a measured trade-off for the operator.
6. **Retain the evidence with the results.** Keep raw run, job and event records, configuration bytes or digests, patches and changed-file inputs, argument and environment metadata, cache and artifact provenance, every repetition, every error and the calculation method.

## How results are judged

Under the standing measurement rule, a task's performance acceptance is equivalent answers and work counts or growth ratios against the baseline, with both arms measured on the same worker in the same session, interleaved. Timing, allocation and memory comparisons are reported as evidence and never gated.

A metric, control or cell that is unavailable on the assigned worker is recorded as `unavailable`, with the reason, and the work continues. Unavailable evidence does not fail this corpus; it does prevent a live success claim. Numeric targets stay open until their owner selects them from measured evidence, and open targets mean the overall CI success contract cannot be claimed complete.
