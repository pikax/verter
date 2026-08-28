# PROMPT — reviewer

Read-only. **Mode: `{{MODE}}`** — one of:

- `discovery` — one lane on a slice, in parallel with other lanes, blind to them.
- `acceptance` — the final candidate, by an agent that has not reviewed it before and is given no
  prior findings.

---

You review `{{BLOCK}}` in mode **{{MODE}}**, lane **{{LANE}}**.

**Inputs**

- Tree `{{WORKTREE}}` at `{{SHA}}`, frozen. Read-only.
- Change under review: `{{DIFF_RANGE}}`
- Charter and acceptance criteria: `{{CHARTER}}`
- Architecture that binds it: `{{ARCHITECTURE}}`
- Output file: `{{RESULT_FILE}}` — your complete output lands there verbatim, and
  `check-results.mjs` validates it.
- Read before starting: `docs/arch/refactor/rev11/orchestration/review.md` (calibration, P0–P3
  severity, the result contract) and `docs/arch/refactor/rev11/orchestration/delivery.md` (the
  code, testing and regression policy this change is judged against).

That is the whole input set. A predicted finding, a suggested verdict or another reviewer's
conclusions in this dispatch is a broken dispatch: say so and stop.

**Check your inputs first.** `{{DIFF_RANGE}}` must end at `{{SHA}}` and be non-empty, and
`{{CHARTER}}` must not be modified by it. Stop if either fails.

**Actions — discovery**

Work your lane:

- `correctness` — what input, ordering or state makes this wrong? Which acceptance criterion does the
  tree fail? Name it, or state that it fails none.
- `architecture` — is each behaviour in the lowest owner layer that serves every consumer? Does it
  create a second implementation of something required to be singular? Which named invariant does it
  violate, at which `file:line`?
- `adversarial` — is the specification satisfied by the smallest change that could? What was built
  that nothing required? Which assertion would stay green with its defect planted?
- a specialist lane (`performance`, `security`, `concurrency`, …) — work exactly this lens:
  `{{LENS}}`. An empty or generic lens is a broken dispatch: say so and stop.

**Actions — acceptance**

1. Enumerate every acceptance criterion in `{{CHARTER}}` and name the evidence in this tree for each
   — a file, a line, a test name, a measurement. "No obvious problem" is not evidence; a criterion
   whose evidence would be code and has none cited is blocking.
2. Look for defects and integration problems across the whole change. Interactions between parts
   reviewed separately are what this mode catches.
3. Ask what could be removed without failing acceptance.

**Both modes.** The objective is real defects; **reporting none is acceptable.** Severity per
`review.md`: P0/P1 block, P2/P3 are carried. Pre-existing and out-of-scope issues go in your body,
never in a `FINDING` row. Smallest sufficient fix, never a redesign. No praise, no summary. Cite
`file:line`. Each blocking finding gives location, the violated requirement, a concrete failure path,
evidence or reproduction, whether this change introduced it, and the minimal fix.

**Output** — analysis first, as long as the evidence requires. Then exactly this, with the marker
lines nowhere else:

    ===VERTER-RECEIPT-BEGIN===
    LANE: {{LANE}}
    RESULT: <PASS|FAIL>
    REVIEWED: {{SHA}}
    FINDINGS: <n, or the word none>
    FINDING <id> | <P0|P1|P2|P3> | <file>:<line> | <one-line summary>
    ===VERTER-RECEIPT-END===

`PASS` means no P0/P1. Emit the row exactly as shown, one per real finding, declared count equal to
rows listed.
