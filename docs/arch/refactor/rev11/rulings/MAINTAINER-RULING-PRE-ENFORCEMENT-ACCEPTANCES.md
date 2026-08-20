---
ruling_id: "PRE-ENFORCEMENT-ACCEPTANCES"
type: "maintainer-ruling"
date: "2026-08-20"
date_source: "stated"
binds: ["BF1", "BF2", "B2", "B3", "B4"]
source_file: "MAINTAINER-RULING-PRE-ENFORCEMENT-ACCEPTANCES.md"
summary: "The maintainer retroactively ratifies the five block acceptances that predate machine-enforced block authorization — BF1, BF2, B2, B3, B4 — each of which left LOCKED while the authorization rule existed in prose only. This is the authorizing document their authority-registry records cite. It ratifies exactly these five and grants no standing exemption: every subsequent transition is bound by the enforced registry."
supersedes: []
superseded_by: []
contradicts: []
notes: "Issued in response to the finding recorded in docs/arch/architecture-lock/ledger/UNAUTHORIZED-TRANSITIONS.md, which enumerated four possible dispositions; the maintainer chose retroactive ratification. Scope is exactly the five named blocks and does not extend to any block accepted after enforcement landed in a7b07d31b."
---

# Maintainer Ruling — Retroactive ratification of the five pre-enforcement acceptances

**Status:** RATIFIED by the maintainer, 2026-08-20.

## What was found

Machine-enforced block authorization landed in `2efa644a7` + `a7b07d31b`. On its
first live run it reported five violations: **BF1, BF2, B2, B3 and B4** are
`ACCEPTED` — past `LOCKED` — with no digest-bound authorization record.

Those five transitions were recorded while the authorization rule existed in
prose only. Enforcement did not break them; it made an existing gap visible. A
search of the ruling corpus and the amendment records found no document that
authorizes any of the five, and none was fabricated to clear the failure.

The finding, and the four possible dispositions, are recorded in
[`UNAUTHORIZED-TRANSITIONS.md`](../../architecture-lock/ledger/UNAUTHORIZED-TRANSITIONS.md).

## The ruling

The maintainer **retroactively ratifies** the acceptance of BF1, BF2, B2, B3 and
B4.

This document is the authority their `[[authorization]]` records cite. Each
record states on its face that the ratification is retroactive and dated to this
document, not to the date the transition was originally recorded — the ledger
should not claim an authority existed at a time when it did not.

## Scope

Exactly these five blocks. This ruling grants **no standing exemption** and
establishes **no precedent** for accepting a block without prior authority.
Every transition after `a7b07d31b` is bound by the enforced registry, and a
block that leaves `LOCKED` without a digest-bound record is a violation that
must be resolved before it is accepted, not after.
