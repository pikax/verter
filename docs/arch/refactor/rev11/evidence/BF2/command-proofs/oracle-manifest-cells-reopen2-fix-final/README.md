# Final-candidate execution (second-reopen fix tree) — BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE

Real execution evidence for the two locked `performance-gates.toml` cells,
run against the second-reopen FIX candidate (session-recorded
`candidate_head` a78dd9a7f — the exact tree after every fix item landed;
the tree under review differs from it only by this evidence directory and
the final report record). This session REPLACES the prior
`oracle-manifest-cells-final-candidate` session, whose evidence was ruled
invalid (wrong-SHA binding / insufficient exclusivity — concurrent review
activity was running during it). It makes no claim about, and does not
supersede, any cell other than these two; the separate
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` row remains OPEN under
its existing debt disposition (`../../debt-BF2-perf-gate-deferred.md`).

## Exclusive-lease verification (the invalidation cause, addressed head-on)

Verified IMMEDIATELY before the session start, by direct process
inspection (`ps aux` sorted by CPU, sampled repeatedly over several
minutes), not assumption:

- Zero builds, test suites, benchmarks, or other agent/review sessions
  running. The only other Claude session on the machine was idle
  (intermittent ≤10% of one core, no spawned work).
- Spotlight (`spotlightknowledged.updater`) was initially indexing this
  session's own freshly-created files (bursts to ~50–80% of one core); the
  session was DELAYED until six consecutive 5-second samples read 0.0%.
- Residual disclosed load: `RustDesk` (a resident remote-desktop daemon,
  not stoppable from this session) idling at ~10–30% of ONE core on an
  8-core machine, plus WindowServer/Terminal UI noise. Nothing else above
  20% of a core at any sampled point.
- AC power ("AC attached"), load averages ~1.9/8 cores at session start.

Post-hoc corroboration that the window was genuinely quiet: this
session's wall-time coefficient of variation is 1.2565% — TIGHTER than
the frozen BF1 baseline session's own 2.3388% and the tightest of any
session recorded for these cells. A perturbed session shows the opposite.

## Measured subject (exact, pinned — matches both cells' corpus_fingerprint)

- Tool: `generate-official-case-manifests.mjs`, git blob
  `b61404de48e8ba86767a09414195b67a06ac56be` (logged in-session;
  unmodified).
- Vue source checkout HEAD `3adb225775c9b28223a56e07f7a2f874b6fbb138`;
  Svelte source checkout HEAD `44a7813730579b94004e182e5a67aab27aa9d2a6`
  (both asserted by the tool's own `assertCheckout` and logged; both
  checkouts verified clean via `git status --porcelain` pre-session).
- `--vue-modules`: a FRESH scratch `node_modules` realized for this
  session by `npm ci --offline --ignore-scripts` against the committed
  `oracles/vue/package-lock.json`, from the provisioned local npm cache.
- Sandbox: the committed deny-network profile (git blob
  `5d41a32d8ba2ac7bfe905d87b406ea8f234de519`, copied from
  `../../../framework-conformance/command-proofs/bf2-oracle-manifest-generate/`
  and verified by `git rev-parse` before copying) wrapping every measured
  invocation. Live in-session control: `curl` to a real host FAILED
  (exit 6, DNS denied) under the identical profile.
- Node v20.20.2, macOS, 10 runs, `/usr/bin/time -l`.
- Driver: `run-session.sh` in this directory — byte-identical to the
  prior session's driver except the scratch path (single-line diff).

## Correctness oracle — applied live on EVERY run (mismatch aborts)

All 10 runs: stdout exactly `{"vue_rows":2003,"svelte_rows":3457}` AND
byte-identical `diff` of both produced TSVs against the committed
`vue-official-cases.tsv` / `svelte-official-cases.tsv` — the files whose
SHA-256 are both cells' `output_oracle` values. Byte-identity reproduces
every locked work counter (Vue: 5 suites — compiler-core 570,
compiler-dom 137, compiler-sfc 509, compiler-ssr 134, compiler-vapor 653;
2003/2003 `blocked`. Svelte: 22 suites; 3313 `blocked` + 144
`not_applicable`).

## Raw measurements (session-raw.txt, verbatim driver log)

| run | wall (s) | peak RSS (bytes) |
|---|---|---|
| 1 | 23.12 | 101,974,016 |
| 2 | 22.76 | 102,973,440 |
| 3 | 22.61 | 101,826,560 |
| 4 | 22.63 | 101,531,648 |
| 5 | 22.53 | 101,236,736 |
| 6 | 22.65 | 102,203,392 |
| 7 | 23.00 | 102,072,320 |
| 8 | 23.02 | 101,285,888 |
| 9 | 22.86 | 101,990,400 |
| 10 | 23.52 | 101,580,800 |

Wall: median 22.81 s, mean 22.87 s, min 22.53, max 23.52; population
stddev 0.2874 s, CoV 1.2565%.
Peak RSS: max 102,973,440 bytes, mean 101,867,520; population stddev
483,439.1, CoV 0.4746%.

Session window: 2026-08-12T21:15:51Z → 2026-08-12T21:19:41Z (UTC).

## Verdict against the locked cell metrics (BOTH cells share this stream)

| metric | locked limit | this session | result |
|---|---|---|---|
| `wall_ns` median, absolute_max | 45,000,000,000 ns (45 s) | 22,810,000,000 ns | PASS |
| `wall_ns` median, no_regression ≤ 4.6776% over frozen median 24.22 s (ceiling 25.352 s) | ≤ 25.352 s | 22.81 s (−5.82% vs baseline) | PASS |
| `peak_rss_bytes` max, absolute_max | 402,653,184 | 102,973,440 | PASS |
| `peak_rss_bytes` max, no_regression ≤ 4.5700% over frozen max 104,316,928 (ceiling 109,084,412) | ≤ 109,084,412 | 102,973,440 (below baseline max) | PASS |
| work counters (both cells) | exact | byte-identical TSVs, all 10 runs | PASS |
| `output_oracle` (both cells) | exact SHA-256 | byte-identical to the committed files carrying those digests | PASS |
| zero network | dns/socket attempts = 0 | sandbox-denied whole process tree + live curl control failed | PASS |

Both `BF2_VUE_ORACLE_MANIFEST_GENERATE` and
`BF2_SVELTE_ORACLE_MANIFEST_GENERATE` PASS on real execution against the
final fix candidate, bound to that candidate's exact HEAD, with 10
samples, under a verified-quiet machine. No threshold, cell definition,
or `performance-gates.toml` content was changed by this session.
