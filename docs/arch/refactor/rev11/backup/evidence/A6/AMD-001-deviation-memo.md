# A6 — Architecture deviation memo: AMD-001's four artifacts are not delivered

**Status: RULED — superseded by `AMEND-AMD-001-TIMING`. This memo's own recommendation (`DEFER`) was
NOT the disposition adopted.** Retained unaltered below as the historical record of what was
recommended and on what evidence; it is no longer the disposition of record.

**The ruling.** The maintainer ruled **AMEND-AMD-001-TIMING** — registered as
[`../maintainer-rulings.md` R-12](../maintainer-rulings.md), and recorded inside the amendment itself
under "Amendment to §1's timing". It is neither this memo's recommended `DEFER` (alternative 2, which
would have left a standing deviation against unamended text) nor the `DELIVER-NOW` instruction the
block first received. It adopts **alternative 4** — the rescope this memo offered as the maintainer's
option: AMD-001 §1 is amended in place so its four artifacts remain mandatory before the first
post-lock stack window opens and unconditionally before `D1` enters `PRIVATE_CHECKPOINT`, but the
delivery duty binds to whichever accepted candidate immediately precedes that event **rather than to
this block by name**. §§2-4 stand unchanged, including §4's traceability duty, which this block
discharges against the post-amendment text.

**What that changes.** There is no longer a deviation: the amendment text and the delivery reality
agree, so the lock record's §11 row U-9 is restated as informational rather than left as an open
item, and §9 binds the **post-amendment** digest. What is unchanged is everything the analysis below
rests on — no stack window is open, the unlocked block is depth-1, and AMD-001 §3's fail-closed
refusal in `scripts/validate-program-state.mjs` is untouched by this candidate.

The three questions in "What a ruling on this memo must decide" are answered by R-12 as: (1) the
rescope at alternative 4, not `DEFER`; (2) yes, §1's timing clause is amended; (3) the resolution
gate is the first opened stack window, and unconditionally before `D1` enters `PRIVATE_CHECKPOINT`.

`governance.md` §10 gave that decision to the maintainer — amend the ADR/architecture/charter, split
the block, or abort — and AMD-001 §3 forbids the one disposition nobody may take unilaterally:
removing the program-state validator's fail-closed refusal without the replacement that supersedes
it. The ruling amended; it did not weaken the refusal.

**Amendment under deviation:** `AMD-001 — Stack-Window Validator Is a Prerequisite for the D1/D2
Path`, [`../../amendments/AMD-001-stack-window-validator-prerequisite.md`](../../amendments/AMD-001-stack-window-validator-prerequisite.md).

**Bound digest — the PRE-amendment text this memo was written against** (SHA-256, lowercase hex,
over the raw bytes at the base tree the memo was drafted on,
`6af543c8a65b495aad2d6231e5e90878c3bf1769`):

```
b70ed6e8e6f7b8dcc86ae684d0568ca8c77ed6a93ade144b55fd8488f2e06208
```

recomputed with

```sh
git show 6af543c8a65b495aad2d6231e5e90878c3bf1769:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md \
  | shasum -a 256
```

That digest is **historical and deliberately not updated**: it names the exact bytes this memo
analysed, which is what makes the memo readable as a record of what was actually reviewed. The
amendment has since been amended by R-12, and the base commit above has since been superseded by the
integration lineage's commit-message rewrite. The **live** §4 traceability binding — the
post-amendment digest
`01661d01445e76f8861995061fd61511415550633a05b6ad351ec562b0ad5fd4` at base tree
`fb863297a04c7eb114d53ff65736c00240354504` — is recorded in
[`implementation-lock-record.md`](implementation-lock-record.md) §9 and in
[`context-packet.md`](context-packet.md)'s second addendum. Do not read the digest above as this
block's current binding.

The amendment's own §4 requires that command be spelled `sha256sum`; on this runner that binary does
not exist and `shasum -a 256` is the same algorithm over the same bytes. The digest is quoted here
and in the lock record and the context packet, never inlined into the amendment — a self-digest is a
fixpoint.

---

## The deviation, in `governance.md` §10 form

