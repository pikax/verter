# A6 — Counter reproduction against A4's retained baseline

Establishes that every counter the gate file turns into a pass/fail metric is reproducible, and
states exactly which counters are not.

## Method

```sh
cargo build -p verter_bench --release --features attribution --example attribution_baseline
./target/release/examples/attribution_baseline --files 40 --runs 3 --format tsv
```

Run three times from a cold process each time. The harness resets the counter table before every
measured run and dumps the last one, so a row is one workload pass regardless of `--runs`
(`crates/verter_bench/examples/attribution_baseline.rs`, `reset()` inside the measured loop). The
dataset saved here is run 1: [`baseline-counters.tsv`](baseline-counters.tsv), 45 lines — one header
plus **44 data rows**. A4's retained dataset has 44 data rows as well, which is what lets the two be
joined row for row.

Compared against A4's retained [`../A4/baseline-40-components.tsv`](../A4/baseline-40-components.tsv),
which was captured on a source-identical tree (see
[`baseline-measurement.md`](baseline-measurement.md) §1). That gives **four** independent
observations of every row: A4's, plus three here.

## Result — the identity-bearing columns

Joining the two datasets on `site` and comparing `calls`, `amount` and `digest` across all 44 data
rows yields exactly **two** differences:

| site | A4 | A6 | why |
|---|---:|---:|---|
| `scheduler.task_wait` | amount 112,353,207 | amount 114,591,498 | the site's declared unit is `Nanoseconds`; its amount column *is* a duration, so it is a timing value in a counter's clothing |
| `session.read_set_signature_build` | amount 134,470 | amount 134,530 | +60 observed items (+0.045%); `calls` is identical at 5,115 in both |

Everything else — all 42 remaining data rows' `calls`, `amount` and `digest` — is byte-identical.

The `read_set_signature_build` drift is the load-bearing negative result here. It is a *count*, not a
clock, and it moved anyway. So "it is a counter" is not by itself a licence to gate on it, and the
gated set was restricted to counters that actually held still.

## Result — the gated subset, three fresh runs

Every counter the locked cell gates on, printed as `calls/amount/digest`, plus
`session.semantic_warm_hit`, which is recorded here alongside cold builds for context and is
deliberately **not** gated: an `absolute_max` on warm hits would pass precisely when warm hits
collapse, which is the failure it would appear to guard.

| site | run 1 | run 2 | run 3 |
|---|---|---|---|
| `workspace.normalize_canonical_id` | 11313/175101/0 | 11313/175101/0 | 11313/175101/0 |
| `compiler.carrier_parse` | 40/41470/0 | 40/41470/0 | 40/41470/0 |
| `session.oxc_script_parse` | 40/41470/0 | 40/41470/0 | 40/41470/0 |
| `session.oxc_eval_program_parse` | 42/42480/0 | 42/42480/0 | 42/42480/0 |
| `session.source_text_copy` | 120/124410/0 | 120/124410/0 | 120/124410/0 |
| `session.fact_observe` | 16917/73923/0 | 16917/73923/0 | 16917/73923/0 |
| `session.indexed_ready_build` | 8032/0/0 | 8032/0/0 | 8032/0/0 |
| `session.semantic_cold_build` | 1063/0/0 | 1063/0/0 | 1063/0/0 |
| `session.cache_admit_cacheable` | 1063/0/0 | 1063/0/0 | 1063/0/0 |
| `session.semantic_dispatch` | 4216/0/0 | 4216/0/0 | 4216/0/0 |
| `session.semantic_warm_hit` | 3153/0/0 | 3153/0/0 | 3153/0/0 |
| `session.component_meta_digest` | 40/0/7161214711717846280 | 40/0/7161214711717846280 | 40/0/7161214711717846280 |

All twelve agree with A4's values as well. Four-for-four.

## Zero-work assertions

`compiler.css_parse`, `compiler.css_transform` and `compiler.style_analysis` appear in **no** dataset
row. The TSV renderer emits only sites that recorded an observation, so absence is zero. The corpus
authors no style block, which is why the locked cell can assert zero CSS work as a
requested-and-forbidden-incidental-work proof rather than as a coincidence.

Verified directly: `grep -c "css\|style_analysis" baseline-counters.tsv` → `0`.

## Determinism

The harness runs the workload twice in-process and compares the component-meta digest:

```
component_meta   run1= 7161214711717846280  run2= 7161214711717846280  AGREE
compiled_output  run1=                   0  run2=                   0  N/A (no observations)
```

The digest was observed identically in four runs **on one machine**. Cross-machine reproducibility is
untested and is not claimed; the cell is bound to the locked runner class for that reason among
others, and a different runner class is a recalibration rather than a local adjustment.

`AGREE` is non-vacuous — the harness pairs each digest with the call count that produced it and
reports `N/A` when both sides are zero, so a site the workload never reached cannot manufacture an
agreement. The compiled-output digest is exactly that case, and it is reported as N/A rather than as
a pass. The gate file does not gate on it.

## Sites recording zero that the gate deliberately does not assert zero

A4 recorded these as known gaps: the site is wired and correct, this workload's lane does not reach
it.

- `compiler.compiled_output_digest` — sits on the Vue bridge compile entry, which `compile_many`'s
  host-backed lane does not take for these inputs.
- the CSS sites are asserted zero (above) because their absence is a property of the *corpus*, not of
  an unreached lane. The distinction is why one set is a gate and the other is not.
- the FFI boundary sites — scope-only, on each crate's `catch_panic` wrapper; no FFI crossing occurs
  in an in-process Rust harness.

Freezing an unreached lane's zero as a gate requirement would make a later block's correct fix fail
the gate. None is asserted.

## Enabled-arm cost, recorded so it is not mistaken for the cell's baseline

The counter dataset is produced by the **enabled** arm, which also installs `AttributingAllocator` as
the global allocator. Its wall median on this workload is 79.04 ms against the disabled arm's 70.525 ms
— the +7–13% A4 measured, consistent on re-measurement. The locked cell's `wall_ns` and
`peak_rss_bytes` metrics are measured on the **disabled** arm; the counter metrics are measured on the
enabled arm. The gate file records that split per metric so the two are never compared to each other.
