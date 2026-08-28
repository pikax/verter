# AMD-009 package record (historical)

**Status: HISTORICAL RECORD — superseded as a ratification record.** This file
describes the package as it stood when it was bound to reviewed package commit
`9e457ca781d3684e562d6eaea24c401e2d9849a7`, and it is retained under its original
filename so historical links keep resolving.

It is **not** the ratification record and must not be read as one. An earlier
revision of this file recorded the 2026-08-16
[`product ruling`](maintainer-product-ruling-no-error-on-bad-output.md) as having
ratified this package; that ruling in fact ratified only the AMD-009 §1/§2
no-retraction direction, by its own text. Full §7 ratification is the separate
2026-08-16 ruling at
[`maintainer-ruling-section7-ratification.md`](maintainer-ruling-section7-ratification.md),
bound to the rebound package content identity recorded in
[`amd009-ratification-packet.md`](amd009-ratification-packet.md) — which is the
authoritative ratification record. Ratification does not accept BF3 or unlock B2/B3.

## Package contents

The proposal consists of:

- [`AMD-009-bf3-audit-and-immediate-correction-blocks.md`](../../amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md),
  which states the binding direction, explicit supersessions, proposed DAG, new
  charter authority, exclusions, and exact ratification action;
- the rewritten [`BF3.md`](../../charters/BF3.md), which is an audit and
  correction-dispatch charter rather than a safety-retraction charter;
- [`BS0.md`](../../charters/BS0.md), [`BA0.md`](../../charters/BA0.md),
  [`BCSS0.md`](../../charters/BCSS0.md), and
  [`BRT0.md`](../../charters/BRT0.md), one immediate charter per settled
  root-cause family; and
- the proposed [`program-dag.toml`](../../program-dag.toml) diff, which renames and
  reclasses BF3, inserts all four correction blocks after it, and makes BV0, BF3,
  BA0, BS0, BCSS0, and BRT0 mandatory predecessors of both B2 and B3.

No compiler/session correction, production retraction path, live ledger transition,
block acceptance, or downstream unlock is part of this package.

## Authority

The settled architecture authority is the
[`scope-consult-ruling.md`](scope-consult-ruling.md): supported requests with wrong
output receive regressions and root-cause corrections, while safety is supplied by
the DAG lock. The [`adjudication-ruling.md`](adjudication-ruling.md) establishes that
BF3 cannot be accepted under the current DAG and may close as an audit only after a
ratified amendment makes every immediate correction owner a B2/B3 predecessor. The
ratified [`dispositions.md`](dispositions.md) supplies the exact rows,
classifications, owners, acceptance IDs, and tests; this package does not reclass,
rename, invent, or reopen them. BRT0 is included because RT-1/TR-1 and the provisional
BND rows were dispositioned after the adjudication's D1–D5 enumeration.

## Live text superseded by ratification

AMD-009 explicitly supersedes, rather than silently reinterprets:

1. BF3's “Known-wrong successful-cell safety retraction” title and objective;
2. BF3 procedure steps 3–5 requiring detection, typed non-success, artifact
   withholding, and whole-cell retraction;
3. BF3 procedure step 7's guard-deletion and removal-acceptance requirement;
4. BF3's retained Svelte/non-Vue-runtime retraction paragraph;
5. BF3's abort logic that substitutes complete-cell retraction for broad repair;
6. BF3's required-exit guard/retraction/removal-ID clause;
7. AMD-005 §5 and §12's BF3 typed-non-success, artifact-withholding, and
   whole-cell-retraction authority for retained Svelte and non-Vue-runtime
   successful cells, together with the conflicting effect of the §15.1
   recorded-ratification wording accepting that authority;
8. AMD-006 §4's retention of the original mechanism for Svelte and
   non-Vue-runtime successful cells;
9. AMD-006 §8.1's `RETROACTIVE-NO-FORWARD-ONLY` ruling;
10. the live BF3 ledger note, with exact replacement text in AMD-009 §2 and below;
11. the old `{BV0, BF3} -> {B2, B3}` edge set; and
12. the `BF3-RET-*` production-record scheme in
    [`bf3-safety-retraction-scope.md`](../framework-conformance/bf3-safety-retraction-scope.md).

