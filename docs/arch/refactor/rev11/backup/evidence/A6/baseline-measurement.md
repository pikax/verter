# A6 — The measured baseline behind `performance-gates.toml`

Every number in the locked gate file traces to this page. Identity-free by the A0 convention: the
tree it measures is named by its properties (below), not by a candidate SHA.

## 1. The tree measured is source-identical to the tree A4 measured

The gates are frozen against the post-A4 implementation baseline. A4 captured its dataset on its own
accepted candidate; A5 changed no production source. That is verified, not assumed:

```sh
git diff --name-only 1ab403c0107801b080438fab30b887c0c8164ecb <A6-baseline>
```

returns only `docs/arch/architecture-lock/ledger/program-state.toml` and files under
`docs/arch/refactor/rev11/`. Restricted to the source surface —

```sh
git diff --stat 1ab403c0107801b080438fab30b887c0c8164ecb <A6-baseline> \
  -- crates packages scripts Cargo.lock Cargo.toml pnpm-lock.yaml .github
```

— the diff is **empty**. So A4's retained counter dataset
([`../A4/baseline-40-components.tsv`](../A4/baseline-40-components.tsv)) describes this exact
source, and the timings below re-measure it under the sampling policy the gate file locks.

The measured commit is named above by its **current** SHA. It was previously cited as
`147258e0be47b65fb872236599655d06bf4621f5`, which the integration lineage's commit-message rewrite
superseded; the replacement has an identical tree OID (`4ccdd9e72e21aa3bbf16615ee59d23339768ef1b`)
and an identical `git patch-id --stable`, so the tree these numbers were measured on is unchanged
and only its name moved. The diffs above were re-run against the current lineage rather than carried
over, and are empty there. See the lock record §1.

## 2. Why the timings were re-measured rather than carried over

