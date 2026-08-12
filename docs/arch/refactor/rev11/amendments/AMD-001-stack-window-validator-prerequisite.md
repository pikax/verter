# AMD-001 — Stack-Window Validator Is a Prerequisite for the D1/D2 Path

**Status:** Registered amendment (repository-local; NOT part of the verbatim-reconstructed
authority set — see [`../PROVENANCE.md`](../PROVENANCE.md)). **§1 amended** (timing clause
only, §§2-4 unchanged) — see "Amendment to §1's timing (rescope)" below.
**Registered in:** [`../README.md`](../README.md) (the `ORCHESTRATOR.md` §3 read-order
item 1) and [`../evidence/maintainer-rulings.md`](../evidence/maintainer-rulings.md).
**Amends the execution plan around:** [`../charters/A6.md`](../charters/A6.md),
[`../program.md`](../program.md) §7 (D1), [`../program-dag.toml`](../program-dag.toml)
(the `D1` block and `D2 -> D1` edge),
[`../contracts/stacked-prs.md`](../contracts/stacked-prs.md) §3.2.
The verbatim authority files themselves are NOT edited: they are byte-for-byte
reconstructions from the digest-verified consolidated master, and editing them would
void the 67/67 fidelity attestation. This amendment is the recorded delta.

## The defect

`scripts/validate-program-state.mjs` fails closed on every path it does not model: any
block that has begun (`READY`/`IN_PROGRESS`/`REVIEW`/`ACCEPTANCE_RECOMMENDED`/
`ACCEPTED`/`PRIVATE_CHECKPOINT`) while a direct predecessor is in `PRIVATE_CHECKPOINT`
is REJECTED, because
the conditions under which a `PRIVATE_CHECKPOINT` predecessor legally satisfies
sequencing live in the stack-window model (`contracts/stacked-prs.md` §3.2), which the
program-state validator does not implement.

