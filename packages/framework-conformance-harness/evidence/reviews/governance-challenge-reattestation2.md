# AMD-005 governance/DAG impact-bounded reattestation 2

## Exact identity and scope

- Previous reviewed candidate: `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`
- Previous reviewed tree: `1ff1f83d8e994b6f1169b0b209c9f557c23f4728`
- Reattested fix candidate: `7442bb9060b7faa0720e528d3f96ee1df1abff95`
- Reattested fix tree: `69502487b55f87eb7c0c009876865b64397da660`
- Branch: `work/framework-conformance-rescope`

This is an impact-bounded reattestation of the three blocking findings in
`governance-challenge.md`, plus the requested next-legal-step and changed-file DAG
sanity checks. It is not a fresh review of the full package. The package and canonical
checkout were inspected read-only; the only write is this report.

## Verdict

BLOCKING_FINDINGS

- **Prior finding 2 remains unresolved for the explicitly required architect report.**
  `.agent-run/architect-report.yaml:4-7` still declares base
  `e6035b433352b106957f27f3e97b71911f39f9ae`, base tree
  `2295458ea1aa53930a00d57a86e344a1adbc6c09`, candidate
  `8fbef4ba2ce30d93a636f769639519df7a773a92`, and candidate tree
  `eba511f865239ac27abf7da4fd3b4d292ed9ebec`. It has no superseded marker or current
  reviewed-package binding. Its package facts are also demonstrably from the obsolete
  round: `.agent-run/architect-report.yaml:33-42` still records 22,657 assertions and
  Svelte option coverage `25/25`, whereas `validation.md:38-40,75-80` records the fixed
  22,718-assertion package and expanded 35-row Svelte inventory. The three primary
  reports now correctly bind `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8` / tree
  `1ff1f83d8e994b6f1169b0b209c9f557c23f4728`, and the two older reattestations are
  explicitly marked superseded while preserving their historical `6920ddc6...`
  binding, but that does not make the still-current architect report identity
  consistent. `validate-package.mjs:268-291` checks only the three primary Markdown
  reports, so the successful post-review run does not close this requested identity
  defect.

## Resolved findings and bounded checks

1. **Stale B1 narrative — resolved.** The read-only canonical checkout
   `<repo-root>` is clean on
   `program/architecture-lock` at
   `b3249d13d07806a14a4307954dfcc459cf7301ac`, whose parent is the accepted B1 commit
   `03b2fdbfc6d12452824768d9e389a5f6f3d680df`, tree
   `7f8230066735db17650b5d594a95d597540b3729`. Its live ledger records B1 `ACCEPTED`,
   all reviews `PASS`, and `maintainer_decision = "ACCEPTED"`. The former B1 branch and
   worktree are absent. `README.md:50-56`, `current-state.md:17-46`, AMD-005
   `:14-15,30-35,265-276`, and `program-state-transition.md:33-45` now describe that
   state and expose BF1 after AMD-005 ratification without scheduling another B1
   acceptance.

2. **Primary review binding and phase contract — resolved apart from the architect
   report blocker above.** `architecture-challenge.md:3-16`,
   `conformance-challenge.md:10-14`, and `governance-challenge.md:3-14` bind the original
   reviewed candidate/tree. The older architecture and governance reattestations now
   begin with explicit `SUPERSEDED AFTER REBASE` notices and distinguish their
   historical `6920ddc6...` binding from the current primary report binding.

3. **Validator conflict — resolved.** Running `validate-package.mjs --pre-review`
   failed as required with `architecture-challenge.md must be absent in pre-review
   preparation mode`. Running `--post-review --reviewed-commit
   ce1d0e4688af1b5bd548b6b68286632cc0f7ede8 --reviewed-tree
   1ff1f83d8e994b6f1169b0b209c9f557c23f4728` passed with 22,718 non-zero assertions.
   Negative checks using the new candidate identity and using the old commit with the
   new tree both failed on the expected commit/tree binding assertions. This matches
   `validation.md:42-63` and AMD-005 `:287-296`.

4. **Ratification action and next legal step — resolved.** AMD-005 `:298-314` now
   requires all three fresh primary verdicts to be `PASS`, binds both the independently
   reviewed package and report-only bundle identities, names B1's already accepted
   predecessor identity, and authorizes BF1 exposure only after ratification. It does
   not require or permit B1 re-acceptance. `program-state-transition.md:40-52` agrees.

The changed-scope sanity pass found no new blocking DAG or ledger-transition defect.
The candidate-to-fix diff does not change `program-dag.toml`; its BF1/BF2/BF3/B2/B3,
B4/BV1/BS1/B5/B6, and C1/C2/C3/C4 predecessor lists match AMD-005 `:82-105` and the
validator's exact-edge assertions. Both program-state validators pass for all 56
blocks. The base-to-candidate live-ledger diff changes only the DAG digest and adds the
five locked/PENDING amendment rows; the accepted B1 row is unchanged. `git diff
--check ce1d0e4688af1b5bd548b6b68286632cc0f7ede8
7442bb9060b7faa0720e528d3f96ee1df1abff95` and the validator syntax check are clean.

**BLOCKING_FINDINGS — the stale architect-report binding remains unresolved, bound to
commit `7442bb9060b7faa0720e528d3f96ee1df1abff95`.**
