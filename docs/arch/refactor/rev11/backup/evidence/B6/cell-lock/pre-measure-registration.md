# Pre-measure registration — `B6_COMPILER_ROUTE_OVERHEAD`

**Status:** committed before the first calibration run.
**Date:** 2026-08-23
**Authority:** pre-B6 gate-authority repair, maintainer-authorised 2026-08-23
  following the architecture consult at
  `~/.claude/briefs/rev11/verify/b6-perfcell-consult.out`.
**Protocol model:**
  `docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md`
  steps 2–6 (exact subject, pre-measure registration, 30-run calibration,
  disjoint holdout). Absolute budgets are product/CI budgets. They are
  **not** a multiple of any B6 timing or RSS figure. Existing B6
  timing/RSS results are contaminated audit evidence only and were not
  read for this registration.

This file is the digest-addressed registration. Threshold arithmetic after
calibration may only *instantiate* the formulas below. It may not change
them.

## 1. Measurement subject

| axis | frozen value |
|---|---|
| Cell id | `B6_COMPILER_ROUTE_OVERHEAD` |
| Owner | B6 routes (gate locked here, before B6 is measured) |
| Boundary | Rust (`StandaloneCompiler`) |
| Product | `CompileProduct::RuntimeClient(RuntimeProductRequest::default())` — `runtime_source_map = false`, `inline = None` (resolves from `is_production = false`) |
| Frameworks | Vue (`VueCompileRequest::default()`) and Svelte (`SvelteCompileRequest::default()`) |
| Semantic profile | none |
| Maps / diagnostics / provenance / serialization | off (maps-on is a different cell, verification.md 8.4) |
| Direct execution inputs | Vue: `VueExecutionInputs::default()` + `VueMacroSemanticInput::Unavailable`; Svelte: `SvelteExecutionInputs { css_hash_override: None }` |
| Corpus | eight in-process sources synthesised by the harness (four Vue, four Svelte). No fixture directory, no third-party corpus. |
| Harness | `crates/verter_bench/examples/route_overhead_baseline.rs` |
| Build | `cargo build -p verter_bench --release --example route_overhead_baseline` with `CARGO_BUILD_JOBS=4` |
| Runner class | the already-locked `[runner]` in repo-root `performance-gates.toml` (`apple-silicon-laptop-8core-24gib`) |
| Control benchmark | unchanged (`attribution_baseline --files 40 --runs 30`); session void if start/end control medians differ by more than `runner.max_control_drift_percent` |
| Allocator | system allocator (no `attribution` feature, no `AttributingAllocator`) |

Prepared-first, prepared-repeat, and direct-batch arms are part of the
cell identity. They do not exist on the B5 tree this registration
measures. Their **absolute** wall/RSS ceilings are locked here from the
B5 direct product budget. Their work counters are locked here as
structural contracts. B6 is measured against those ceilings later; it
does not get to pick them.

## 2. Corpus (size distribution)

Eight independently authored Verter-local sources, in this order:

| id | framework | bytes (source) | notes |
|---|---|---:|---|
| `vue-simple` | Vue | small | script-setup + text interpolation |
| `vue-styled` | Vue | small | same + one `<style>` block |
| `vue-list` | Vue | medium | `v-for` / `v-if` / event handler |
| `vue-computed` | Vue | medium | `ref` + `computed` + template branch |
| `svelte-simple` | Svelte | small | runes `$state` counter |
| `svelte-styled` | Svelte | small | same + one `<style>` block |
| `svelte-each` | Svelte | medium | `{#each}` list |
| `svelte-if` | Svelte | medium | `{#if}` / `{:else}` |

Exact source bytes live in the harness. The harness blob is the corpus
fingerprint (same discipline as `A6_META_COMPILE_40_COLD_RUST`).

## 3. One measured sample

1. One unmeasured warmup pass over the eight sources (discarded).
2. One measured pass: `StandaloneCompiler::compile` on each source in
   corpus order, `RuntimeClient`, maps off.
3. Record wall nanoseconds for the measured pass only.
4. Record `compile_calls = 8`, `artifact_count = 8`, and the SHA-256 of
   the concatenated payload `id \\0 code \\0 styles_len \\n` over the
   eight artifacts.

Peak RSS is process-level: `/usr/bin/time -l` around one cold invocation
of `--runs 1` (warmup + one measured pass).

## 4. Sampling

- Calibration session: **30 cold process invocations** of
  `--runs 1` (`[statistics].short_min_samples`). Every sample enters the
  statistic. A run whose validity assertions fail, or during which the
  machine was not idle, invalidates the whole session, not the sample.
- Holdout session: a **disjoint** 30 cold invocations of the same
  binary, started after calibration has been written to disk. Holdout is
  the pass/fail evidence. Calibration is not.
- Interleave inside a session is unnecessary for a single-arm B5
  baseline. Later B6 measurement against this cell uses the file-level
  `alternating_invocation_abba` policy (B5-direct vs B6 arm).

## 5. Idle-machine protocol (session is void if any fail)

- 1-minute load average `< 2.00` on this 8-core class.
- No other `cargo`, `cargo-nextest`, `gate.mjs`, or `rustc` process
  except the session's own child.
- AC power; `pmset -g | grep lowpowermode` reports `0`.
- Control benchmark run at session start and end; void if the two
  control medians differ by more than 3.0%.

A measurement taken under load is not a measurement. If this protocol
cannot be met, this registration produces no cell.

## 6. Pre-registered absolute budgets (product/CI, not a fit)

These numbers are justified independently of any run of this harness and
independently of B6.

### 6.1 `wall_ns` absolute_max = 20_000_000 (20 ms)

