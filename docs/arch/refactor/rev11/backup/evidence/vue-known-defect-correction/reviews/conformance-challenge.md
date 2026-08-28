# Conformance challenge — round 1

**Reviewed candidate:** commit `1afe1bb76869dbad700477a9d2c41bdc80f9981d`, tree
`b7733b55f191cd86a17940596c0ae2beba46f5b2`, branch `work/bv0-vue-correction-amendment`.
**Reviewer:** independent Codex xhigh session, read-only sandbox. Did not author the
candidate.

## Verdict

`BLOCKING_FINDINGS`

## Findings

1. `AMD-006 §4:79`, `BF3.md:13`, `BF3.md:38` — the amendment affirmatively preserves
   typed non-success, artifact withholding, guards, and whole-cell retraction for
   wrong-but-successful Svelte cells, read as violating the "fix, don't guard"
   standing rule as applied to BF3's narrowed Svelte scope.
2. `bf3-safety-retraction-scope.md:11,20`, live `program-state.toml:318` — the
   (at review time) untouched scope document still ordered Vue VDOM/Vapor/SSR
   probing and `BF3-RET-VUE-*` retraction, and the live ledger still cited that
   document as execution authority, contradicting the narrowed `BF3.md`.
3. `evidence/framework-conformance/validate-package.mjs:225` — the package validator
   still requires 56 blocks and the old DAG edges/digest, rejecting the 57-row DAG.
4. Live `program-state.toml:14,299` — old DAG digest, missing `BV0` record.

## Confirmed clean

The cited 36-cell Vue seed count is grounded in BF2's committed golden fixture
matrix (3 fixtures × 3 backends × maps on/off × dev/prod); `version-domain.md` and
`domain-pin.mjs` pin `svelte@5.56.8`, and `svelte-golden-lib.mjs:32` is confirmed
still pinned to `5.56.3`; the BF2 goldens contain 6 Svelte client + 6 Svelte server
cells; every cited implementation location (`MacroState.has_expose`, the VDOM slot
caching/separator paths, `ServerGenerate`, the version pins) is present and
substantively accurate; BV0 routes acceptance through the existing BF2 comparator and
leaves AMD-005's oracle/exclusion/normalizer/capability/performance locks untouched;
§8's ratification blockquote is byte-identical to the ratified text; the worktree was
clean and matched the requested commit/tree exactly.

## Preparer disposition (not part of the independent review)

- Finding 2 is ACCEPTED and fixed: `bf3-safety-retraction-scope.md` narrowed in a
  follow-up commit to remove the Vue VDOM/Vapor/SSR probe-matrix rows and the
  `BF3-RET-VUE-*` convention, matching `BF3.md`'s narrowed scope.
- Findings 3 and 4 are REJECTED as false positives, for the same reason recorded in
  the architecture-challenge report: `validate-package.mjs` is AMD-005's own frozen,
  already-ratified package validator, not a living gate; the live ledger transition is
  an explicitly deferred, separate follow-up commit, not part of this package.
- Finding 1 is REJECTED on the same architecture ruling recorded in the
  architecture-challenge report (RETROACTIVE-NO-FORWARD-ONLY): AMD-005's ratified BF3
  mechanism stays intact for its retained Svelte/non-Vue-runtime inventory; "fix,
  don't guard" governs BV0 and work outside BF3's already-ratified retained scope,
  not BF3's own unchanged mechanism. No candidate text change follows.
