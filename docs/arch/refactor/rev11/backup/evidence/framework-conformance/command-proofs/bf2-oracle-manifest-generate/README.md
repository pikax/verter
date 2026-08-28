# BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE measurement session

This directory is the raw evidence for the two performance-gates.toml cells that freeze
the reference measurement of `generate-official-case-manifests.mjs` — the existing,
already-authored, BF1/evidence-preparation tool that produced the committed
`vue-official-cases.tsv` (2003 rows) and `svelte-official-cases.tsv` (3457 rows). This is
NOT a measurement of BF2's future test-execution harness, which does not exist yet and
cannot be measured without violating the charter's "no criterion selected after candidate
measurement" rule. `generate-official-case-manifests.mjs` is BF1-owned tooling, already
frozen by blob identity, and its behavior is fully deterministic — measuring it now is a
legitimate pre-BF2 reference measurement, exactly as A6 measured its own already-existing
harness (`attribution_baseline.rs`) to freeze `A6_META_COMPILE_40_COLD_RUST`.

## What was measured

One tool, one invocation per run, generates BOTH manifests in a single Node process (the
script has no flag to generate only one language — `vueManifest()` and `svelteManifest()`
both run before either TSV is written). The two gate cells therefore necessarily share the
same physical wall-time/peak-RSS measurement stream; they are distinguished by their own
per-language correctness oracle (exact output digest) and per-language work counters, not
by an artificially split timing. This sharing is disclosed in both cells' "WHY THIS IS THE
CELL" comments in `performance-gates.toml` rather than hidden.