A4's wall-clock arms used 7 measured runs per invocation. `verification.md` §8.3 requires at least
30 measured samples for a short cell, and A6 freezes that policy. Carrying a 7-run median into a
file that locks `short_min_samples = 30` would lock a baseline that does not satisfy its own
sampling rule. The counter dataset needed no re-measurement for a different reason: the harness
resets counters before every measured run and dumps the last one, so counter values are per
workload pass and independent of `--runs` (`crates/verter_bench/examples/attribution_baseline.rs`
— `reset()` inside the measured loop, and the comment at the `emit_dataset` call site: "The dataset
reflects the LAST measured run only (each run resets)"). It was re-run anyway, and §4 records the
comparison.

## 3. Wall clock and peak RSS — disabled arm, release profile

Harness: `crates/verter_bench/examples/attribution_baseline.rs`. The corpus is synthesised
in-process (40 Vue components + 1 shared TypeScript module), so the command reproduces on any
machine with no fixture directory and no external checkout. One measured sample = one full workload
pass: fresh host, upsert 41 files, `ensure_loaded`, `get_component_meta` per component, then
`compile_many` over the corpus. The harness runs one unmeasured warmup pass first.

```sh
cargo build -p verter_bench --release --example attribution_baseline
for i in 1 2 3 4; do
  /usr/bin/time -l ./target/release/examples/attribution_baseline --files 40 --runs 30
done
```

| invocation | median ms | min ms | maximum resident set size (bytes) |
|---|---:|---:|---:|
| 1 | 72.15 | 69.05 | 75,104,256 |
| 2 | 70.06 | 68.36 | 74,153,984 |
| 3 | 70.94 | 68.52 | 77,889,536 |
| 4 | 70.11 | 68.61 | 74,596,352 |

Each invocation reports the median of its own 30 measured samples. Four invocations were run so the
**between-invocation** spread — which is what a candidate-vs-baseline comparison actually has to
survive — could be measured rather than assumed.

### 3.1 Locked baseline statistics

- **`wall_ns` baseline** = median of the four invocation medians = `(70.11 + 70.94) / 2` =
  **70.525 ms = 70,525,000 ns**.
- **`peak_rss_bytes` baseline** = median of the four maximum-RSS readings =
  `(74,596,352 + 75,104,256) / 2` = **74,850,304 bytes**.

### 3.2 Measured noise floor, and the gate it produces

Noise floor is taken as half the range of the invocation-level statistic, relative to its mean —
the largest one-sided error an unlucky pair of invocations can contribute.

| statistic | mean | range | noise floor | 2 × noise | `max(3%, 2 × noise)` |
|---|---:|---:|---:|---:|---:|
| `wall_ns` | 70.815 ms | 2.09 ms | 1.4757% | 2.9513% | **3.000%** |
| `peak_rss_bytes` | 75,436,032 | 3,735,552 | 2.4760% | 4.9520% | **4.952%** |

`verification.md` §8.3 caps the no-regression bound at `max(3%, 2 × measured noise floor)`. The
locked cell therefore takes `wall_ns` at 3.000% and `peak_rss_bytes` at 4.952%. Neither is rounded
up to the next convenient figure: the rule is an upper bound, so `4.952` may not be written as `5.0`.

One residue is disclosed rather than left implied. The RSS noise floor is exactly 2.4759733% and
twice it is 4.9519466%; the table's 2.4760% / 4.9520% are those values at the precision printed here.
So the locked `4.952` sits 0.0000534 percentage points **above** the exact cap, not below it — about
one part in 46,000 of the noise floor it derives from. The limit is not restated at more digits than
four 30-sample invocations support, and the residue is recorded rather than described as exact.

For wall clock this reproduces the maintainer's own frozen finding on a different cell — twice the
measured noise floor lands just below 3%, so noise does not control the gate and the 3% bound is
neither noise-driven nor a licence for a larger regression.

### 3.3 The absolute limits are product budgets, not fits

Stated explicitly because the distinction is the whole point of freezing gates before a candidate
exists. Neither absolute limit is derived from the measurements above.

- **`wall_ns absolute_max = 100,000,000` (100 ms).** A cold project batch must sustain no worse than
  2.5 ms per component end to end (component metadata plus runtime compile), so that a
  1,000-component project's initial batch stays inside a 2.5 s cold-project budget. Forty
  components buys 100 ms. The measured 70.525 ms sits under it with room; that is the budget being
  met, not the budget being fitted.
- **`peak_rss_bytes absolute_max = 268,435,456` (256 MiB).** An editor host must hold a
  1,000-file project under 2 GiB. A 41-file batch is budgeted 256 MiB — an eighth of the whole-project
  ceiling for four percent of the files, deliberately generous because peak RSS is allocator- and
  platform-dependent. The tight fence on this metric is the 4.952% relative gate; the absolute limit
  is a catastrophe stop, and it is recorded as such rather than pretending to be sensitive.

## 4. Work counters — enabled arm

```sh
cargo build -p verter_bench --release --features attribution --example attribution_baseline
./target/release/examples/attribution_baseline --files 40 --runs 3 --format tsv
```

Raw dataset: [`baseline-counters.tsv`](baseline-counters.tsv). It is compared row by row against
A4's retained [`../A4/baseline-40-components.tsv`](../A4/baseline-40-components.tsv) in
[`counter-reproduction.md`](counter-reproduction.md); the gated subset is reproduced exactly, which
is what licenses gating on it.

The counters that become gate metrics were chosen because each one fails for a specific, nameable
mistake rather than because it was available:

| metric | baseline | the mistake it catches |
|---|---:|---|
| `compiler.carrier_parse.calls` | 40 | a second carrier parse per file |
| `session.oxc_script_parse.calls` | 40 | a script re-parse outside the retained snapshot |
| `session.oxc_eval_program_parse.calls` | 42 | an eval-program re-parse per demand instead of per file version |
| `session.source_text_copy.amount` | 124,410 | a source-sized copy introduced by a new identity or unit type |
| `workspace.normalize_canonical_id.calls` | 11,313 | one more path normalisation per identity construction — the single most repeated call in the system, and exactly what a new `StableEntityId` wrapper is liable to add |
| `session.fact_observe.calls` | 16,917 | a fact observation added per identity or per key |
| `session.indexed_ready_build.calls` | 8,032 | a re-entry of the serve path per new key |
| `session.semantic_cold_build.calls` | 1,063 | **cache-key identity breakage** — the canonical failure mode of a block that changes what an identity means. A candidate whose new key type is not equal where the old one was turns warm hits into cold builds and this counter moves first |
| `session.cache_admit_cacheable.calls` | 1,063 | a cold build that stops being admitted (equal to cold builds at baseline: every cold build admits, zero `ReturnOnly`). **The one metric gated `absolute_min`** — this failure makes the counter FALL, and an `absolute_max` on it would pass with maximum margin exactly when admissions collapse to zero |

Zero-work assertions on the same cell: `compiler.css_parse`, `compiler.css_transform` and
`compiler.style_analysis` must record zero calls. The corpus authors no style block, so any CSS work
is incidental work the request did not ask for — `verification.md` §7's zero-work negative-proof
form.

Two sites that record zero on this corpus are deliberately **not** asserted zero:
`compiler.compiled_output_digest` and the FFI boundary sites. A4 recorded both as known gaps — the
site is wired but this workload's lane does not reach it. Freezing a gap as a requirement would make
a later block's correct fix fail the gate.

## 4a. The cell's request identity, in full

The gate file's `normalized_product_request_digest` is SHA-256 over the exact byte string below,
with no trailing newline. It is recorded here rather than only as a digest so the digest can be
recomputed instead of trusted. The Revision 11 normalized product-request type does not exist at
lock time; the request is therefore pinned by its literal description rather than by a type it has
no instance of.

```text
verter-rev11-cell:A6_META_COMPILE_40_COLD_RUST;files=40;corpus=synthesised-in-process;ops=upsert(41),ensure_loaded,get_component_meta*40,compile_many(CompileManyTarget::HostBacked,CompileBatchOptions::default());host=HostConfig::default();workspace=MemoryWorkspace(MemoryOptions::default());profile=release
```

```sh
printf '%s' '<the line above>' | shasum -a 256
# d80a5f9e174de68b10257e6ed929331f031950639496ac8465048804fb0f4d48
```

The instrumentation arm is deliberately **not** part of the request identity: the same request is
measured in both arms — timings in the disabled arm, counters in the enabled one — and making the
arm part of the identity would split one cell into two.

## 5. Output oracle

`session.component_meta_digest` records `7161214711717846280` — the order-independent digest over the
component-meta results for all 40 components. The locked cell requires that exact value. This is the
cell's semantic-equivalence oracle: it is what stops a candidate from passing every work and timing
gate by producing different metadata faster.

The harness's own determinism check runs the workload twice in-process and compares this digest; A4
recorded AGREE, and §4's re-run records the current verdict. The compiled-output digest reports
`N/A (no observations)` rather than a vacuous `0 == 0` agreement — the harness distinguishes the two,
and the gate file does not gate on it.

## 6. Runner

| field | value | source |
|---|---|---|
| os | `Darwin 25.6.0 arm64` | `uname -srm` |
| cpu | `Apple M3` | `sysctl -n machdep.cpu.brand_string` |
| logical / physical cpus | 8 / 8 | `sysctl -n hw.logicalcpu hw.physicalcpu` |
| memory | 25,769,803,776 bytes | `sysctl -n hw.memsize` |
| rust toolchain | `1.97.1 (8bab26f4f 2026-07-14)`, exact-pinned by `rust-toolchain.toml` | `rustc --version` |
| cargo | `1.97.1 (c980f4866 2026-06-30)` | `cargo --version` |
| nextest | `0.9.130 (f0feb11a1 2026-03-09)` | `cargo nextest --version` |
| node | `v20.20.2` | `node --version` |
| pnpm | `10.22.0` | `pnpm --version` |
| power policy | AC power, `lowpowermode 0` | `pmset -g` |

Absolute wall and RSS limits are bound to this runner class. A different machine class is a
recalibration under `verification.md` §8.1 (new record digest, retained before/after calibration, no
candidate result inspected first, independent performance reviewer) — not a local adjustment.

## 7. What this measurement does not establish

- **It is single-machine.** One runner class is locked; the gate file says so in `[runner]`. A
  second class would need its own calibration, and no claim is made that these absolute numbers
  transfer.
- **It measures one workload.** Forty synthesised Vue components sharing one TypeScript module,
  cold, host-backed. It is representative of the component-meta/compile path and of nothing else.
  The direct-compiler, CSS, provider, and competitor benchmark families `verification.md` §8.4
  requires are **not** measured here and no cell claims them.
- **`wall_ns` is not attributed.** The scope timings in the counter dataset are inclusive and
  re-entrant (A4 states this at length); they rank regions, they do not decompose wall clock, and
  the gate file does not gate on any of them.