But `program-dag.toml` (`D2.predecessors = ["D1"]`, `D1.class =
"foundational-private-checkpoint"`) and `program.md` §7 ("`D1` may receive checkpoint
review approval but must not merge or release independently from `D2`") make
`D1 PRIVATE_CHECKPOINT -> D2` the CANONICAL atomic path — `contracts/stacked-prs.md`
§3.2 says so verbatim: "`D1`/`D2` is the canonical case: D1 is a private checkpoint; D2
is the sole acceptance and landing unit."

So the ledger's only validator categorically rejects a state the plan REQUIRES the
program to pass through, and no block's charter owns delivering the model that would
make that state validatable. Fail-closed is safe only when the unsupported path has a
prerequisite that delivers its model; without one, fail-closed is a trap: when D2
begins, the mandatory always-green validator (`governance.md` — it "must pass after
every transition") goes red with no legal move to make it green except weakening the
validator ad hoc, unreviewed, at the worst possible moment (mid-atomic-cutover).

## The amendment

1. **Prerequisite (as amended — see rescope below).** Before ANY post-A6 stacked delivery
   is permitted — any stack window is opened, any block claims the contingent stacked-work
   exception on a `PRIVATE_CHECKPOINT` predecessor, and in particular before `D1` may enter
   `PRIVATE_CHECKPOINT` — the accepted candidate immediately preceding that event must
   deliver:
   - a **Node stack-window validator** (the `tools/validate_stack_window.py`
     reimplementation under maintainer ruling R-4) validating stack-window records
     against `contracts/stacked-prs.md`;
   - **composite program-state cross-validation**: the stack-window validator and
     `scripts/validate-program-state.mjs` run against each other's records (the
     `--current-program-state` cross-check named in `contracts/stacked-prs.md`), so
     the mutable ledger and the immutable snapshot cannot silently diverge;
   - **CI wiring** for the new validator's test suite, in the same
     `test:scripts`/path-filter pattern used for the program-state validator suite;
   - a **discriminating D1/D2 transition test**: a fixture where `D1` is
     `PRIVATE_CHECKPOINT` inside a validated `ATOMIC_REVIEW` window with `D2` as its
     `acceptance_block_id` VALIDATES, and the same state without the window (or with a
     mismatched snapshot, a non-D2 acceptance block, or a landed-independently D1)
     REJECTS.
2. **The acceptance rule for the composite validator.** The eventual composite
   validation must accept a begun `D2` over a `PRIVATE_CHECKPOINT` `D1` ONLY when `D1`
   is the declared private checkpoint in the same validated `ATOMIC_REVIEW` snapshot
   whose `acceptance_block_id` is `D2` (`contracts/stacked-prs.md` §3.2 — a
   `PRIVATE_CHECKPOINT` state "is valid only for the final acceptance block").
3. **The refusal stays until then.** The program-state validator's fail-closed
   rejection of begun successors of a `PRIVATE_CHECKPOINT` predecessor **must not
   simply be deleted**: it is removed only by being SUPERSEDED by the composite
   validation above, delivered and reviewed under whichever accepted candidate
   discharges the amended §1 prerequisite. Deleting or bypassing the refusal
   without that replacement recreates the unvalidated-path defect this amendment
   records.
4. **Mechanical traceability.** The `A6` context packet AND the `A6` Implementation
   Lock evidence must each NAME this amendment by identifier (`AMD-001`) and bind
   the SHA-256 (lowercase hex, over the raw bytes) of this file as it stands in
   `A6`'s base tree — recompute with
   `sha256sum docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md`.
   (The digest is quoted THERE, never inlined here — a self-digest is a fixpoint.)
   An `A6` candidate whose packet or lock evidence omits the name or the digest has
   not carried this prerequisite; the reviews must treat that as a missing required
   input, not prose to rediscover. This makes the prerequisite mechanically
   traceable from `A6`'s own record rather than dependent on a reader re-finding
   this file. This traceability duty is UNCHANGED by the §1 rescope below — A6 must
   still name and bind this amendment even though A6 itself no longer delivers the
   four artifacts.

## Amendment to §1's timing (rescope, ratified after A6 architecture review)

**Failed assumption:** §1's original text assumed A6 — an implementation-lock block whose
charter explicitly excludes "later production ownership or API cutovers" and "speculative
services" — was the correct forcing function for delivering machinery that only becomes
load-bearing once a stack window actually opens.

**Measured/source evidence:** A6's own accepted candidate unlocks exactly one successor,
`B1`, at `stack_layer 0` / depth 1 with no stack window open (`stack-window-policy.toml`).
No block between `A6` and `D1` opens a stack window under current program sequencing.
`D1` is many blocks downstream. Meanwhile §3's fail-closed refusal in
`scripts/validate-program-state.mjs` is untouched and continues to reject any begun
successor of a `PRIVATE_CHECKPOINT` predecessor — the unmodelled `D1`/`D2` path stays
CLOSED, not open, for the entire interval between A6's acceptance and whichever later
candidate delivers §1's four artifacts.

**Affected architecture/verification invariants:** §1 (timing only — WHO must deliver and
WHEN). §§2-4 are unaffected: the composite validator's acceptance rule (§2), the
must-not-bypass refusal (§3), and the mechanical-traceability duty (§4) all stand exactly
as written, including `A6`'s continuing duty to name and bind this amendment.

**Compatibility or consumer consequences:** none. No wire, cache, API, persisted artifact,
or generated output depends on the undelivered artifacts today, because nothing downstream
of A6 opens a stack window yet.

**Alternatives considered:**
1. Deliver in A6 anyway — rejected: freezes the validator's semantics against zero real
   stack-window instances, which is the same "self-declared test universe" failure
   `governance.md`'s Verification-Must-Prove-Execution rule names, under a regime where
   `performance-gates.toml`-style immutability would make a later correction a
   recalibration rather than an ordinary fix.
2. Defer with no timing change (an open debt row) — rejected: leaves §1's literal text
   pointing at `A6` while the actual delivery point drifts informally, which is exactly the
   "TODO masquerading as a disposition" pattern `CLAUDE.md`'s Explicit Finding Disposition
   rule forbids.
3. **Amend §1's timing clause (ADOPTED)** — the prerequisite now binds to the event it
   actually gates (the first post-A6 stack window opening, and unconditionally before `D1`
   enters `PRIVATE_CHECKPOINT`) rather than to a specific block identity. Whichever accepted
   candidate immediately precedes that event carries the delivery duty. `§3`'s refusal
   remains the enforcement mechanism in the interim, exactly as before.
4. Rescope AMD-001's ownership to a named future block — not adopted now because no
   concrete block between `A6` and `D1` is yet chartered to own it; §1 as amended already
   binds correctly to "whichever candidate is immediately pre-window" without requiring a
   specific block name today. A future amendment MAY name a concrete owning block once one
   is chartered.

**Work that remains valid:** all of it. A6's mechanical-traceability discharge (naming
AMD-001, binding its digest) stands. Nothing in A6's accepted candidate depended on the
four artifacts existing.

**Ruling:** ADOPTED. Recorded in `../evidence/maintainer-rulings.md`.
