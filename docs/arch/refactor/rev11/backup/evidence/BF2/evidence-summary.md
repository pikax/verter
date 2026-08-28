# BF2 evidence summary

Reports (full text retained in worker worktrees at time of writing, summarized here):

1. Conformance review — VERDICT: PASS. Ran the real test suite
   (`pnpm --filter @verter/framework-conformance-harness test`): 9 files, 43 passed / 6 skipped
   (49 total), all skips environment-gated honestly. Required exits verified against real
   execution, not README claims.

2. Architecture review — VERDICT: PASS. Confirmed zero .rs files touched, no package outside
   packages/framework-conformance-harness/ touched (besides evidence docs / performance-gates.toml
   / lockfile), no forbidden third-party corpus references, pinned devDependencies exactly match
   official Vue 3.6.0-rc.3 / Svelte 5.56.8.

3. Adversarial review (perf/memory/stub-hunting) — VERDICT: BLOCKING_FINDINGS, then resolved.
   - Finding 1 (BLOCKING): README claimed hydration pairing #1 "fully exercised" while
     hydrateVue/hydrateSvelteClient have zero call sites in any test/CLI path. FIXED: README
     corrected to state the pairing is implemented but not yet exercised, matching the honest
     phrasing already used for pairings #2/#3.
   - Finding 2 (non-blocking): executeSvelteSsr also untested (asymmetric with Vue path). FIXED
     in the same pass: README now discloses this asymmetry.
   - Finding 3 (informational, not actioned): a redundant (non-weakening) assertion clause in
     assertMutationApplied — logically inert but does not compromise test correctness. Left as-is
     (informational only, not blocking).
   - Independently reproduced perf numbers (median 0.30s wall, ~115-119MiB peak RSS) and a
     byte-exact SHA-256 match on the golden corpus against the frozen
     BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE performance-gate evidence — confirms the
     implementer's frozen threshold was not post-hoc-selected.

Post-fix verification: `pnpm --filter @verter/framework-conformance-harness test` green
(43 passed / 6 skipped) on the squashed candidate commit 9932ae15e634b930b74ba0c63239768e019bbcab.
