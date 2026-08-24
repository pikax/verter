# Compiler route-overhead — measured landing evidence

The charter's exit criterion requires this block's four compile routes to be measured.
This document is that measurement. It records what was run, on which tree, under which
machine conditions, and what came back. It does **not** define, choose, or tune a
threshold, it does not write or amend a `[[cell]]` entry in `performance-gates.toml`,
and it records no acceptance state.

## Why this revision exists: the previous figures were void, and said so

The revision this replaces disqualified its own timings in its own text. It recorded that
its numbers were taken

> on a shared developer machine under concurrent load, not under a locked-machine
> protocol

and that its `latency_ms` and `rss_*` figures were

> **informational only** and must not be read as evidence that the routes are performing
> correctly.

That judgement was correct and is preserved here rather than quietly dropped: **the wall
and RSS figures in every revision of this record before 2026-08-24 are void and must not
be cited.** Their contemporaneous observation — a 1.127–7.282 ms range across forty
leg-runs, a 6x spread on a sub-millisecond operation — is itself the evidence that the
machine, not the code, produced them. What survived from those revisions unaffected is
what was never load-sensitive: the cross-route output digest, the measured work counters,
and the exit status. Those agree with this run.

This revision replaces the void timings with a session taken under the machine protocol
below, and states plainly which conjunct of that protocol this host could not meet.

## Measured tree and binary

| | |
|---|---|
| Commit the binary was built from | `3ae319a2367a35cda0ade86de7e72dec62d8fa16` |
| Binary SHA-256 | `55b0820018cd3e338c0fc34179cbdde3ebaa8e941b87b0be92474f310cad4976` |
| `crates/verter_bench/examples/compiler_route_overhead.rs` | `fda6bb0a5f868e4ef870c5c36364a324728f9d35` |
| `crates/verter_compiler/src/standalone.rs` | `022c9a379aa5e73f4cae18eaaeb83423f172495d` |
| `crates/verter_compiler/src/assembly/publish.rs` | `8d8b339d17d85af7b7d12a58b59c3a6ff0a14b24` |
| `crates/verter_parser/src/ast/types.rs` | `286b52d4a21096c7208bd05fe243d8d4a93240db` |
| `crates/verter_parser/src/parser/types.rs` | `cd894acb2d184a35e74fcfa8efd1f51fce4a5006` |
| `crates/verter_compiler/src/svelte/parser/retained_weight.rs` | `8cbc75de2ec91b304de3c9c411d15c5b9f73ef0e` |

The check to run: every blob hash above must equal `git rev-parse HEAD:<path>`. If any
differs — including by a comment — this record is out of date and the numbers below are
not evidence for HEAD.

**A commit landed mid-measurement, and the first four sessions were discarded because of
it.** `3ae319a23` ("group batch items by map probe instead of a linear scan") rewrote
`compile_batch` at 18:43, after four sessions had already been taken from a binary built
at 17:47 from the parent `323bc7fb4`. A worktree check does not catch this: by the time
those sessions ran, the worktree was already at `3ae319a23` and only the *binary* was
stale. The **binary** SHA-256 is what separates them, which is why it is recorded above
and in every session header. The discarded sessions are retained as the A/B in
[`route-overhead-sessions-2026-08-24.txt`](route-overhead-sessions-2026-08-24.txt) and
are not part of the measurement.

## Machine, and the one protocol conjunct this host could not meet

The host is the locked runner class in `performance-gates.toml` `[runner]` —
`apple-silicon-laptop-8core-24gib`, Darwin 25.6.0 arm64, Apple M3, 8 logical CPUs,
25769803776 bytes, rustc 1.97.1, node v20.20.2 — matched on every field.

`evidence/B6/cell-lock/pre-measure-registration.md` section 5 freezes an idle-machine
protocol whose conjuncts are individually checkable. Reported honestly, conjunct by
conjunct, for the published session:

| conjunct | required | observed | |
|---|---|---|---|
| no other `cargo` / `rustc` / `cargo-nextest` | none | 0 at every probe | **met** |
| AC power | on AC | `Now drawing from 'AC Power'` | **met** |
| macOS low-power mode | `0` | `0` | **met** |
| thermal warning | none | `No thermal warning level has been recorded` | **met** |
| control medians at session start and end | within 3.0% | worst arm 2.14% | **met** |
| 1-minute load average | `< 2.00` | 2.41–2.54 | **NOT met** |

