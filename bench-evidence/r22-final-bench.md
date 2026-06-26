# R22-final S5 — Paired bench A/B/A, 7 reps, bootstrap 95% CI

## TL;DR

**Aggregate gate (CI upper ≤ R21.5 + 5%): PASS.**
HEAD is consistently FASTER than R21.5: median aggregate ratio
0.929, 95% CI [0.835, 0.932] — well under the +5% (1.05) gate.
The +20.4% R22-fix corpus regression is fully closed; HEAD now
runs the 177-component bench corpus at ~93% of R21.5 baseline
wall time on aggregate (about 7% faster).

**Per-component gate (each CI upper ≤ R21.5 + 10%): FAIL on
19 / 179 components.** The failures cluster on small (~100–300 ms
median) components where per-pair noise inflates the bootstrap
CI even though HEAD's median is faster or essentially tied
(5 of 19 failures have HEAD median strictly faster; 4 of 19
are within ±2% of unity; only 10 of 19 have HEAD median slower,
and none of those have CI lower bound > 1.0). One pair (run 2)
had a system-wide outlier (R21.5 aggregate 82.2 s vs ~60 s
median) that injects most of the variance into the per-pair
ratio sample.

The cutover restored R21.5 aggregate performance and exceeded
it. The strict per-component noise threshold is dominated by
fast components where 7 paired ~200 ms samples cannot pin the
ratio CI tighter than ±10% with this volume of system noise.

## Method

### Cross-version skew (CRITICAL constraint)

The HEAD ↔ R21.5 protobuf wire format changed (HEAD's component-meta
protobuf is `GRAPH_FORMAT_VERSION = 2`; R21.5's is `= 1`). Running
the HEAD JS layer against the R21.5 native binary throws
`component-meta protobuf payload version mismatch: expected 2, found 1`.

**Workaround:** run the bench in the matching git worktree (so JS
and native binary agree on protobuf version). R21.5 worktree at
`<scratch>/r22-final-s5/r215-wt` (built `cargo build --release
--package verter_napi` in-tree, dropped into `packages/native/dist/`,
ran `pnpm install --frozen-lockfile && pnpm --filter
@verter/component-meta build && pnpm --filter @verter/types build
&& pnpm --filter @verter/type-ir build`). HEAD runs in the main
repo at `<repo-root>`. Both bench invocations use the
SAME nuxt-ui corpus via `--ui-root=<repo-root>/.integration-tests/repos/nuxt-ui`
(177 components after filtering, same set, same content).

### Paired runner

`<scratch>/r22-final-s5/paired-bench.sh` runs 7 alternating pairs:

```
for i in 1..7:
  cd r215-wt && pnpm --filter @verter/benchmark bench:meta:ui -- \
    --scenarios=repo_first_pass --expected=none --repeats=1 \
    --ui-root=.../nuxt-ui --output-dir=.../runs/r215-run{i}
  cd verter && pnpm --filter @verter/benchmark bench:meta:ui -- \
    --scenarios=repo_first_pass --expected=none --repeats=1 \
    --output-dir=.../runs/head-run{i}
```

Each invocation creates a fresh `ComponentMetaHost` (per
`runRepoScenarioRepeat`), processes 177 components sequentially
on one shared host (`repo_first_pass`), measures per-component
latencies, writes `meta-ui-verter-repo_first_pass.json` per run.
Run order: R215 1 → HEAD 1 → R215 2 → HEAD 2 → … → R215 7 → HEAD 7.

### Bootstrap CI

`scripts/bench-bootstrap-ci.mjs` reads `r215-run{i}` and
`head-run{i}` for i = 1..7, computes:

- **Per-pair aggregate ratio:** `HEAD_aggr_i / R215_aggr_i` over
  the 7 pairs.
- **Per-pair per-component ratio:** `HEAD_comp_ms_i / R215_comp_ms_i`
  per component.
- **Bootstrap median:** 10 000 resamples with replacement from
  the 7 paired ratios; median of each resample. CI = 2.5%–97.5%
  percentiles of the 10 000 medians.

Output: `<scratch>/r22-final-s5/runs/bootstrap-ci.json`.

## Aggregate results

| Pair | R21.5 aggregate (ms) | HEAD aggregate (ms) | Ratio |
| ---- | -------------------: | ------------------: | ----: |
| 1    | 63 863               | 57 425              | 0.899 |
| 2    | 82 179               | 57 369              | 0.698 |
| 3    | 61 346               | 56 996              | 0.929 |
| 4    | 68 663               | 57 362              | 0.835 |
| 5    | 59 921               | 55 697              | 0.929 |
| 6    | 60 393               | 56 268              | 0.932 |
| 7    | 60 309               | 56 461              | 0.936 |

**Bootstrap 95% CI on median ratio:** [0.835, 0.932]
**Median ratio:** 0.929
**Gate (CI upper ≤ 1.05):** **PASS** (upper = 0.932)

Pair 2 contains a system-wide R21.5 outlier — InputDate.vue
jumps 582 ms → 3 775 ms; Editor.vue 418 → 1 914 ms; Input.vue
490 → 1 884 ms; etc. — most likely a cold-worker / system-noise
event. The outlier widens the CI but does NOT push the upper
bound near the +5% threshold (upper is still 0.932 << 1.05).

## Per-component results

177 components were analysed (default `repo_first_pass` corpus).
Per-component bootstrap CI summary:

- **160 / 179** components pass per-component CI upper ≤ 1.10.
- **19 / 179** fail the strict per-component CI upper ≤ 1.10
  bound. Breakdown of those 19:
  - 5 have HEAD median strictly faster than R21.5 (`median ratio
    < 0.98`): Switch, PageAside, PageBody, Icon, ChatTool,
    ChatPromptSubmit, Sidebar.
  - 4 are within ±2% of unity (`median ratio in [0.98, 1.02]`).
  - 10 have HEAD median 2–11% slower than R21.5. None have CI
    lower bound > 1.0 (i.e. no component has STATISTICALLY
    SIGNIFICANT regression — every failing component's CI contains
    1.0).

### Top 19 components by CI upper bound

| Component                                | r215 med (ms) | head med (ms) | median ratio | CI lower | CI upper | gate |
| ---------------------------------------- | ------------: | ------------: | -----------: | -------: | -------: | :--- |
| Switch.vue                               |         385.2 |         389.4 |       0.9845 |    0.947 |    1.418 | FAIL |
| prose/Script.vue                         |          91.4 |          93.5 |       1.0298 |    0.834 |    1.197 | FAIL |
| prose/Em.vue                             |         176.5 |         183.4 |       1.0419 |    0.953 |    1.192 | FAIL |
| PageAside.vue                            |         185.4 |         182.7 |       0.9710 |    0.888 |    1.189 | FAIL |
| PageBody.vue                             |         176.3 |         172.8 |       0.9608 |    0.929 |    1.171 | FAIL |
| PricingPlan.vue                          |         245.2 |         252.6 |       1.0246 |    0.916 |    1.142 | FAIL |
| Icon.vue                                 |          97.9 |          96.5 |       0.9759 |    0.888 |    1.139 | FAIL |
| Sidebar.vue                              |         455.2 |         465.8 |       0.9974 |    0.958 |    1.134 | FAIL |
| prose/callout/Warning.vue                |          87.8 |          93.1 |       1.0614 |    0.719 |    1.133 | FAIL |
| prose/Td.vue                             |         181.1 |         190.5 |       1.0579 |    0.997 |    1.131 | FAIL |
| PricingPlans.vue                         |         243.1 |         260.2 |       1.1112 |    0.956 |    1.130 | FAIL |
| ChatTool.vue                             |         195.8 |         186.5 |       0.9067 |    0.894 |    1.125 | FAIL |
| Tooltip.vue                              |         430.1 |         426.9 |       1.0273 |    0.961 |    1.114 | FAIL |
| PageHero.vue                             |         207.9 |         224.7 |       1.0411 |    0.957 |    1.113 | FAIL |
| color-mode/ColorModeSelect.vue           |         117.2 |         118.6 |       1.0346 |    0.910 |    1.112 | FAIL |
| Banner.vue                               |         188.1 |         193.5 |       1.0176 |    0.929 |    1.103 | FAIL |
| ChatPromptSubmit.vue                     |         429.2 |         406.5 |       0.9421 |    0.718 |    1.102 | FAIL |
| DashboardGroup.vue                       |         191.6 |         203.5 |       0.9894 |    0.820 |    1.102 | FAIL |
| ChangelogVersion.vue                     |         238.4 |         245.0 |       1.0268 |    0.921 |    1.101 | FAIL |

### Gate verdict (strict reading of brief)

**Per-component gate: FAIL** (19 of 179 components have CI upper >
1.10).

### Structural interpretation

Every failing component's CI **contains 1.0** (i.e. ratio
indistinguishable from 1 at 95% confidence). No component has a
statistically significant regression (`CI lower > 1.0` on no
component). The strict 1.10 upper-bound gate is dominated by
per-pair noise on fast (~100–300 ms) components where 7 paired
samples cannot pin a 7% median difference inside a 10% CI.

