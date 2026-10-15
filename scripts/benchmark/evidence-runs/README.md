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
edits it. Shipped: `skr-perf0-structural`, `skr-mapped-indexb`, `switch-discriminated`,
`skr-callback-cost` (10, 100 and 500 contravariant callbacks; cold Verter time
and peak memory beside whole-program TypeScript wall time),
`skr-augment-scaleb` (a global interface merged from 2,000 script contributors
and a module interface augmented by 2,000 unimported modules; cold Verter time
and peak memory), `skr-scale-answers`.

`skr-mapped-indexb` demands conditional mapped values through the entire source
key domain at 1,000, 3,000 and 10,000 keys. The recorded TypeScript 7.0.2 answer
is `"b" | "n" | "s"`. Its strict, cold Verter cells require `firstTypeMs` and
`peakBytes`; the fleet measures those cells on the benchmark worker.

`skr-scale-answers` demands the recursive `Names` and `Paths` conditionals over
an `as const` route tree of 100 and 300 parents (five children each), a repeated
`infer` pattern over a 100- and a 1,000-member union, and an inference through
100 and 1,000 distinct aliases of one generic. TypeScript 7.0.2 answers every
one with the full literal union and no diagnostic. Its required cells are the
largest sizes: cold Verter `coldMs` and `peakBytes` beside whole-program
`tsc-cli` `wallMs`; the middle sizes run beside them for comparison. The
run also demands the `Names` of a 2,000-parent tree (12,000 names, every child
name included) and the repeated `infer` over a 6,000-member union (6,000
numbers). Those two are answer obligations, not timing cells: the run sets
`requireAllMatched`, so any Verter answer in it that does not match the
measured TypeScript answer exactly (wrong, partial, refused or killed) fails
the run.

| Field                             | Meaning                                                                                                                                                                                                  |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                            | the run name; equals the file stem (lower-case letters, digits, hyphens)                                                                                                                                 |
| `description`                     | what the run is for, in one sentence                                                                                                                                                                     |
| `tier`                            | `quick`, `standard` or `stress`: default scenario set, deadline and repetition counts                                                                                                                    |
| `scenarios`                       | scenario ids or prefixes from the catalog (`scenarios.mjs`), as `--only` takes them; omitted means the tier's set                                                                                        |
| `settings`                        | setting ids (`strict`, `snc-off`, `nia-off`, `both-off`); the harness runs `strict` alone or all four, so any other subset is rejected; omitted means `strict`                                           |
| `arms`                            | arm ids from `ARMS` in `semantic-perf/analyze.mjs`; unknown ids are rejected                                                                                                                             |
| `modes`                           | `cold` (fresh-process demands) and `warm` (in-process repeats); a whole-program `tsc -p` arm has only `cold`                                                                                             |
| `threads`                         | `"default"` (the tool's own choice) or `1` (`tsc-cli-1`); the harness takes no thread option, so any other count for an arm is rejected                                                                  |
| `repeat`, `warmup`, `warmRepeats` | measured invocations, unmeasured warmups, in-process warm repeats per cell; omitted means the tier's defaults. `repeat` takes any count from 2 up                                                        |
| `requireAllMatched`               | `true` forwards `--require-all-matched` to `validate.mjs`: every Verter answer of the run must match the measured reference, or the run fails; omitted means `false` (a non-matched answer is a finding) |
| `noise`                           | link to a section of the semantic-benchmark page defining the noise methodology                                                                                                                          |
| `requiredCells`                   | cells the summary must contain: scenario, setting, arm, mode, thread count and required metrics                                                                                                          |

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
