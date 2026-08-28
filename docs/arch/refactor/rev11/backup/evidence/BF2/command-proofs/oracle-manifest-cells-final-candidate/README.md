# Final-candidate execution — BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE

Real execution evidence for the two locked `performance-gates.toml` cells,
run against the final candidate tree (session recorded `candidate_head`
d4a6eda7d; the tree under review differs from it only by this evidence
directory and the final report/test-suite records — the measured tool and
all measured inputs are byte-identical at both trees). This replaces
cell-existence/schema-validity as the pass evidence: the cells were
EXECUTED and the raw session output is committed here (`session-raw.txt`;
driver `run-session.sh`, adapted from the BF1 session driver at
`../../../framework-conformance/command-proofs/bf2-oracle-manifest-generate/`
with only path updates).

The separate `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` row is NOT
covered here and remains OPEN under its existing debt disposition
(`../../debt-BF2-perf-gate-deferred.md`) — untouched by this session.

## Measured subject (exact, pinned — matches both cells' corpus_fingerprint)

- Tool: `generate-official-case-manifests.mjs`, git blob
  `b61404de48e8ba86767a09414195b67a06ac56be` (verified via `git rev-parse`
  in-session; unmodified).
- Vue source checkout HEAD `3adb225775c9b28223a56e07f7a2f874b6fbb138`;
  Svelte source checkout HEAD `44a7813730579b94004e182e5a67aab27aa9d2a6`
  (both asserted by the tool's own `assertCheckout`, and logged).
- `--vue-modules`: scratch `node_modules` from `npm ci --offline
  --ignore-scripts` against the committed
  `oracles/vue/package-lock.json` (git blob
  `0dd0269c4caff6f449315e1f70e44f7f23e20944`).
- Sandbox: the committed deny-network profile, git blob
  `5d41a32d8ba2ac7bfe905d87b406ea8f234de519`, wrapping every measured
  invocation; live control in-session: `curl` to a real host FAILED
  (exit 6, DNS denied) under the identical profile.
- Node v20.20.2, macOS, 10 runs, `/usr/bin/time -l`.

## Correctness oracle — applied on EVERY run (live, would abort the session)

All 10 runs: stdout exactly `{"vue_rows":2003,"svelte_rows":3457}` AND
byte-identical `diff` of both produced TSVs against the committed
`vue-official-cases.tsv` / `svelte-official-cases.tsv` — whose SHA-256
(`30123a6d…95bd` / `c251be5b…7a8e`) are exactly both cells'
`output_oracle` values. Byte-identity therefore also reproduces every
work counter both cells lock (Vue: 5 suites — compiler-core 570,
compiler-dom 137, compiler-sfc 509, compiler-ssr 134, compiler-vapor 653;
2003/2003 `blocked`. Svelte: 22 suites; 3313 `blocked` + 144
`not_applicable`).

## Raw measurements (session-raw.txt)

| run | wall (s) | peak RSS (bytes) |
|---|---|---|
| 1 | 22.78 | 100,352,000 |
| 2 | 22.72 | 102,875,136 |
| 3 | 23.21 | 101,974,016 |
| 4 | 23.24 | 100,401,152 |
| 5 | 22.93 | 100,253,696 |
| 6 | 23.25 | 99,467,264 |
| 7 | 23.03 | 100,614,144 |
| 8 | 23.12 | 101,318,656 |
| 9 | 23.03 | 100,171,776 |
| 10 | 23.13 | 101,777,408 |

Wall: median 23.075 s, mean 23.044 s, min 22.72, max 23.25.
Peak RSS: max 102,875,136 bytes, mean 100,920,524.8.

## Verdict against the locked cell metrics (BOTH cells share this stream)

| metric | locked limit | candidate | result |
|---|---|---|---|
| `wall_ns` median, absolute_max | 45,000,000,000 ns (45 s) | 23,075,000,000 ns | PASS |
| `wall_ns` median, no_regression ≤ 4.6776% over frozen median 24.22 s (ceiling 25.352 s) | ≤ 25.352 s | 23.075 s (−4.7% vs baseline) | PASS |
| `peak_rss_bytes` max, absolute_max | 402,653,184 | 102,875,136 | PASS |
| `peak_rss_bytes` max, no_regression ≤ 4.5700% over frozen max 104,316,928 (ceiling 109,084,412) | ≤ 109,084,412 | 102,875,136 (below baseline) | PASS |
| work counters (both cells) | exact | byte-identical TSVs, all 10 runs | PASS |
| `output_oracle` (both cells) | exact SHA-256 | byte-identical to committed files carrying those digests | PASS |
| zero network | dns/socket attempts = 0 | sandbox-denied whole process tree + live curl control failed | PASS |

Both `BF2_VUE_ORACLE_MANIFEST_GENERATE` and
`BF2_SVELTE_ORACLE_MANIFEST_GENERATE` PASS on real final-candidate
execution. No threshold, cell definition, or `performance-gates.toml`
byte was changed.
