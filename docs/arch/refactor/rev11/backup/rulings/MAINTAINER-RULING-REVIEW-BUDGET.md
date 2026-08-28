---
ruling_id: "REVIEW-BUDGET-BY-ARTIFACT-CLASS"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["program-wide review protocol"]
source_file: "MAINTAINER-RULING-REVIEW-BUDGET.md"
summary: "Review rigour is reallocated by artifact class, backed by a process audit (8,581 review-report lines / 239 findings across four doc-only campaigns produced zero production defects, while review mandates pointed at running code produced all 16 found production defects). Production code (crates/, packages/, scripts/) keeps unchanged full rigour. Evidence/landing records/context packets get NO review rounds — authored once accurately, checked by orchestrator fact-verification not reviewer prose preference. Charters/amendments/specs get one cheap grok soundness pass (model 4.6, Extra High), escalating to codex only on a real flagged contradiction. Grok is explicitly encouraged liberally up front for pre-implementation scoping and premise verification."
supersedes: []
superseded_by: []
contradicts: []
notes: "Explicit: correctness standards on code (TDD, no stubs, proven-applied mutation plants, honest UNPROVEN) are untouched — the objection is to review volume on prose, not rigour on code."
---

# Maintainer ruling — review budget by artifact class (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax).
Binding on every track orchestrator and every dispatch for the remainder of the program.

## Verbatim ruling

> i don't mind spending grok resources to make each block easier to implement or to confirm if it's
> sound and whatnot, but the number of changes to docs with too many reviews is quite annoying I
> wouldn't mind as much if it was production code

## The evidence behind it

A process audit measured: the four largest artifact/spec campaigns produced **8,581 review-report
lines and 239 findings, of which ZERO were production defects**. Governance review found zero across
four campaigns. Meanwhile review mandates pointed at RUNNING CODE produced **all 16** production
defects found by review. Executable oracles independently out-found review ~30 to 16. Twice, review
findings would have made production WRONG had they been executed. Separately, 70% of amendments
corrected previously-ratified text and 31% of wall-clock was rework.

Conclusion: review rigour is MISAPPLIED, not excessive. Reviewing prose is provably unproductive;
reviewing running code is where every real defect came from.

## The rule — review budget by what is being changed

**1. Production code (`crates/`, `packages/`, `scripts/`) — UNCHANGED, full rigour.**
Required mandates per the block's class, adversarial plant-red-green at the first pass, round cap 3,
discriminating tests, executable evidence. This is where defects are, so this is where review goes.

**2. Evidence records, landing records, context packets — NO REVIEW ROUNDS.**
Author once, accurately, concisely. They RECORD what happened; they are not artifacts under test. Do
not open a review cycle on them, do not iterate them for style, and do not let a reviewer's prose
preference generate a fix round. Accuracy is checked by the orchestrator's verification of the facts
they assert, not by a review mandate.

**3. Charters, amendments and specs — ONE cheap soundness pass, not three mandates.**
Use a single grok pass (model 4.6, Extra High) asking: is this internally consistent, does it
contradict a ratified document, and does it contradict the code? Escalate to a codex seat ONLY if
that pass flags a real contradiction. Do not run three mandates on prose.

**4. Use grok liberally UP FRONT — this is explicitly encouraged.**
Pre-implementation scoping, soundness checks, premise verification ("is this ratified claim actually
true of the code?"), call-site inventories, blast-radius checks. It is cheap, the maintainer has
authorised the spend, and it attacks the 31% rework figure at its source: every block this program
reopened did so because a premise was wrong, not because review was too thin.

**5. Stop producing documents where production code is the deliverable.**
Already binding. Restated because it is the same root cause.

## What does NOT change

Correctness standards on code are untouched: TDD, no stubs, discriminating tests, proven-applied
mutation plants, no zero-selection false greens, no manufactured PASS, honest UNPROVEN over a
comfortable claim. The maintainer's objection is to review VOLUME ON PROSE, not to rigour on code.
