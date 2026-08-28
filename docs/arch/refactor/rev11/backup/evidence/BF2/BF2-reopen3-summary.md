# BF2 reopen #3 evidence summary

Candidate: `4a0952ce450a5396c33a3f1c98598c65e6afd3f7` (tree `05c0b07be91d7f4539ba023388afb794bac181fc`)
Base: `ed615a96a0f1a811c765ecc2c606026ab6cbb687`

## Reopen trigger

BV0's round-1 adversarial review (`evidence/vue-known-defect-correction/reviews/round1-adversarial.md`)
found `packages/framework-conformance-harness/src/invoke-vue-oracle.mjs` never passed `vapor`/
`templateOptions.ssr`/`isProd` into the official `compileScript` call, so every harness-generated
Vapor/SSR/production golden's script half was compiled by the official compiler under the wrong
profile. A Codex xhigh scoping ruling (`evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md`)
held this defect is BF2-owned (offline official compiler invocation + immutable golden generation is
BF2's owned scope) and required a formal reopen with fresh 3-mandate review before BV0 could proceed.

## Owned-scope verification (charter `docs/arch/refactor/rev11/charters/BF2.md`)

- Offline official compiler invocation now carries the complete requested profile (`vapor`,
  `templateOptions.ssr`, `isProd`, `ssrCssVars`) into `compileScript`, matching the official
  compiler's own derivation — fixed at the option-propagation root, not per-symptom.
- Full golden corpus regenerated under the corrected invocation; every changed record traced to
  the option-propagation fix, classified individually (no candidate-output-driven expectation
  updates — BF2's forbidden-list item is respected).
- New harness-level JS+TS controls for `__vapor`, `defineVaporComponent`, SSR css-vars, and the
  `isProd` axis, each independently plant-red-green mutation tested against the pre-fix invocation.
- Source-map composition restored to a genuine candidate-vs-official structural comparison on
  decoded segments (was narrowed to candidate self-consistency); literal column-precise splice
  controls added across all three backends.
- Candidate acceptance CLI gained an opt-in fail-closed mode (a skipped link/runtime axis becomes
  a hard failure instead of a silent pass) and is vapor-capable end to end.
- No production compiler code and no Svelte file touched by this change (confirmed via diff scope).

## Required exits (`FC-HARNESS-001`, `FC-MANIFEST-001`, `FC-NORMALIZER-001`, locked performance cells)

- Harness self-tests: 279/279 (`pnpm test` in `packages/framework-conformance-harness`), independently
  re-run by the program orchestrator on the landed tree.
- `verter_vue_conformance` corpus/generator/normalizer suite: 8/8, independently re-run by the program
  orchestrator on the landed tree (`cargo test -p verter_vue_conformance --test main`).
- Locked `BF2_VUE_ORACLE_MANIFEST_GENERATE`/`BF2_SVELTE_ORACLE_MANIFEST_GENERATE` performance cells:
  reattested via input-identity proof (the measured workload makes zero compiler calls and is
  provably untouched by this change — script blob/source heads/sandbox profile identical to the
  last passing session); two off-protocol timing sessions under host contention are retained as
  explicitly labeled `NONCONFORMING` evidence, not claimed as passing measurements. Full record:
  `evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/`.

## Review verdicts

Three independently dispatched CLI review workers, round 3 (rounds 1-2 found real BLOCKING/CHANGES
REQUIRED findings against the option-propagation fix and the source-map narrowing, both fixed and
reverified before round 3):

- Conformance: PASS
- Architecture: PASS
- Adversarial: PASS (one non-blocking P3 debt item recorded, no blocking findings)

## Predecessor status

BF1 not reopened — it locked the domain/option contract before the harness existed and its own
inventory already specifies `compileScript.vapor`/`templateOptions` as derived from the canonical
request; BF2 violated a valid BF1 lock, the lock itself was not falsified (Codex ruling,
`evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md` Q1).
