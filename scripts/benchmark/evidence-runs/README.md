# Evidence runs

An evidence run is a named, repeatable invocation of the semantic benchmark
harness (`semantic-perf.mjs`), described by one manifest in this directory.
`scripts/benchmark/evidence-run.mjs` is a thin wrapper: it validates the
manifest, drives the harness with the manifest's options, validates the result
with the harness's own `validate.mjs`, and writes one machine-readable summary.
It adds no second harness, classifier or validator. Noise and verdict
methodology: `docs/contributing/semantic-benchmark.md#verdicts` and `#schedule`.

```bash
node scripts/benchmark/evidence-run.mjs --run <name> --dry-run   # validate the manifest, emit a summary skeleton
node scripts/benchmark/evidence-run.mjs --run <name>             # execute it
node scripts/benchmark/evidence-run.mjs --run <name> --worker <record.json> [--allow-sampled]
```

`--worker` takes the worker record the Tama evidence job supplies: a JSON
object with a string `id` and a list of string `tags` (further identity fields
are kept verbatim in the summary). `--allow-sampled` and `--supervisor <path>`
pass through to the harness. Exit status: 0 passed (or a valid dry run), 1 the
run failed, 2 a usage or manifest error. `--dry-run` runs nothing and writes
the summary skeleton (every required cell present, none measured) on any
worker. Self-tests: `scripts/benchmark/evidence-run.test.mjs` with planted
manifests under `scripts/benchmark/evidence-run-fixtures/` (`pnpm test:scripts`).

## Manifests

`scripts/benchmark/evidence-runs/<name>.json`. Each run name has exactly one
owner: the change that needs the run adds its manifest and no other change
edits it. Shipped: `skr-perf0-structural`.

| Field                             | Meaning                                                                                                                                                        |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                            | the run name; equals the file stem (lower-case letters, digits, hyphens)                                                                                       |
| `description`                     | what the run is for, in one sentence                                                                                                                           |
| `tier`                            | `quick`, `standard` or `stress`: default scenario set, deadline and repetition counts                                                                          |
| `scenarios`                       | scenario ids or prefixes from the catalog (`scenarios.mjs`), as `--only` takes them; omitted means the tier's set                                              |
| `settings`                        | setting ids (`strict`, `snc-off`, `nia-off`, `both-off`); the harness runs `strict` alone or all four, so any other subset is rejected; omitted means `strict` |
| `arms`                            | arm ids from `ARMS` in `semantic-perf/analyze.mjs`; unknown ids are rejected                                                                                   |
| `modes`                           | `cold` (fresh-process demands) and `warm` (in-process repeats); a whole-program `tsc -p` arm has only `cold`                                                   |
| `threads`                         | `"default"` (the tool's own choice) or `1` (`tsc-cli-1`); the harness takes no thread option, so any other count for an arm is rejected                        |
| `repeat`, `warmup`, `warmRepeats` | measured invocations, unmeasured warmups, in-process warm repeats per cell; omitted means the tier's defaults. `repeat` takes any count from 2 up              |
| `noise`                           | link to a section of the semantic-benchmark page defining the noise methodology                                                                                |
| `requiredCells`                   | cells the summary must contain: scenario, setting, arm, mode, thread count and required metrics                                                                |

Probe-arm `cold` metrics: `firstTypeMs`, `coldMs`, `initMs`, `setupMs`,
`engineStartMs`, `observeMs`, `teardownMs`, `cpuMs`, `peakBytes`,
`retainedBytes`, `observePeakBytes` (`verter-counted` adds `coldAllocations`,
`coldAllocatedBytes`); `warm`: `warmMs`; `tsc -p` arms: `wallMs`, `cpuMs`,
`tscCheckMs`, `tscTotalMs`, `peakBytes`, `tscMemoryUsedBytes`.

The loader rejects a manifest that is not valid JSON, carries an unknown field,
misses a required one, names anything the harness does not define, or lists a
required cell outside the run's own scenarios, settings, arms, modes and
threads. A run whose result lacks a required cell, or whose validation fails,
fails.

## Summary

Each run writes `evidence-summary.json` into its output directory (default
`target/evidence-runs/<name>/<timestamp>`), beside the harness output in
`harness/`. It holds the manifest name and byte digest, whether it was a dry
run, the worker record, the per-cell status (measured, skeleton, or
`unavailable` with a reason) and the harness validation verdict and problems.
