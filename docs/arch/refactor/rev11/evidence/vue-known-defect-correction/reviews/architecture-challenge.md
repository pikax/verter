# Architecture challenge — round 1

**Reviewed candidate:** commit `1afe1bb76869dbad700477a9d2c41bdc80f9981d`, tree
`b7733b55f191cd86a17940596c0ae2beba46f5b2`, branch `work/bv0-vue-correction-amendment`.
**Reviewer:** independent Codex xhigh session, read-only sandbox. Did not author the
candidate.

## Verdict

`BLOCKING_FINDINGS`

## Findings

1. `BF3.md:15`, `BF3.md:21`, `AMD-006:94` — BF3 still mandates a pre-publication
   production guard, typed non-success, whole-cell retraction, and removal tracking
   for its retained Svelte scope, which the reviewer read as violating the standing
   "wrong-but-successful output is a bug, never a guard" rule as applied to "anything
   else in this package."
2. `AMD-006:61`, live `program-state.toml:14/320`,
   `evidence/framework-conformance/validate-package.mjs:225,230` — the tracked live
   ledger has no `BV0` row and the old DAG digest, and AMD-005's package validator
   still expects 56 DAG rows and `BF3`-only `B2`/`B3` predecessors, so it rejects this
   candidate's DAG.
3. Live `program-state.toml:318` — the live `BF3` row's notes still describe Vue
   VDOM/Vapor/SSR safety-retraction work.

## Confirmed clean

Commit/tree identity; the DAG diff changes only `BV0` and `B2`/`B3` predecessors as
specified; `BV0.md` and the `BV1.md` preservation edit conform to the ratified text;
the Svelte-version and `ServerGenerate` requirements are exact; §8's ratification
blockquote retains literal placeholders; the deviation memo's repository citations
check out; AMD-005's compatibility-domain, oracle, capability-matrix, and
performance-lock content is untouched.

## Preparer disposition (not part of the independent review)

- Finding 2 and 3 are REJECTED as false positives: `validate-package.mjs` is AMD-005's
  own frozen, already-ratified one-time package validator pinned to AMD-005's exact
  historical DAG identity (56 rows) — it is evidence of that already-landed
  ratification, not a living gate this or any future amendment must keep passing, and
  editing it would rewrite ratified historical evidence. The live ledger is
  out-of-scope for this package by design: ledger transitions land as a separate,
  minimal follow-up commit after this package is reviewed, per this program's
  branch-hygiene rule and this task's explicit division of labor. Both are expected,
  not defects.
- Finding 1 is REJECTED, per an explicit architecture ruling obtained on escalation
  (RETROACTIVE-NO-FORWARD-ONLY): AMD-005's ratified BF3 typed-non-success/
  whole-cell-retraction/removal-owner mechanism stays intact and unchanged — a
  general subsequent rule cannot silently repeal a specific, already-ratified
  charter decision without its own explicit amendment, which was not authorized
  here. BF3 may keep applying its existing mechanism to defects found while
  exhausting its retained Svelte/non-Vue-runtime inventory. "Fix, don't guard"
  (forward-only) governs BV0's Vue findings, findings outside BF3's already-ratified
  retained inventory, new BS1/later-work defects, and any proposed expansion of BF3
  guards beyond assigned correction-owner acceptance — not BF3's existing, unchanged
  mechanism. No candidate text change follows from this disposition.
