# BF2 reopen #1 — pre-dispatch context packet

Authored BEFORE any implementation change in this pass, as the first commit on
`work/bf2-reopen-fix`. This is the pre-dispatch record of scope and intent for
the reopened BF2 fix pass — not a post-hoc summary.

## Why this reopen exists

A Codex Sol xhigh parallelism consult independently re-examined BF2's exit
criteria and found the prior 3/3 PASS review verdict was wrong on all three
review mandates (conformance, architecture, adversarial). The prior
`accepted_sha`/`accepted_tree` (9932ae15e / 30fb53f18) was invalidated by
`docs/arch/architecture-lock/ledger/program-state.toml` at commit `0c0c6bc78`,
which this branch is based on. See that commit's ledger notes for the full
finding text.

## Scope of this pass (ratified — items 2, 3, 4 only)

Item 1 (the performance-gate freeze / `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE`
cell) is EXPLICITLY OUT OF SCOPE for this pass — it is being resolved separately
via a dedicated architecture consult. `performance-gates.toml` is not touched here.

### Item 2 — manifest/source proofs must actually run and pass, not skip

The prior candidate shipped 43 passed / 6 skipped. The 6 skips are exactly the
required FC-MANIFEST-001 proofs: pinned-source-drift tests
(`test/drift-refusal.spec.mjs`), Vue/Svelte runner re-enumeration and
corrupted-locator discrimination (`test/coverage.spec.mjs`). They skip because
`BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` point at real pinned git checkouts of Vue
3.6.0-rc.3 and Svelte 5.56.8 that were never present in a bare worktree.

Intent: obtain real hermetic pinned checkouts (matching `src/domain-pin.mjs`
commit/tree exactly, verified via `git rev-parse`), point the env vars at
them, and get all 6 tests to execute and pass for real — not stay skipped.
Strengthen `coverage-report.mjs`'s re-enumeration check to verify case/title
hash, profile, and product-evidence fields per manifest row, not just
file/directory existence. Finally, use the now-fully-working harness to
classify the Vue (2003 rows) and Svelte (3313 blocked + 144 not_applicable
rows) manifests for real, per AMD-005 — mechanically via
`bin/coverage-report.mjs`/`bin/generate-goldens.mjs`, writing real evidence
IDs, not fabricated completion. If the full ~5460-row set cannot be honestly
completed in this pass, the packet commits to leaving an explicit, honest
accounting of what remains and why.

### Item 3 — harness/normalizer gaps

Intent: close four concrete gaps found by the reopen finding —
(a) `src/compare.mjs` real-package link validity currently only calls
`require.resolve()`, which would pass a genuinely-missing named export;
strengthen it to check the actual imported names exist on the resolved
module's exports.
(b) Svelte SSR execution (`src/execute-svelte-runtime.mjs`) has zero
self-test; add one mirroring the existing Vue self-test pattern in
`test/failure-detection.spec.mjs` (throws-on-error + succeeds-on-real-SSR).
(c) Both hydration-control entry points (`hydrateVue`, `hydrateSvelteClient`
in `src/hydration.mjs`) have zero test/CLI callers; add real self-tests
driving each against real golden SSR output, and correct the README's
"implemented but not yet exercised" wording once true exercise exists (or
leave the honest wording if genuinely out of reach this pass).
(d) The normalizer suite (`test/normalizer-mutations.spec.mjs`) lacks
distinct mutation tests for several forbidden categories (import/export
source, events, component calls, slots, authored/public names, control-flow);
add them following the file's existing real-mutation-plus-detection pattern.

### Item 4 — evidence custody

This packet itself is the fix for the "context packet was authored
post-implementation" half of the finding — it is committed first, before any
other change, so its position in git history is the proof it is pre-dispatch.
The other half — durable digest-addressed storage of full review-report
content — is a process instruction for the reviewers who evaluate this pass's
output: they must commit their full report text into the repository (e.g.
under `docs/arch/refactor/rev11/evidence/BF2/reviews/`), not leave it only in
an ephemeral worker worktree. This implementer pass does not itself produce
those reports.

## Non-goals / explicit exclusions for this pass

- No `.rs` file is touched.
- `performance-gates.toml` and the
  `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` cell are not touched.
- No package outside `packages/framework-conformance-harness/` is modified,
  except this evidence document and the harness's own `README.md`.
- No scope creep beyond items 2–4 as enumerated above.

## Verification plan

`pnpm --filter @verter/framework-conformance-harness test` must go from
43 passed / 6 skipped to a run where the 6 previously-skipped tests, plus the
new tests added for item 3, all execute and pass. Full command output is
captured in the final report at `.agent-run/BF2-FIX-REPORT.md` (gitignored,
not part of the durable evidence trail).
