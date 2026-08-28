# BF2 reopen #1 — evidence summary (round 2, post-fallback)

Landed commit: 58d42a65d (fast-forward, program/architecture-lock).
Tree: (computed post-commit, see below).
Base: 0c0c6bc78 (the invalidation commit).

## Reopen resolution

1. **Performance exit** — FALLBACK per maintainer decision. The invalid
   `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` cell (frozen from
   BF2's own measurement of its own implementation) is REMOVED from
   `performance-gates.toml`; the row is explicit OPEN/NOT-YET-LOCKED via
   comment, no `[[cell]]` defined. Tracked as debt:
   `docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md`
   — owner BV1/BS1 (whichever first needs it locked), resolution gate
   before that owner's own perf-lock exit, acceptance ID `FC-PERF-001`,
   ruling reference = Codex Sol xhigh consult (full text preserved at
   `docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md`)
   + maintainer FALLBACK decision.
2. **Manifest/source proofs** — the 6 previously-skipped tests (source-drift
   x3, coverage re-enumeration x2, corrupted-locator x1) now execute for
   real against pinned official checkouts and pass. Re-enumeration
   strengthened to a git-hash content check + closed-set field validation,
   with two NEW discriminating negative-control tests
   (`test/coverage.spec.mjs`) added in round 2 after the adversarial
   reviewer proved the original strengthening had zero test coverage
   (mutation-planted, confirmed fail, reverted, confirmed pass).
3. **Harness/normalizer gaps** — closed: real named-export link-validity
   check, Svelte SSR self-test, both hydration entry points self-tested
   against real golden output, 6 new forbidden-mutation normalizer
   categories.
4. **Evidence custody** — genuine pre-dispatch context packet
   (`context-packet-reopen1.md`, committed before any fix code) plus full
   review-report text for both rounds committed durably under
   `docs/arch/refactor/rev11/evidence/BF2/reviews/`.

## Reviews (round 2, hardened per-criterion-evidence method)

- Conformance: PASS
- Architecture: PASS
- Adversarial: BLOCKING_FINDINGS (one gap — re-enumeration content-hash
  branch had zero discriminating test coverage) → fixed (two new
  mutation-proven negative-control tests) → re-verified locally: 65/65
  package tests pass, both new tests independently confirmed to fail when
  the target branch is disabled and pass when restored.

Final package test run (with pinned checkouts):
`Test Files 10 passed (10)` / `Tests 65 passed (65)`.

Manifest classification (AMD-005): 5316 of 5460 rows honestly remain
`blocked`/evidence_id `-`, attributed to genuine downstream dependency on
BV1/BS1/B2/B3 candidate output per `program-dag.toml` — not fabricated,
not silently dropped. 144 Svelte `not_applicable` rows are pre-existing
BF1 classification, re-verified unchanged.