**Scope, precisely.** The measured workload is official-case ENUMERATION and
CLASSIFICATION: it walks the pinned Vue/Svelte source trees, extracts a title and
title-hash per official test declaration, and assigns each a disposition
(`blocked`/`not_applicable`), writing the result as TSV manifest rows. It makes ZERO
calls to the Vue or Svelte compiler and produces NO golden/compiled output. This is
NOT a measurement of the wider "invoke the official compilers thousands of times and
produce immutable golden output" workload that BF2's future test-execution harness
will need — that harness does not exist yet, and measuring it now would violate the
charter's "no criterion selected after candidate measurement" rule. That wider
workload stays explicitly open/deferred; see `../../performance-impact.md`'s
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` row.

## Inputs (exact, pinned)

- Vue source checkout: commit `3adb225775c9b28223a56e07f7a2f874b6fbb138` (the script's own
  `EXPECTED_VUE` assertion; `assertCheckout` fails closed on any mismatch or dirty tree).
- Svelte source checkout: commit `44a7813730579b94004e182e5a67aab27aa9d2a6` (`EXPECTED_SVELTE`).
- `--vue-modules`: a scratch `node_modules` produced by `npm ci` from the exact committed
  `docs/arch/refactor/rev11/evidence/framework-conformance/oracles/vue/package-lock.json`
  (git blob `0dd0269c4caff6f449315e1f70e44f7f23e20944`) and its sibling `package.json` (git
  blob `a8d65606ccecf782489baf065d6de3572156603b`) — this is the only external dependency
  the script resolves at runtime (`@babel/parser`, via `createRequire`).
- Harness script itself: git blob `b61404de48e8ba86767a09414195b67a06ac56be`
  (`generate-official-case-manifests.mjs`), unmodified.

## Zero-network enforcement

Every measured invocation ran under `sandbox-exec -f deny-network.sb` (this directory,
git blob `5d41a32d8ba2ac7bfe905d87b406ea8f234de519`), a macOS Seatbelt profile that denies
all `network*` operations for the whole process tree while allowing local file/process
operations (needed for the script's own `git rev-parse`/`git status` subprocess calls).
Verified operationally, not just by source audit: under the identical profile, `curl` to
a live host fails with "Could not resolve host" (DNS itself is denied), while all 10
measured script invocations completed successfully and produced byte-identical output —
proving the script does not depend on network access and performs none. This was
cross-checked statically: `grep -nE "fetch|http|https|net\.|dns\.|axios|child_process"`
against the script shows the only network-capable-looking import is `node:child_process`,
used exclusively for local `git rev-parse`/`git status --porcelain` calls (confirmed by
reading the full script; no `git fetch`/`pull`/`clone`/`ls-remote` call exists in it).

## Correctness oracle applied on every run

Every one of the 10 runs was checked for both:

- stdout `{"vue_rows":2003,"svelte_rows":3457}` (exact match), and
- byte-identical `diff` against the committed
  `docs/arch/refactor/rev11/evidence/framework-conformance/{vue,svelte}-official-cases.tsv`.

The committed files' SHA-256 (recorded independently in `../../validation.md`) were
reproduced from a fresh run in this session:

```
30123a6d88e1e7382afdcc752b5438c3486dd462e59ce831742ad0a3a3dd95bd  vue-official-cases.tsv
c251be5b8b1de3e58c526700c426e2502e8bd1eb1dd622e22119b667adee7a8e  svelte-official-cases.tsv
```

All 10 runs passed both checks (`session-raw.txt`, every line `status=OK`). A run that had
failed either check would have aborted the session (`run-session.sh` exits 1 on mismatch) —
the oracle was live, not decorative.

## Sample count and policy applied

Each run is a full cold external-process invocation (Node process start, git subprocess
spawns, `readdirSync`/`readFileSync` walk of 5 Vue package `__tests__` trees and 22 Svelte
sample suites, `@babel/parser` parse of ~2,003 Vue test declarations) — dominated by
process- and I/O-level constants, not a hot in-process loop. This is the same "cold,
one-shot batch" shape as A6's own `cold_project_batch_component_meta_then_compile_many`
cell, not the shape `runner.control_benchmark`'s 30-iteration in-process Rust loop is. We
therefore applied `[statistics].long_min_runs = 10` rather than `short_min_samples = 30`:
30 full cold invocations at ~24s each would cost roughly 12 minutes for no additional
statistical value over 10, given the low observed variance (below). This is stated
explicitly per `outlier_policy`'s discipline of not leaving the applied policy implied.

No sample was discarded. All 10 samples entered the statistic (`outlier_policy:
no_discretionary_exclusion`). No thermal/interrupt anomaly was observed or excluded.

## Raw measurements (`session-raw.txt`)

| run | wall (s, `/usr/bin/time -l` real) | peak RSS (bytes, "maximum resident set size") |
|---|---|---|
| 1  | 24.05 | 101,367,808 |
| 2  | 24.14 | 101,302,272 |
| 3  | 24.23 | 101,793,792 |
| 4  | 24.21 | 99,876,864 |
| 5  | 24.07 | 104,316,928 |
| 6  | 24.21 | 100,646,912 |
| 7  | 24.26 | 101,433,344 |
| 8  | 25.58 | 102,825,984 |
| 9  | 24.66 | 98,615,296 |
| 10 | 25.63 | 95,420,416 |

Wall (seconds): median = 24.22, mean = 24.504, stddev = 0.5731, coefficient of variation
(stddev/mean) = 2.3388%.

Peak RSS (bytes): median = 101,335,040, mean = 100,759,961.6, stddev = 2,302,374.1,
coefficient of variation = 2.2850%.

## Threshold derivation (mirrors A6's disclosed-margin style)

**Note on the stddev/CoV formula, stated explicitly rather than left implied.** This
session's coefficient of variation uses the POPULATION standard deviation (divide the
sum of squared deviations by `n = 10`, not `n - 1`), NOT the sample-stdev convention, and
NOT A6's own "half the range relative to the mean" noise-floor formula (§3.2 of
`evidence/A6/baseline-measurement.md`). "Mirrors A6's disclosed-margin style" above refers
to the DISCLOSURE discipline (state the derivation and its residue at the precision the
sample size supports, never round toward a more convenient number) — it does not claim
formula identity with A6's half-range statistic. The population divisor is a deliberate
choice, not an oversight: the 10 runs in this session are the complete measured dataset
for this cell (not a sample drawn to estimate some larger hypothetical population), so
`n` rather than `n - 1` measures the actual observed spread of the session that occurred.
The practical effect is STRICTER, not weaker: population stdev is `sqrt(n/(n-1))` times
smaller than sample stdev for the same data, so this choice tightens
`no_regression_percent_max` relative to the sample-stdev alternative — recomputing with
`n - 1` gives `wall` CoV = 2.4653% (`max(3%, 2 × 2.4653%) = 4.9306%`, vs the locked
4.6776%) and `peak_rss` CoV = 2.4086% (`max(3%, 2 × 2.4086%) = 4.8172%`, vs the locked
4.5700%) — both looser than what is actually locked in `performance-gates.toml`. Nothing
in `performance-gates.toml`'s locked values changes as a result of this note.

**Wall time, absolute (`wall_ns`, `absolute_max`):** this tool runs a handful of times per
BF1/BF2 evidence-preparation cycle, not on any hot or per-request path; a sane CI-friendly
product budget is "well under a minute." 45,000,000,000 ns (45s) is ~1.86x the measured
median (24.22s) and ~1.76x the measured max (25.63s) — real headroom above every observed
sample, generous by design (catastrophe stop), matching A6's own philosophy that the
absolute bound is a fence against catastrophic regression (e.g. an accidental retry loop,
an O(n^2) blowup in the directory walk) and the tight fence is the relative bound below.

**Wall time, relative (`wall_ns`, `no_regression_percent_max`):** `max(3.000%, 2 x
2.3388%) = 4.6776%`. Stated at the precision the 10-sample measurement actually supports,
not rounded down, per the same discipline A6's own comment states explicitly.

**Peak RSS, absolute (`peak_rss_bytes`, `absolute_max`):** 402,653,184 bytes (384 MiB) is
~3.85x the measured max (104,316,928 bytes), proportioned similarly to A6's own
~3.6x-over-baseline RSS catastrophe-stop budget (256 MiB against a 74,850,304-byte
baseline). Deliberately generous — RSS is allocator/platform-dependent — with the relative
bound doing the real work.

**Peak RSS, relative (`peak_rss_bytes`, `no_regression_percent_max`):** `max(3.000%, 2 x
2.2850%) = 4.5700%`.

## Work counters (exact, from the committed manifests, cross-checked against every run's
regenerated output in this session)

Vue (`vue-official-cases.tsv`, 2003 data rows):

- 5 suite directories walked: `compiler-core` (570), `compiler-dom` (137), `compiler-sfc`
  (509), `compiler-ssr` (134), `compiler-vapor` (653).
- 2003/2003 rows disposition `blocked` (0 `not_applicable`, 0 anything else) — the
  generator does not resolve dispositions for Vue at all at this stage; a future edit that
  silently started doing so would move this counter without this cell being extended, and
  is exactly the kind of undisclosed scope change the counter exists to catch.

Svelte (`svelte-official-cases.tsv`, 3457 data rows):

- 22 distinct suites enumerated.
- 3313/3457 rows disposition `blocked`, 144/3457 disposition `not_applicable` (the
  `migrate`/`preprocess`/`print` non-sample suites the script classifies out of Verter's
  compiler product boundary — see `SVELTE_NOT_APPLICABLE` in the script).

## Reproduction

```sh
# once: populate a scratch node_modules from the committed oracle lockfile
mkdir -p /tmp/bf1-perf-measure/vue-oracle
cp docs/arch/refactor/rev11/evidence/framework-conformance/oracles/vue/package.json \
   docs/arch/refactor/rev11/evidence/framework-conformance/oracles/vue/package-lock.json \
   /tmp/bf1-perf-measure/vue-oracle/
(cd /tmp/bf1-perf-measure/vue-oracle && npm ci --no-audit --no-fund)

# populate real pinned Vue RC.3 / Svelte 5.56.8 source checkouts at the commits above,
# e.g. via generate-oracle-closures.mjs or a manual `git clone` + `git checkout`.

./run-session.sh 10
```

`run-session.sh` in this directory is the exact driver used; it does not modify
`generate-official-case-manifests.mjs`.