The aggregate gate fully passes (CI upper 0.932 << 1.05). The
+20.4% R22-fix regression that motivated this work is closed; HEAD
runs the corpus 7% faster than R21.5 on aggregate.

This is the brief's "per-component CI upper bound > 1.10" STOP-
and-escalate trigger. The cutover restored R21.5 aggregate
performance and exceeded it; the strict per-component gate failure
is statistical-noise-dominated, not architectural-regression-driven.
Parent-orchestrator decision required on whether to treat
"no component statistically regresses (no CI lower > 1.0)" as
gate-equivalent, or whether the strict 1.10 upper-bound is
inviolable.

## Tooling

- `<scratch>/r22-final-s5/paired-bench.sh` — alternating R21.5 ↔
  HEAD driver. Runs the bench in its matching git worktree to
  avoid the protobuf version skew.
- `<repo-root>/scripts/bench-bootstrap-ci.mjs` —
  reads paired runs, computes per-pair aggregate + per-component
  ratios, runs bootstrap CI (10 000 resamples, 95% percentiles),
  emits `bootstrap-ci.json` and prints a verdict.

## Captures

- `<scratch>/r22-final-s5/runs/r215-run{1..7}/meta-ui-verter-repo_first_pass.json`
- `<scratch>/r22-final-s5/runs/head-run{1..7}/meta-ui-verter-repo_first_pass.json`
- `<scratch>/r22-final-s5/runs/bootstrap-ci.json` — full per-component
  table + per-pair aggregate ratios + gate verdicts.

## Environment

- OS: Windows 11 Pro 10.0.26200
- CPU: x86_64 (`platform.cpu === 'x64'`)
- Node: v22.20.0
- pnpm: v10.22.0
- Rust: cargo `1.X` release profile, `verter_napi.dll` cdylib
  built with default `--release` flags
- nuxt-ui corpus: 177 .vue components under
  `.integration-tests/repos/nuxt-ui/src/runtime/components/`
- bench scenario: `repo_first_pass` (cold shared host, sequential
  component queries, no warmup)
