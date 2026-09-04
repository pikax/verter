# TCM0R budget amendment (rev11.typescript-mapper)

- Status: accepted
- Date: 2026-09-04
- Amends: `charters/rev11-typescript-mapper/TCM0R.md` budget section, and the
  matching machine fields on `authority/dag/rev11-typescript-mapper.toml`
- Grounds: `ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION`, the ruling this block
  implements, read with the rescope decision at
  `decisions/2026-09-02-typescript-mapper-rescope.md`
- Scope: TCM0R only; no other node's budgets change

## Context

TCM0R was chartered as an authority-and-evidence block: `max_production_loc=0`,
`max_production_files=0`, `max_related_packages=0`, and a mutation boundary
reading "authority/evidence bytes only; production LOC is zero".

The ratified contract this block exists to land states that the observed-profile
component of the query identity is a SET — one profile observed twice is the
same question as observing it once. The shared canonical encoder sorted and
length-prefixed that field without deduplicating it, so the sentence being
ratified was false about the code it describes on the day it was written.

Four answers were available and three of them publish a claim stronger than its
evidence: ratify the false sentence; disclose the gap beside an atom that still
derives proven; carry it as a fourth remainder the governing ruling closes the
set against; or make the sentence true. The block took the fourth. The reasoning
is recorded in full at `decisions/2026-09-02-typescript-mapper-rescope.md`; this
record exists because that reasoning was written into the charter's prose while
three machine fields kept saying zero, and a budget stated twice with two
different numbers binds nobody.

## Decision

Amend TCM0R's production budget to the candidate's measured footprint:

| field                  | was | now |
| ---------------------- | --- | --- |
| `max_production_loc`   | 0   | 92  |
| `max_production_files` | 0   | 2   |
| `max_related_packages` | 0   | 1   |

Measured against the branch point: 92 added production lines across
`crates/verter_identity/src/encoding.rs` (+28/−2) and
`crates/verter_identity/src/identity.rs` (+64/−14), in one crate. The behaviour
change inside that total is one statement — `sorted.dedup()` in
`CanonicalEncoder::field_sorted_set`. The rest is the doc comment stating why
the field is a set rather than a sorted bag, and the two test bodies that
discriminate the property: sorting already makes `[p1, p2]` and `[p2, p1]` agree
under either encoding, so an order-independence case proves nothing here and the
repeat case is the whole of the evidence.

The three named budget fields are the amendment. The mandatory-rescope trigger
(1,500 LOC / 12 files / 3 unrelated packages) is UNCHANGED and remains in force,
as do the correctness budget and the `architecture-3` review profile.

## What this record cannot do

It cannot evidence itself, for the same reason the rescope record states of its
own ratification: no artifact inside the tree distinguishes an amendment a
maintainer accepted from a document asserting that one was accepted. The
numbers, the measurement, and the reason are properties of committed bytes a
reader re-checks without trusting this paragraph; the acceptance is the
maintainer's, and it is the same `maintainer_tcm0_rescope_ratification`
requirement the node already declares. An unaccepted record is an open
requirement, not a satisfied one.

## Consequences

- TCM0R's charter header, its "Target ceiling" line, and its DAG node state one
  set of numbers rather than three.
- No successor's budget changes. TCM1–TCM4 keep their own.
- The precedent this follows is `decisions/2026-08-30-rev11-flow-d2b-budget-amendment.md`,
  which moved `max_production_loc` in both the charter header and the DAG module
  rather than recording the overrun only in prose.