**The load conjunct was not met and could not be met on this host.** With every build
process stopped and the machine otherwise idle, ambient load sat at 2.41–2.54 across
this session's probes, against a CPU that was 70.4–75.3% idle (median 73.3%, over the
settled samples of each `top -l 3` probe; each batch's first sample covers a since-boot
window and reads 43–67%, and is excluded for that reason). The residual load is a
supervised RustDesk remote-desktop server (≈15% of one core, respawned by its `service`
supervisor within seconds of being killed, so stopping it would mean disabling the
operator's remote access), WindowServer, a Terminal, and ~15 resident node/claude
processes. Load average on macOS counts I/O-blocked threads, which is why it reads ~2.5
while roughly three quarters of the CPU is idle.

Under the frozen protocol's own words — *"session is void if any fail"* — **this session
does not satisfy the registered idle-machine protocol.** It is published as the best
available measurement on this host, with that failure stated, not as a
protocol-conformant one. What replaces the missing conjunct as evidence of a stationary
machine is measured rather than asserted, and is given below.

### Evidence the machine was stationary anyway

Two independent controls, both recorded in the receipt:

- **A compiler-independent synthetic control** — a fixed 200-million-iteration integer
  loop, timed in-process, 11 repetitions before the session and 11 after. Median drift
  across the whole session: **−0.00%** (221.749 ms → 221.743 ms), with a within-block CV
  of 0.19% and 0.17%. A machine drifting under competing load does not reproduce a fixed
  workload to four significant figures at both ends of the session.
- **The protocol control** — the measured binary itself, re-run 8 times at session start
  and 8 times at session end, per `[runner].control_benchmark` ("the control benchmark is
  the baseline arm itself... a separate synthetic control would drift independently of
  the thing being measured"). Worst arm drift **2.14%**, inside the frozen 3.0% bound;
  first-10-vs-last-10 drift across the measured samples ran from **0.00%** to **−0.98%**.

The residual ambient load is therefore *steady*, not *trending* — which is the property
that matters. A steady load inflates every sample alike and is visible in the CV; a
trending load corrupts the median and is what voided the previous record.

## Command

```
CARGO_BUILD_JOBS=4 cargo build -p verter_bench --release --features attribution \
  --example compiler_route_overhead
./target/release/examples/compiler_route_overhead      # x46 (8 control, 30 measured, 8 control)
```

`--features attribution` is required (`required-features` on this example): the harness
reads the real `compiler.carrier_parse.calls` counter
(`verter_audit::attribution::{reset, read}`) around every leg instead of trusting its own
loop-iteration count, and that counter does not exist in the build without the feature.
An unmeasured build is therefore a hard error, not a silently zeroed run.

`VERTER_ROUTE_OVERHEAD_REPEATS` was unset, so the prepared-repeat leg used the harness
default of 5 repeats per carrier.

## Sampling

30 measured cold process invocations, per `[statistics].short_min_samples`. One sample is
one process, matching the cell's `cache_state = cold_process_per_invocation`. Every
sample entered the statistic — `[statistics].outlier_policy`'s `no_discretionary_exclusion`
was applied and nothing was excluded. Eight further invocations before and eight after
form the control blocks and are kept out of the measured statistic. Machine condition was
probed before the session, after every 10th measured sample, and after the session; all
probes are in the receipt.

## Result — the four arms

Nothing refused. All four arms this cell names ran and reported. Complete unedited stdout
for all 46 invocations: [`route-overhead-run-2026-08-24.txt`](route-overhead-run-2026-08-24.txt).

### Load-insensitive conjuncts

Constant across all 46 invocations — 184 arm-runs — of the published session, and
likewise across all eight sessions taken today:

- Every invocation exited **0**. Nonzero exits: **0**.
- A **single** output digest, `e5bb61c90a3eea9154593277e63f90633678382b1b251f9feff230218fa6204b`,
  across every arm of every invocation, with the harness's own
  `all routes produced byte-identical output (7 fixtures)` printed 46 times.
- The measured `compiler.carrier_parse.calls` delta per arm, constant:
  `direct=7 prepared-first=7 prepared-repeat=0 batch=7 (n=7, repeats=5)`.
- Per-arm `(cold_build_count, reuse_count, measured_carrier_parse_calls)`, constant:
  `direct (7, 0, 7)`, `prepared-first (7, 7, 7)`, `prepared-repeat (0, 35, 0)`,
  `batch (7, 14, 7)`.
- Batch topology, constant: `item_count=14 distinct_group_count=7`, the group count
  independently known from `corpus.len()` rather than from what `compile_batch` reported.

Derived from those counters: parse amplification on the direct arm is exactly
**7 parses / 7 distinct sources = 1.000**; the prepared-repeat arm performed **35**
`compile_prepared` calls for **zero** carrier parses; the batch arm parsed **7** times for
**14** submitted items over **7** distinct source keys.

### Wall clock, 30 cold samples

`latency_ms` as the harness reports it — the elapsed time of one whole leg, not per call.

| arm | median | min | max | p25 | p75 | mean | SD | CV |
|---|---|---|---|---|---|---|---|---|
| direct | **1.757** | 1.687 | 1.841 | 1.747 | 1.776 | 1.759 | 0.032 | 1.83% |
| prepared-first | **1.116** | 1.080 | 1.333 | 1.109 | 1.126 | 1.128 | 0.048 | 4.25% |
| prepared-repeat | **4.457** | 4.401 | 4.903 | 4.431 | 4.482 | 4.500 | 0.114 | 2.54% |
| batch | **1.814** | 1.793 | 2.220 | 1.806 | 1.847 | 1.881 | 0.141 | 7.51% |

The reported statistic is the **median**, matching every `wall_ns` row in the cell.
Spread is given as min/max/p25/p75/SD/CV rather than a single sample, per this record's
purpose. The comparable figure from the cell's own calibration session is a 5.1678% wall
CV; three of the four arms came in under it and the batch arm above it.

### Peak RSS, 30 cold samples

`rss_peak_bytes`, the harness's sampled running maximum. On macOS its source
(`getrusage().ru_maxrss`) is already a process lifetime peak, which is why these are
near-deterministic.

| arm | median | min | max | CV |
|---|---|---|---|---|
| direct | **7,421,952** | 7,372,800 | 7,471,104 | 0.45% |
| prepared-first | **7,757,824** | 7,700,480 | 7,815,168 | 0.42% |
| prepared-repeat | **7,979,008** | 7,929,856 | 8,011,776 | 0.35% |
| batch | **8,110,080** | 8,044,544 | 8,142,848 | 0.32% |

The cell's calibration peak-RSS CV was 0.5986%; every arm came in under it.

### Reproducibility across independent sessions

Four independent 30-sample sessions were taken from this binary. Their medians:

| session | direct | prepared-first | prepared-repeat | batch |
|---|---|---|---|---|
| H1 (published) | 1.757 | 1.116 | 4.457 | 1.814 |
| H2 | 1.760 | 1.118 | 4.465 | 1.829 |
| H3 | 1.771 | 1.118 | 4.468 | 1.835 |
| H4 | 1.761 | 1.131 | 4.526 | 1.836 |
| **spread** | **0.80%** | **1.39%** | **1.56%** | **1.19%** |

Peak-RSS medians across the same four sessions agree to within 0.000–0.423%. H1 is
published because it is the session whose own drift certificate passed; H2–H4 did not
(their control or within-session drift exceeded 3.0%) and are reported here as
corroboration of the medians only, not as measurements.

## What these numbers do and do not support

- **The arms are not independently warmed, so do not compare them to each other.** The
  four legs run in a fixed order in one process — direct, prepared-first, prepared-repeat,
  batch — and the direct leg absorbs the process's first-touch costs. This is visible in
  the receipt rather than inferred: `rss_before_bytes` for the direct leg is 1,769,472 in
  every run, while the prepared-first leg starts at 7.39–7.50 MB. The direct arm therefore pays
  for first-touch page faults on ~5.6 MB of heap that the later arms find already
  resident. Within-arm comparison across trees is sound; cross-arm comparison within one
  process is confounded by leg order. In particular, prepared-first reading *lower* than
  direct is not evidence that preparing and compiling is cheaper than compiling.
- **This session does not evaluate the locked cell's metric rows.** The cell's
  `corpus_fingerprint` pins a different harness — `route_overhead_baseline.rs`, eight
  in-process sources (4 Vue, 4 Svelte) — while this harness carries seven fixtures
  (4 Vue, 3 Svelte). The cell's structural rows are stated over that eight-source corpus
  (`route.direct.compile_calls` exactly 8, `route.direct.artifact_count` exactly 8,
  `route.direct.payload_bytes` exactly 5384, `route.batch.unique_source_parse_calls` ≤ 8)
  and this harness emits none of those metric names. Two rows have direct semantic
  counterparts that this run satisfies on its own corpus —
  `route.prepared_repeat.additional_parse_calls` ≤ 0 (measured 0) and
  `parse_amplification` ≤ 1.0 (measured exactly 1.000) — but a semantic counterpart on a
  different corpus is not the locked row. Reconciling the cell's corpus with the harness
  the block actually built is outside what a measurement can decide.
- **The wall bounds have little teeth at this scale, by the cell's own analysis.** The
  cell records that its 20 ms absolute sits far above the operation's median and first
  trips at roughly a 5377% regression. Every arm here is between 1.1 ms and 4.5 ms, so
  that remains true. The discriminating evidence in this run is the output oracle and the
  two-sided work counters, not the clock.

## The commit that landed mid-measurement, measured

Because `3ae319a23` rewrote `compile_batch` while this measurement was in progress, the
four discarded parent-tree sessions and the four HEAD sessions form a same-machine,
same-harness, same-corpus A/B over the change. Median of each group's four session
medians:

| arm | parent `323bc7fb4` | HEAD `3ae319a23` | delta |
|---|---|---|---|
| direct | 1.7325 | 1.7603 | +1.60% |
| prepared-first | 1.1078 | 1.1178 | +0.90% |
| prepared-repeat | 4.4740 | 4.4665 | −0.17% |
| batch (**the touched arm**) | 1.8272 | 1.8320 | +0.26% |

**No effect on the batch arm is resolved by this comparison.** The touched arm moved less
than two of the three arms the commit does not touch, and less than the 0.80–1.87%
between-session spread within a single binary. Using each untouched arm in turn as a
common-mode control for `batch` gives −1.32% (against direct), −0.64% (against
prepared-first) and +0.43% (against prepared-repeat) — three estimates that disagree in
sign across a 1.75-point range, which is the definition of an unresolved effect rather
than a small one.

This is a statement about resolution, not about the change. At this corpus size the
replaced linear scan performed at most ~50 key comparisons for the whole batch (14 items
over 7 groups), which is nanoseconds against a 1.8 ms leg; the map probe is
asymptotically the better structure and this corpus cannot exercise the case where that
matters. Resolving a sub-1% effect would need the frozen
`[statistics].interleave_policy` — both binaries alternated A,B,B,A within one session,
which cancels the common-mode drift these two session groups clearly carry — and a corpus
with many more distinct sources per batch.

## Not an acceptance record

This document records a measurement and nothing else. No `[[cell]]` field was added or
amended — `corpus_fingerprint`, the metric rows, and every other field of
`B6_COMPILER_ROUTE_OVERHEAD` are untouched. No ledger transition was written and no
`accepted_sha` was recorded. Acceptance is the maintainer's, and the disposition frozen in
`scoping-spec.md` section 6 stands: B6 builds the harness, runs it, and records the
measured numbers as landing evidence; a formally locked threshold is a separate ADR-016
action this block was not chartered to invoke.

## Superseded

Earlier revisions of this document recorded runs that have since been replaced, each for
a stated reason: a harness whose `cold_build_count`/`reuse_count` were assigned from its
own loop-iteration counts (true by construction, and unable to fail when
`compile_prepared` regressed to re-parsing); a batch leg that submitted one item per
distinct source, so `cold_build_count == n` held whether or not anything deduped; two
receipts whose recorded commit no longer matched the harness or production blobs the
numbers were taken from; and the immediately preceding revision, whose counters and digest
stand but whose timings were taken under concurrent load and are void. Their receipt is
retained unedited at [`route-overhead-run.txt`](route-overhead-run.txt) so the history
stays checkable. Only the measurement above is current. The stable conjuncts across every
revision have been the work counters, cross-route digest equality, and exit 0.
