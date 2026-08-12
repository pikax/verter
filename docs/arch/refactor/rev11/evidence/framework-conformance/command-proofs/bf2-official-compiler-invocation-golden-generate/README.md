# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE measurement session

This directory is the raw evidence for the performance-gates.toml cell
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE`, freezing the row that
`performance-impact.md` left deliberately open pending BF2's own harness
existing. This measures BF2's actual test-execution-harness golden generator
(`packages/framework-conformance-harness/bin/generate-goldens.mjs`) — the
real "invoke the official Vue/Svelte compilers to produce immutable golden
output" workload, not the BF1 manifest-enumeration tool the sibling
`BF2_VUE_ORACLE_MANIFEST_GENERATE` / `BF2_SVELTE_ORACLE_MANIFEST_GENERATE`
cells already measure.

## What was measured

One script, one invocation per run: `node bin/generate-goldens.mjs` with no
flags (the full regenerate path, not `--check`). It compiles 6
independently-authored fixtures (3 Vue, 3 Svelte) across every declared
coverage axis — Vue: 3 fixtures × `vdom`/`vapor`/`ssr` backend × source-map
on/off × dev/prod = 36 golden records; Svelte: 3 fixtures ×
`client`/`server` generate target × dev/prod = 12 golden records — through
the pinned official `@vue/compiler-sfc`+backends and `svelte/compiler`
packages, computes each cell's raw/normalized digests, and writes 48
immutable golden JSON records under `goldens/`.

## Inputs (exact, pinned)

- Harness script: git blob (see `session-raw.txt`'s `script_blob` field,
  reproduced fresh at measurement time), unmodified during the session.
- Oracle packages: the exact `vue@3.6.0-rc.3` / `@vue/compiler-{core,dom,
  sfc,ssr,vapor}@3.6.0-rc.3` / `@vue/{runtime-core,runtime-dom,runtime-
  vapor,server-renderer,reactivity,shared}@3.6.0-rc.3` /
  `svelte@5.56.8` devDependencies of
  `packages/framework-conformance-harness/package.json`, pinned exactly (no
  ranges), cross-verified at every invocation against
  `docs/arch/refactor/rev11/evidence/framework-conformance/oracles/{vue,svelte}/package-lock.json`
  by `src/package-pin.mjs`'s three-layer drift check (see
  `test/drift-refusal.spec.mjs`).
- Fixtures: the 6 independently-authored files under
  `packages/framework-conformance-harness/fixtures/`.

## Zero-network enforcement

Every measured invocation ran under `sandbox-exec -f deny-network.sb` (this
directory), the identical macOS Seatbelt profile BF1 used for its own
manifest-generation measurement. Verified operationally in this session: a
`curl` to a live host fails with "Could not resolve host" (DNS itself
denied) under the identical profile, while all 10 measured invocations
completed successfully and produced a byte-identical combined golden digest.
This is the same operational proof `test/offline-execution.spec.mjs`
exercises as a standing regression test.

## Correctness oracle applied on every run

Every one of the 10 runs was checked for:

- stdout `{"goldens_written":48}` (exact match), and
- an identical combined SHA-256 (`cat goldens/vue/*.json goldens/svelte/*.json | shasum -a 256`)
  to the pre-session reference digest, itself produced by `node
  bin/generate-goldens.mjs --check` passing against the already-committed
  goldens immediately before the session began.

All 10 runs passed both checks (`session-raw.txt`, every line
`status=OK digest_ok=true`). `run-session.sh` exits 1 on any mismatch — the
oracle was live, not decorative.

## Sample count and policy applied

Each run is a full cold external-process invocation (Node process start,
oracle package resolution, 6 fixture parses, 48 compiler invocations across
Vue's 3 backends and Svelte's 2 targets, 48 normalizer passes for the
embedded digest). Unlike BF1's ~24s manifest-enumeration tool, this workload
is short (~0.3s median) — 10 full cold invocations complete in seconds, so
`[statistics].long_min_runs = 10` is used for direct comparability with the
sibling BF2 cells' methodology, not because the workload is expensive per
run. No sample was discarded (`outlier_policy: no_discretionary_exclusion`).

## Raw measurements (`session-raw.txt`)

| run | wall (s, `/usr/bin/time -l` real) | peak RSS (bytes) |
|---|---|---|
| 1  | 0.31 | 116,490,240 |
| 2  | 0.27 | 116,736,000 |
| 3  | 0.30 | 115,687,424 |
| 4  | 0.30 | 118,767,616 |
| 5  | 0.30 | 114,704,384 |
| 6  | 0.30 | 116,686,848 |
| 7  | 0.34 | 116,916,224 |
| 8  | 0.30 | 117,276,672 |
| 9  | 0.32 | 118,652,928 |
| 10 | 0.30 | 115,556,352 |

Wall (seconds): median = 0.30, mean = 0.304, population stddev =
0.0168523, coefficient of variation (stddev/mean) = 5.5435%.

Peak RSS (bytes): median = 116,711,424, mean = 116,747,468.8, population
stddev = 1,216,837.15, coefficient of variation = 1.0423%.

**Disclosed precision note.** `/usr/bin/time -l`'s `real` field is only
2-decimal-precision (10ms buckets) for a sub-second workload, so the wall
CoV above is materially inflated by timer-quantization noise relative to a
multi-second workload like the sibling manifest-generation cells — stated
explicitly rather than left implied, per the same disclosure discipline
those cells' own README applies. The relative wall gate below is
correspondingly looser than the sibling cells', which is the honest
consequence of measuring a fast workload at this timer resolution, not a
weakened bar.

## Threshold derivation

**Wall time, absolute (`wall_ns`, `absolute_max`):** this generator runs a
handful of times per BF2 harness-evidence cycle, not on any hot or
per-request path; a sane CI-friendly product budget is "well under a few
seconds". 5,000,000,000 ns (5s) is ~16.7x the measured median (0.304s) and
~14.7x the measured max (0.34s) — real headroom, catastrophe-stop only
(e.g. an accidental retry loop or an O(fixture²) blowup as more fixtures are
added later), matching A6/BF1's philosophy that the absolute bound is a
fence, not the tight fit.

**Wall time, relative (`wall_ns`, `no_regression_percent_max`):**
`max(3.000%, 2 x 5.5435%) = 11.0870%`. Stated at the precision the 10-sample
measurement actually supports (see precision note above), not rounded down.

**Peak RSS, absolute (`peak_rss_bytes`, `absolute_max`):** 402,653,184 bytes
(384 MiB) is ~3.39x the measured max (118,767,616 bytes) — generous,
catastrophe-stop only, independently derived here (not copied from the
sibling cells) but landing at a comparable order of magnitude because the
underlying process shape is comparable (one short-lived Node.js process,
no heavy allocation).

**Peak RSS, relative (`peak_rss_bytes`, `no_regression_percent_max`):**
`max(3.000%, 2 x 1.0423%) = 3.000%` (the 3% floor governs; the measured
2x-CoV term is smaller).

## Work counters (exact, from the committed goldens and cross-checked
against every run's regenerated output in this session)

- `goldens_written.count`: 48 (both `absolute_max` and `absolute_min` — a
  generator that silently produced more or fewer records than the fixture ×
  axis product is a real correctness regression, not "more work than
  expected").
- `vue.goldens.count`: 36 (3 fixtures × 3 backends × 2 sourceMap × 2 isProd).
- `svelte.goldens.count`: 12 (3 fixtures × 2 generate targets × 2 dev).

## Reproduction

```sh
# once: pnpm install already resolves the exact pinned oracle packages as
# this package's own devDependencies (packages/framework-conformance-harness/package.json)
pnpm install --filter @verter/framework-conformance-harness...

./run-session.sh 10
```

`run-session.sh` in this directory is the exact driver used; it does not
modify `bin/generate-goldens.mjs`.
