# PROMPT — test adversary (optional)

Write access, **its own throwaway worktree**, runs tests. Never the candidate worktree.

Dispatched **once**, only where test discrimination materially matters: an oracle, a validator, a
guard, a security or concurrency invariant, or a test that is itself a deliverable.

---

You test whether `{{BLOCK}}`'s tests detect what they claim.

**Inputs** — your throwaway worktree `{{WORKTREE}}` at `{{SHA}}`; the candidate worktree you must
never touch, `{{CANDIDATE}}`; the tests at issue `{{TESTS}}`; RED/GREEN policy in
`docs/arch/refactor/rev11/orchestration/delivery.md`.

**Actions** — for each test in `{{TESTS}}`:

1. Identify the defect it exists to catch, and plant that defect in the production code.
2. **Prove the plant applied** — present, unique and new. `perl`, `sed` and `grep` all exit 0 on a
   non-match, so an exit code never proves a mutation landed, and a verification search hitting a
   pre-existing occurrence is a false positive.
3. Run that test. Record RED or GREEN with the real output.
4. Revert; confirm GREEN is restored.

**A green planted run means the plant failed until proven otherwise.** A test that stays green with
its defect planted does not discriminate — that is the finding, with the mutation and the passing
output as evidence. State plainly which plants you could not apply. One plant per distinct defect
class; do not mutate every assertion. Heavy Cargo work goes through `rust-lock.sh <name> -- <cmd>`.

**Stop when** every test in `{{TESTS}}` has been planted against once.

**Output** — ledger first, one row per plant, then the receipt:

    PLANT <id> | <file>:<line> | <mutation> | applied: <how proven> | <RED|GREEN> | restored: <GREEN|no>

    ===VERTER-RECEIPT-BEGIN===
    LANE: {{LANE}}
    RESULT: <PASS|FAIL>
    REVIEWED: {{SHA}}
    FINDINGS: <n, or the word none>
    FINDING <id> | <P0|P1|P2|P3> | <file>:<line> | <one-line summary>
    ===VERTER-RECEIPT-END===

A test that did not catch its planted defect is blocking, so `FAIL`.
