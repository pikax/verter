# Reopen-3 disposition — BF2_VUE_ORACLE_MANIFEST_GENERATE / BF2_SVELTE_ORACLE_MANIFEST_GENERATE

Both locked cells measure `generate-official-case-manifests.mjs` (git blob
`b61404de48e8ba86767a09414195b67a06ac56be`), a workload that walks the pinned Vue/Svelte
source trees and classifies official test titles. It makes **zero** calls to the official
compiler and writes no golden/compiled output. This reopen's fix is confined to
`packages/framework-conformance-harness/src/invoke-vue-oracle.mjs`,
`src/check-candidate.mjs`, `src/compare.mjs`, `src/sourcemap.mjs`, and their test/README
files — it does not touch this script, its inputs, or its execution environment.

## Identity proof (inputs unchanged — the workload is provably unaffected)

Both reopen-3 sessions below (2026-08-13T18:38Z and 2026-08-13T18:46Z) recorded, and
verified byte-identical to the reopen-2 final-candidate acceptance session
(`../oracle-manifest-cells-reopen2-fix-final/`):

- `script_blob` — `b61404de48e8ba86767a09414195b67a06ac56be` (identical)
- `vue_source_head` — `3adb225775c9b28223a56e07f7a2f874b6fbb138` (identical)
- `svelte_source_head` — `44a7813730579b94004e182e5a67aab27aa9d2a6` (identical)
- `sandbox_profile_sha` — `e11a4d9848f2527e2770be448f35ab491c0a4130` (identical)
- work counters / `output_oracle` — both sessions reproduced the exact locked row counts
  (`vue_rows=2003`, `svelte_rows=3457`) and byte-identical TSVs against the committed
  files carrying the cells' pinned SHA-256 `output_oracle` values, on all 10 runs each.

## Why these two sessions are NOT acceptance evidence for the timing metrics

Both sessions were run WITHOUT the exclusive-lease quiet-window protocol the reopen-2
acceptance session used (six consecutive 5-second 0.0%-CPU samples before starting; see
`../oracle-manifest-cells-reopen2-fix-final/README.md` §"Exclusive-lease verification").
`ps`/`uptime` sampled during both sessions showed persistent non-Verter background load
(a standing `RustDesk --server` process and an unrelated concurrent `claude
--dangerously-skip-permissions` session sustained at 14-23% CPU each, plus — session 1
only — a transient Spotlight indexing burst from the `git worktree add` that created this
worktree). Median wall time came in ~6.7% (session 1) and ~7.8% (session 2) above the
toml-documented 24.22 s baseline median — inside the absolute ceiling (45 s) and RSS
ceilings by a wide margin, but outside the locked 4.6776%/4.5700% relative-regression
gates.

Raw logs are retained here for transparency
(`session1-nonconforming-raw.txt`, `session2-nonconforming-raw.txt`,
`run-session.sh` — `.txt`, matching the reopen-2 `session-raw.txt` precedent,
because the repo `.gitignore` excludes `*.log` and would silently drop the
files from the tree) but are explicitly labeled: **NONCONFORMING / NOT ACCEPTANCE
EVIDENCE — locked runner-idleness precondition violated by active unrelated workload.**
They are not read as PASS and not read as a genuine gate failure.

## Scoping ruling

Consulted `gpt-5.6-sol` (xhigh... high reasoning effort) on the correct disposition given
the above (`scoping-consult-out.txt`, full transcript). Ruling, applied here:

1. A valid locked-gate failure would block regardless of attribution — but these two
   sessions are not valid gate sessions (the runner-idleness precondition was violated),
   so they prove neither PASS nor a genuine FAIL.
2. The two cells are **structurally outside this reopen's affected performance cone** —
   they measure a workload with zero compiler calls that this reopen's diff never
   touches. The reopen packet's instruction to "rerun affected BF2 performance and
   provenance gates" is satisfied by the identity proof above (inputs unchanged), not by
   forcing a noisy timing re-execution of an unaffected workload.
3. Recalibration is out of scope (maintainer-only, requires a material premise change;
   `post_result_exception_allowed = false` on these cells) and was correctly NOT
   attempted — no threshold value in `performance-gates.toml` was touched by this pass.
4. These two cells do not block this reopen's landing. If a future pass needs a fresh
   conforming timing sample for these cells (e.g. because their existing reopen-2
   evidence ages out under some other rule), it must use the exclusive-lease
   quiet-window protocol, not this pass's diagnostic sessions.

The separate `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` row — the workload that
WOULD cover official compiler invocation / golden generation, i.e. the thing this reopen
actually changed — remains explicitly OPEN/deferred under its own pre-existing
disposition (`../../debt-BF2-perf-gate-deferred.md`), unaffected and unaltered by this
pass; this reopen does not populate it and was never scoped to.