```text
Failed assumption:
  AMD-001 §1 assumes A6 — an evidence-and-lock block whose deliverables are a record, a gate file,
  a stack policy and one bound charter — is the right owner for four pieces of executable validator
  machinery, and that they must exist at A6's acceptance rather than before the path they model is
  first reachable. The assumption is that A6's acceptance is the last safe moment. It is not: the
  first reachable moment is the first opened stack window, and A6 both opens none and unlocks a
  block that cannot open one.

Measured/source evidence:
  - No stack window exists to validate. The locked policy (decision S-1) sets
    max_open_stack_layers = 2 with a DEFAULT OPERATING DEPTH OF 1, and this lock deliberately ships
    a POLICY file (evidence/A6/stack-window-policy.toml) rather than an instance of the snapshot
    template, because minting a one-layer snapshot would record a stack that does not exist. A
    validator for stack-window RECORDS would therefore be landed with zero records to validate and
    zero call sites, and its D1/D2 fixture would be its only input.
  - The one block this lock unlocks is sequential and single-layer. B1's row in the lock record §9
    table reads `stack_id = ""`, `stack_layer = 0`, "none — depth 1". Every block accepted so far
    has correctly carried an empty stack_id. Nothing between here and the next lock can open a
    window.
  - The unmodelled path is CLOSED, not open. scripts/validate-program-state.mjs rejects any block
    that has begun while a direct predecessor is in PRIVATE_CHECKPOINT. That refusal is precisely
    what makes the deferral safe: the D1/D2 transition cannot be executed while the model for it is
    missing, because the always-green validator refuses it. The defect AMD-001 records is that the
    refusal has no prerequisite delivering its replacement — a trap sprung at D2, not at B1.
  - D1 is not reachable from here. It is many blocks downstream; no predecessor chain from B1
    reaches it without further lock records and further maintainer acceptances, each of which is an
    opportunity to deliver the model with the D1/D2 semantics actually settled rather than guessed.

Affected architecture/verification invariants:
  - AMD-001 §1 (the prerequisite itself) — NOT satisfied by this candidate.
  - AMD-001 §2 (the composite acceptance rule) — undelivered, and therefore unexercised.
  - AMD-001 §3 (the refusal is superseded, never deleted) — SATISFIED and unchanged: this candidate
    modifies scripts/validate-program-state.mjs not at all. The refusal stands exactly as landed.
  - AMD-001 §4 (mechanical traceability) — satisfied by this candidate: the identifier and digest
    are bound in the lock record and in the context packet's addendum. Traceability was the half of
    the amendment a documentation block can actually discharge, and it is discharged.
  - governance.md's always-green program-state validator requirement — currently satisfied, and it
    stays satisfied for every state this lock makes reachable.
  - contracts/stacked-prs.md §3.2 — its model remains unvalidated by tooling; it is held by the
    refusal plus review, which is weaker and is recorded as weaker.

Compatibility or consumer consequences:
  None. No wire format, no cache identity, no public API, no persisted artifact and no generated
  output is involved. The consumer surface of the undelivered artifacts is the orchestrator's own
  transition workflow, which has no state that needs them yet.

Alternatives:
  1. DELIVER NOW, in this block. Land the stack-window validator, the composite cross-validation,
     the CI wiring and the D1/D2 fixture as part of the lock candidate. Cost is not the objection —
     the objection is that the semantics would be frozen against no instance: a stack-window record
     schema validated against zero real records, and a D1/D2 fixture written from contract prose
     rather than from the transition it must accept. A validator that has never seen its subject is
     the "self-declared test universe" the repository's verification rule names, and it would be
     landed under this program's own gate-immutability discipline, making a later correction a
     recalibration rather than an ordinary fix.
  2. DELIVER LATER, at a named gate, with the refusal left standing. The refusal keeps the
     unmodelled path closed in the interim; the model is written when the transition it validates is
     the next thing to happen and its inputs are real. This is the recommendation below.
  3. DELETE OR WEAKEN THE REFUSAL to unblock the path in the interim. FORBIDDEN by AMD-001 §3 and
     recorded here only so the ruling has the full option set in front of it. This recreates exactly
     the unvalidated-path defect the amendment exists to record.
  4. RESCOPE AMD-001 to reassign ownership from A6 to the block that first needs a window (or to a
     dedicated orchestration-tooling block). This is a superset of alternative 2 and is the cleaner
     durable form if the maintainer prefers the amendment text to match the delivery reality rather
     than carry a standing recorded deviation against it.

Recommended amendment:
  DEFER, at alternative 2, with alternative 4 as the maintainer's option for making the amendment
  text match. Concretely: retain AMD-001 §§2-4 unchanged, and amend §1's timing clause so the
  prerequisite binds to the FIRST STACK WINDOW rather than to A6's acceptance — delivery required
  before the first snapshot with more than one open layer is minted, and unconditionally before the
  private-checkpoint block may enter PRIVATE_CHECKPOINT. The refusal in
  scripts/validate-program-state.mjs stays in force until superseded by the delivered composite
  validation, per §3, which this candidate does not touch. Owner: the orchestrator, under the
  amendment. This is recorded in the lock record as unresolved item U-9, which already names that
  resolution point.

Work that remains valid:
  All of it. Nothing in this candidate depends on the undelivered artifacts: the baseline identity,
  the command manifest, the measured gate file and its validator, the counter reproduction, the
  stack POLICY, the bound charter and the B1 unlock are independent of stack-window validation, and
  none of them would be re-derived differently if the four artifacts landed tomorrow. The deferral
  costs no rework; delivering under it costs no invalidation.
```

---

## What a ruling on this memo must decide

1. **Disposition** — `ADOPT-NOW` (deliver the four artifacts, in this block or an immediate
   successor), `DEFER` (the recommendation above), or `REJECT` (a reading under which the
   prerequisite does not apply). The lock record's U-9 row is written for `DEFER` and must be
   rewritten if the ruling differs.
2. **Whether AMD-001 §1's timing clause is amended** (alternative 4) or the deviation stands
   recorded against unamended text. Both are legal; they differ in whether a later reader finds an
   amendment that says A6 delivered something A6 did not.
3. **The named resolution gate**, if `DEFER`: this memo and U-9 propose "before the first
   multi-layer snapshot, and unconditionally before the private-checkpoint block begins."

**That ruling has since been recorded**, at [`../maintainer-rulings.md`](../maintainer-rulings.md)
R-12, and it is `AMEND-AMD-001-TIMING` — the rescope at alternative 4, not the `DEFER` this memo
recommended. The three questions above are answered in the status block at the top of this file. This
memo is therefore **no longer the disposition of record**; R-12 is. The `governance.md` §10 form
above, and the three questions above it, are retained **unaltered** as the historical record that a
`DEFER` recommendation was made, on what evidence, and that the maintainer chose differently — which
is exactly the trail §10 exists to leave. Read them in the past tense: where they say "the lock
record's U-9 row is written for `DEFER` and must be rewritten if the ruling differs", the ruling did
differ and U-9 has been rewritten. What the memo was remains true of it: it was not a `TODO`, and it
was not a resolution.
