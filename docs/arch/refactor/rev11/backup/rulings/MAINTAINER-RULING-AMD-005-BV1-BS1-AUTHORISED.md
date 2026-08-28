---
ruling_id: "AMD-005-BV1-BS1-AUTHORISED"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["BV1", "BS1", "AMD-005"]
source_file: "MAINTAINER-RULING-AMD-005-BV1-BS1-AUTHORISED.md"
summary: "Narrow ruling given after an orchestrator escalation: BV1 and BS1 are authorised; AMD-005 itself is NOT ratified wholesale (its DAG amendments, oracle/exclusion rules, capability and performance locks §4-§9 stay PROPOSED, no execution authority). BV1's landing stands (ACCEPTANCE_RECOMMENDED -> ACCEPTED); BS1 returns from LOCKED to dispatchable. Neither block's authority derives from AMD-005 any longer; their ledger rows cite this ruling directly with enabling_amendment = \"\"."
supersedes: []
superseded_by: []
contradicts: []
notes: "Records the orchestrator's dispatch error (BV1 was executed before its authority was established) rather than erasing it. Notes a durable fix in flight: enabling_amendment becomes a structured ledger field validated against the amendment's real Status: line."
---

# Maintainer ruling — BV1 and BS1 are authorised; AMD-005 is NOT ratified

**Date:** 2026-08-20. **Given by:** maintainer, in response to an orchestrator escalation.

## Question put

AMD-005 is marked `**Status:** PROPOSED — NOT RATIFIED. This candidate has no execution authority.`
BV1 and BS1 are the only two blocks it introduces. BV1 had already been dispatched and landed —
reviewed 3/3, gate-clean — because the orchestrator unlocked it on DAG predecessor satisfaction alone
and did not read the block row's own note that its enabling amendment was unratified. Nothing in the
validator gated on it.

## Ruling

**Narrow: BV1 and BS1 are authorised. AMD-005 is NOT ratified wholesale.**

The maintainer selected the narrow option over ratifying AMD-005 in full. The remainder of AMD-005 —
its DAG amendments, oracle/exclusion rules, capability and performance locks, §4–§9 — stays PROPOSED
and carries no execution authority. Do not cite AMD-005 as authority for anything else.

Supporting context put to the maintainer, and not contradicted: AMD-009, which the maintainer ratified
in full on its §7 scope, already references "BV1 preservation requirement likewise remain in force"
(AMD-009:110) and "post-B4 BS1" (AMD-009:197, :225), treating both blocks as in-force expectations.

## Consequences

- BV1: `ACCEPTANCE_RECOMMENDED` → `ACCEPTED`, accepted identity restored to the landed candidate. Its
  landing stands; nothing is reverted.
- BS1: returned from `LOCKED` to dispatchable.
- Neither block's authority derives from AMD-005 any more. Their ledger rows carry
  `enabling_amendment = ""` and cite THIS ruling, so the amendment-authority gate does not need an
  exception path for them.
- The orchestrator's dispatch error is recorded rather than erased: the block was executed before its
  authority was established, and only a manual audit caught it.

## Durable fix, in flight separately

`enabling_amendment` becomes a structured ledger field validated against the amendment's real `Status:`
line, so a block whose enabling amendment is unratified cannot leave `LOCKED`. Prose in a `notes` field
is not an enforcement mechanism — that is precisely how this was missed.