Historical superseded AMD-005 text and other ratification and evidence text remains
useful history. It does not become a second live authority for implementing
production defect recognition after AMD-009 ratification. AMD-005's unaffected Vue
and oracle body remains in force. Before AMD-009 was ratified, the settled consult
prohibited an implementer from building the disputed mechanism. The ratified
amendment now supplies the no-retraction authority; the DAG continues to prohibit
downstream dispatch until every required predecessor is accepted.

## Proposed ledger transition for the returning orchestrator (APPLIED — historical)

**This snippet has already been applied by the program orchestrator** and is kept only
as the record of what was proposed. The four correction-block rows exist in the live
ledger today, and the current BF3 row's status and notes have since moved on. Read the
live ledger for present state, and [`landing-record.md`](landing-record.md) for the
transition proposed after the cure.

The program orchestrator owns
`docs/arch/architecture-lock/ledger/program-state.toml`. This package did not write
that file. BF3 remained `READY` at the time this snippet was proposed, B2/B3 remain
`LOCKED`, and `current_block` remained unchanged; the snippet intentionally contains no
acceptance or unlock transition.

```toml
# Preserve the current top-level field exactly. Live value is BF3, not BV0A.
current_block = "BF3"

# In the existing BF3 row, leave status = "READY" and replace only notes.
notes = "BF3 is a pre-B2/B3 conformance-exhaustion and correction-dispatch audit under ratified AMD-009. It adds no production guard, typed refusal, artifact-withholding, retraction, or runtime tracking mechanism. Inventory exhaustion requires actual results; every genuine failure has evidence, an independently discriminating regression, root-cause classification, a named immediate correction owner, and a correction acceptance/test ID. BF3 may close only after AMD-009 ratification and creation of mandatory B2/B3 predecessor edges for BA0, BS0, BCSS0, and BRT0. B2/B3 remain locked until BV0, BF3, BA0, BS0, BCSS0, and BRT0 are accepted. The existing Svelte ServerGenerate refusal is a contract-defined pre-compilation capability boundary and receives no BF3 removal ID."

# Insert these four rows after BF3.
[[block]]
id = "BA0"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = "Introduced by ratified AMD-009. Predecessor BF3; mandatory predecessor of B2 and B3. Remains LOCKED until BF3 is ACCEPTED."

[[block]]
id = "BS0"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = "Introduced by ratified AMD-009. Predecessor BF3; mandatory predecessor of B2 and B3. Remains LOCKED until BF3 is ACCEPTED."

[[block]]
id = "BCSS0"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = "Introduced by ratified AMD-009. Predecessor BF3; mandatory predecessor of B2 and B3. Remains LOCKED until BF3 is ACCEPTED."

[[block]]
id = "BRT0"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = "Introduced by ratified AMD-009. Predecessor BF3; mandatory predecessor of B2 and B3. Remains LOCKED until BF3 is ACCEPTED."

# In the existing B2 row, leave status = "LOCKED" and replace only notes.
notes = "Predecessors are BV0, BF3, BA0, BS0, BCSS0, and BRT0. B2 remains LOCKED until all six are ACCEPTED; AMD-009 does not unlock it."

# In the existing B3 row, leave status = "LOCKED" and replace only notes.
notes = "Predecessors are BV0, BF3, BA0, BS0, BCSS0, and BRT0. B3 remains LOCKED until all six are ACCEPTED; AMD-009 does not unlock it."
```

The orchestrator must also recompute the charter/DAG digests required by the live
ledger validator when it performs the ratified transition. That bookkeeping is not
authorization to change any block status.

## Exclusions and non-authority

The stale `svelte@5.56.3` root pin and corpus migration is a separately identified
train and is excluded because this package does not authorize it. No pin-migration
block was added. No production code was changed or deleted, and no
BS0/BA0/BCSS0/BRT0 implementation was created.

H-delta reviews, if completed, did not authorize BF3 acceptance, correction-block
acceptance, B2/B3 dispatch, ledger mutation, or AMD-009 ratification. Review evidence
qualified the exact candidate for the designated maintainer's decision; the
2026-08-16 product ruling supplied that decision. It still does not accept BF3,
unlock B2/B3, or mutate the ledger.
