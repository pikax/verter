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
and peak memory, and each reproduction's retention proof: one engine answering
500 then 2,000 contributors against a 2,000-only baseline session),
`skr-scale-answers`, `skr-mapped-expand`, `rev11-l1-soak`.

`skr-mapped-indexb` demands conditional mapped values through the entire source
key domain at 1,000, 3,000 and 10,000 keys. The recorded TypeScript 7.0.2 answer
is `"b" | "n" | "s"`. Its strict, cold Verter cells require `firstTypeMs` and
`peakBytes`; the fleet measures those cells on the benchmark worker. The run
sets `requireAllMatched`, so a Verter answer at any of the three sizes that
does not match the measured TypeScript answer exactly fails the run.

`skr-scale-answers` demands the recursive `Names` and `Paths` conditionals over
an `as const` route tree of 100 and 300 parents (five children each), a repeated
`infer` pattern over a 100- and a 1,000-member union, and an inference through
100 and 1,000 distinct aliases of one generic. TypeScript 7.0.2 answers every
one with the full literal union and no diagnostic. Its required cells are the
largest sizes: cold Verter `coldMs` and `peakBytes` beside whole-program
`tsc-cli` `wallMs`; the middle sizes run beside them for comparison. The
run also demands the `Names` of a 2,000-parent tree (12,000 names, every child
name included) and the repeated `infer` over a 6,000-member union (6,000
numbers). Those two are answer obligations with timing evidence: their Verter
`coldMs` and `peakBytes` and `tsc-cli` `wallMs` cells are required, but no
timing threshold passes or fails the run. The run sets `requireAllMatched`, so
any Verter answer in it that does not match the measured TypeScript answer
exactly (wrong, partial, refused or killed) fails the run.

`skr-mapped-expand` demands homomorphic and remapped mapped types over
concrete sources: the keys of a class built from 10, 25 and 40 mixins, the
intersection a builder accumulates over 50, 200 and 500 calls, the 100, 1,000
and 3,000 keys of an imported interface remapped through a recursive template
conditional, and a generic interface heritage chain 10, 40 and 100 levels
deep. TypeScript 7.0.2 answers each with the resolved object type. The run sets
`requireAllMatched`, so every Verter answer must match the measured reference
exactly; its required cells are the largest sizes' cold Verter `coldMs` and
`peakBytes`.

`rev11-l1-soak` is a soak manifest (`"kind": "soak"`): it registers the
long-churn soak runner (`crates/verter_bench/src/soak/`, binary
`verter-soak`) rather than a semantic-perf invocation. The runner exists only
in the declared `soak.build` (`verter_bench` with its default-off
`soak-support` feature, which forwards the engine's flight rendezvous, the
retention account's ACTIVE-occupancy window and the session's test-support
seams for the semantic graph store's joined-wait and query counters, never
`semantic-observe` or audit capture); the driver
builds exactly that configuration and the validator rejects any other. Its
two workloads, `normal` and `pressure`, are 100 tranches of one 100-action
template (10,000 actions, at least 3,000 distinct query identities, every
lifecycle class, both typed refusal classes), frozen by the digests under
`soak.workloads`. A digest covers the action list, the digest of the path
and exact bytes of every publication of every action and of the setup, and
the registered surge shapes' frozen bounds (`soak.surgeLimits`: each shape's
module count, props and oversized props, its largest published input bytes,
the components whose results it holds live at once, its absolute
live-artifact, registered-source and snapshot-lease maxima, and its finite
retained-byte bound — fixed figures a larger request never enlarges; the
full surge's retained bound is the ratified aggregate ceiling). The driver
requires both complete shapes with finite positive integer limits, rejects
requests larger than their declared populations, and checks each retained
bound against the applicable retention policy. The
runner records what it actually published and the verdict fails any action
whose publications differ, so a fixture the freeze omits fails the run. The
runner's test (`cargo nextest run -p verter_bench --features soak-support -E
'test(soak::)'`) regenerates the digests and compares, and runs the
one-tranche smoke manifests (`soak.smoke`) and the negative controls on any
worker. Each mode's verdict comes from the runner's own judgement of its
written summary: action, result and publication inventories, the per-class
answer rules, sampling at least every `sampleEvery` actions, the envelope the
first `envelopeActions` actions set, and the drain to baseline (object
counts, content pins and retained account bytes at the host's close route;
pinned account bytes and store storage once the host is dropped). Overlapping
cold demands are scheduled by the flight rendezvous and judged by one shared
producer, a subscriber registered on its flight and a positive `joined_waits`
delta. Every pressure manifest runs a bounded surge in its first tranche; the
full manifest also freezes surges that hold component-meta results past the
account's aggregate ceiling and entry cap, and its verdict requires positive,
surge-attributed aggregate-pressure and oversized refusal deltas, an answer
delivered complete but not retained, complete delivery of every surge answer,
no ACTIVE refusal, and each full surge's own ACTIVE window reaching the
former advisory active target. `work` cells (`surge*` cells are the surges'
own deltas and window peaks, never lifetime counters) are read on every
worker; wall time, peak RSS and the process-tree peak are `benchM3Only`
cells. A soak manifest carries `name`, `kind`, `description`, `modes`,
`noise`, `soak` and `requiredCells` (`mode`, `metrics`, `benchM3Only`).

Candidate churn records the demanded slot's admission tokens and declared
cap: an old token leaving while a new one enters at the cap proves displacement.
An unchanged aggregate candidate count alone does not. Concurrent refusal
demands report `null` for unmeasured work counters; only measured zero counters
can prove a replay without recomputation. A demand's work is the semantic graph
queries it ran (a retained component-meta replay runs none) and its producer
runs are the graph's cold misses. Paid-context controls also record and
check their equal entry allowance.

| Field                             | Meaning                                                                                                                                                                                                  |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                            | the run name; equals the file stem (lower-case letters, digits, hyphens)                                                                                                                                 |
| `description`                     | what the run is for, in one sentence                                                                                                                                                                     |
| `tier`                            | `quick`, `standard` or `stress`: default scenario set, deadline and repetition counts                                                                                                                    |
| `scenarios`                       | scenario ids or prefixes from the catalog (`scenarios.mjs`), as `--only` takes them; omitted means the tier's set                                                                                        |
| `sessions`                        | exact session ids from the session catalog (`sessions.mjs`), run through `--only`; a growth session's baseline must be listed too; with no `scenarios`, the listed sessions run and no scenario does     |
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
For each session the manifest lists it holds the status (present, missing or
not run), the Verter answer class and, for a growth session, its retention
proof (see `docs/contributing/semantic-benchmark.md#session-workloads`). The
proof's figures are retention-account bytes and object counts, read on every
worker; a missing session, a failed proof or a proof with nothing to compare
fails the run.