A6 locked 2.5 ms per component for a **heavier** host-backed
component-meta + compile batch
(`performance-gates.toml` cell `A6_META_COMPILE_40_COLD_RUST`, 100 ms
for 40 components). This cell's DIRECT arm is B5's
`StandaloneCompiler::compile` of eight local RuntimeClient sources —
no host, no component-meta, no VFS. The lighter path may not be
budgeted slower than the heavier locked path at the same per-file
product rate.

8 × 2.5 ms = **20 ms = 20_000_000 ns**.

The same 20 ms ceiling is the absolute_max for `prepared_first.wall_ns`,
`prepared_repeat.wall_ns`, and `batch.wall_ns`. A new route that exceeds
the B5-direct product budget on the identical corpus has failed the
product, not earned a larger budget.

### 6.2 `peak_rss_bytes` absolute_max = 134_217_728 (128 MiB)

A6 locked 256 MiB as a catastrophe stop for a 41-file **host** process
(an editor host must hold a 1_000-file project under 2 GiB). This cell
is a standalone process with no host/session, eight files. **128 MiB**
is half that host catastrophe stop. The tight fence is the relative
gate; the absolute is a catastrophe stop, same philosophy as A6.

Prepared/batch arms share this process-level RSS ceiling: they run in
the same class of standalone process over the same corpus.

### 6.3 What these are not

- Not `k × B6_observed_median` for any k.
- Not a number read from
  `verter-b6/.../B6_COMPILER_ROUTE_OVERHEAD-measurement.md`.
- Not tunable after calibration. If the B5-direct calibration median
  exceeds 20 ms, or its max RSS exceeds 128 MiB, this lock **stops**
  rather than raising the ceiling.

## 7. Relative-noise formula (frozen before calibration)

```
no_regression_percent_max = max(3.0000, 2 * population_cv_percent)
```

- `population_cv_percent` is the sample coefficient of variation
  (100 × s / mean) over the 30 calibration wall samples for `wall_ns`,
  and over the 30 calibration peak-RSS samples for `peak_rss_bytes`.
- `s` is the population standard deviation (divide by n, not n−1),
  matching a 30-sample census of the session.
- The result is not rounded up. verification.md 8.3 is an upper bound.
- The same wall percentage is the no-regression bound on every wall
  metric in the cell. Prepared/batch have no independent B5 noise;
  the B5-direct calibration is the comparable leg.

## 8. Work counters (structural, not timed)

| metric | statistic | comparison | limit | why |
|---|---|---|---:|---|
| `route.direct.compile_calls` | max | absolute_max | 8 | one compile per source |
| `route.direct.compile_calls` | max | absolute_min | 8 | a faster run that compiled fewer files fails |
| `route.direct.artifact_count` | max | absolute_max | 8 | one RuntimeClient artifact per source |
| `route.direct.artifact_count` | max | absolute_min | 8 | |
| `route.direct.payload_bytes` | max | absolute_max | 5384 | concatenated RuntimeClient code bytes |
| `route.direct.payload_bytes` | max | absolute_min | 5384 | empty-faster fails |
| `route.prepared_repeat.additional_parse_calls` | max | absolute_max | 0 | reuse is the route; a reparse is overhead |
| `route.batch.unique_source_parse_calls` | max | absolute_max | 8 | batch of 8 unique sources parses each once |
| `parse_amplification` | max | absolute_max | 1.0 | template field; parses / unique sources |

`parse_amplification` for the DIRECT arm is identically 1.0 (8/8).

## 9. Output oracle

`sha256(payload) == 577f62e3ba72dcf39cd56d62285372b249752be1c1b8c3bedf02e70070446131`

taken from a correctness compile of this harness (`--runs 1 --warmup 1`) on
2026-08-23. Load-insensitive: the digest is over artifact bytes, not wall
or RSS. Reproduced on a second invocation in the same process.

Payload = concatenation over corpus order of
`id || 0x00 || artifact.code() || 0x00 || decimal(styles.len()) || 0x0a`.

`payload_bytes` of that compile was 5384. The cell gates
`route.direct.payload_bytes` at absolute_min = absolute_max = 5384 so an
empty-faster run fails.

A candidate that emits different code faster fails. The digest is a
correctness pin, load-insensitive, and may be taken on a busy machine.

Zero-counter assertions (unrequested work):

- `network.dns_resolution_attempts`
- `network.socket_connect_attempts`
- no `IdeCompanion` / `RuntimeServer` / `Declarations` / `PublicApi` /
  `Analysis` artifact is published (the request did not ask for them)

CSS parse is **requested** on the two styled sources and is not
zero-asserted.

## 10. Holdout pass/fail (the evidence, not calibration)

The holdout session passes only if all of the following hold:

1. holdout median `wall_ns` ≤ 20_000_000
2. holdout max `peak_rss_bytes` ≤ 134_217_728
3. `|holdout_median_wall − calibration_median_wall| / calibration_median_wall`
   ≤ the frozen relative bound (otherwise the session is void as drift,
   not a fail of the budget)
4. work counters equal the table in §8 for the DIRECT arm
5. output digest equals the correctness pin
6. idle-machine protocol held for the whole holdout session

Calibration numbers never become the absolute limits.

## 11. Request-identity string

SHA-256 is over this literal, no trailing newline:

```
verter-rev11-cell:B6_COMPILER_ROUTE_OVERHEAD;operation=direct_prepared_first_prepared_repeat_batch_runtime_client;product=RuntimeClient;runtime_source_map=false;is_production=false;inline=default;frameworks=vue+svelte;corpus=8_in_process_sources;harness=crates/verter_bench/examples/route_overhead_baseline.rs
```
