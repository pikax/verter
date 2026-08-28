---
ruling_id: "GREEN-BRANCH-AND-TRIAGE"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["program-wide gate-failure triage discipline"]
source_file: "MAINTAINER-RULING-GREEN-BRANCH-AND-TRIAGE.md"
summary: "program/architecture-lock is GREEN BY INVARIANT, not by hypothesis — a red working branch is a P0, never re-derived with a second full gate run to check whether it pre-existed on trunk. On a branch gate failure, triage in isolation (re-run the failing tests alone, several times): deterministic failure is REAL, intermittent is FLAKY; report to the maintainer; fix flaky tests ASAP. Abolishes the standing 'known pre-existing baseline / environmental' disposition category entirely — cites a real deterministic production bug (compose_generated_chunk aborting on an empty source map) that hid behind that category across four landing records."
supersedes:
  - document: "the orchestrator's gate-range-mode line of work (not part of this corpus, described as 'withdrawn')"
    claim: "Detecting and running only the affected range of tests between two commits as the canonical gate."
superseded_by: []
contradicts: []
notes: "Explicitly withdraws gate.mjs range mode as a maintainer suggestion (not landed); the affected-tests selector itself stands as an inner-loop tool, not wired into the canonical gate."
---

# Maintainer ruling — the working branch is ALWAYS green; failures are triaged, not re-baselined

**Date:** 2026-08-20. Supersedes the orchestrator's gate-range-mode line of work (withdrawn).

> I want is not spending time running the gate then the gate fails and the agent runs the working branch
> gate to confirm if is broken in working branch, that's a waste of time, gate SHOULD ALWAYS be green in
> working branch! if the gate fails in a branch the agent should run the tests in isolation to confirm if
> is flaky or not, then report to you, we must fix flaky tests whenever possible and ASAP

## The waste being eliminated

The established habit was: gate fails on a block branch → the agent runs a SECOND full gate on
`program/architecture-lock` to decide whether the failure pre-existed. That is two whole-workspace gate
runs to answer one question, and it happened repeatedly (B3, B4, BV1, BS1 landing records all contain
some form of "reproduced identically against the base commit").

## The rule

1. **`program/architecture-lock` is GREEN BY INVARIANT, not by hypothesis.** A red working branch is a
   P0, not a baseline to be measured. Never re-derive it with a second gate run.
2. **On a branch gate failure, triage in ISOLATION.** Re-run the specific failing test(s) alone, several
   times. Deterministic failure ⇒ REAL, the branch broke it. Intermittent ⇒ FLAKY.
3. **Report to the maintainer**, with the classification and the evidence.
4. **Fix flaky tests whenever possible and ASAP.** Flakiness is not a cost of doing business; it is the
   thing that let a real production defect hide.

## Why this matters more than gate speed

The "known pre-existing baseline" list — three tests carried across four consecutive landing records as
environmental — turned out to contain a REAL DETERMINISTIC PRODUCTION BUG:
`external_template_ide_compile_contains_selected_bytes` was failing because `compose_generated_chunk`
aborted composition whenever one side's source map was empty, breaking essentially every non-source-map
IDE request. Four blocks in a row cited the previous block's dismissal instead of re-deriving it.

A standing list of accepted failures is where real bugs go unexamined. **This ruling abolishes that list.**
Every gate failure is now either REAL (fix it) or FLAKY (fix it ASAP). There is no third category, and
"environmental" is not a disposition.

## Withdrawn

`gate.mjs` range mode ("detect from one commit to another and resolve what to run") — the maintainer
withdrew the suggestion. The affected-tests selector (`86eb6ee29`) stands on its own as an inner-loop
tool; it is not being wired into the canonical gate.
