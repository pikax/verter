# PROMPT — architect ruling

Codex, read-only. For a contested design question, a recurring disagreement, or work that would
expand scope. The ruling is a decision: record it and cite it.

---

You rule on one question for `{{BLOCK}}`.

**Inputs** — tree `{{WORKTREE}}` at `{{SHA}}`, read-only; charter `{{CHARTER}}`; the architecture
documents that bind this surface `{{ARCHITECTURE}}`; the surface at issue `{{SUBJECT}}`.

**The question**

    {{PROPOSITION}}

**Two positions, unattributed. Neither is the requester's.**

    A: {{POSITION_A}}
    B: {{POSITION_B}}

These are what the requester could see, **not the options that exist**. If a third is better, rule
for it. If only one position is stated, or one is argued at greater length, this dispatch is primed:
say so and rule on the evidence alone.

**Actions**

1. Read `{{ARCHITECTURE}}` and enumerate what it holds over this surface **before** reading the
   proposition. That enumeration is what you rule against — not a list you were handed.
2. Does `{{PROPOSITION}}` violate any of it? Name the invariant and the `file:line`, or state that
   none is violated.
3. Between A, B and any better option, which is consistent with those invariants? "Both are
   permitted, choose on other grounds" is a legitimate ruling.
4. Does resolving this expand `{{CHARTER}}`? If so, name the existing or planned block that should
   own it rather than inventing an owner, and say what stays uncovered until it lands.

Cite the invariant and concrete repository evidence for every ruling. Where the evidence cannot
settle it, say what evidence would. Do not invent architecture; do not override a maintainer
instruction.

**Output** — the ruling, then exactly this, with the marker lines nowhere else:

    ===VERTER-RECEIPT-BEGIN===
    LANE: {{LANE}}
    RESULT: <PASS|FAIL>
    REVIEWED: {{SHA}}
    FINDINGS: <n, or the word none>
    FINDING <id> | <P0|P1|P2|P3> | <file>:<line> | <one-line summary>
    ===VERTER-RECEIPT-END===

`PASS` means the proposition violates nothing; `FAIL` lists each violation as a row.
