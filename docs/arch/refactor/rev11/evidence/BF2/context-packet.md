# BF2 landing context packet

- Block: BF2 (framework-conformance-harness)
- Landed commit: 9932ae15e634b930b74ba0c63239768e019bbcab (fast-forward, single squashed commit)
- Tree: 30fb53f181ac0be2a6c860bd67c714353f12728a
- Base: 6a3ff0f761d38a1c91bcb764635c292aee944e07
- Scope: new package packages/framework-conformance-harness/, evidence docs, performance-gates.toml
  insertion (BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE), pnpm-lock.yaml update. Zero .rs files
  touched.
- Reviews: 3 independent blind mandates against candidate commit 489e5eba8:
  - Conformance: PASS
  - Architecture: PASS
  - Adversarial: BLOCKING_FINDINGS (1 blocking, 1 non-blocking, 1 informational) — README overclaimed
    hydration-pairing #1 self-test coverage for code with zero call sites.
- Fix cycle: one bounded doc-only fix in the original worktree correcting README wording for hydration
  pairing #1 and Svelte SSR self-test coverage claims. No source/test logic changed. Re-verified
  `pnpm --filter @verter/framework-conformance-harness test` green (43 passed / 6 skipped) after fix.
- Landing: squashed 6 WIP commits into one (9932ae15e), fast-forward onto program/architecture-lock
  (no merge commit, accepted_sha == candidate_sha post-fix, no landing-equivalence proof required).
