# AMD-014 — B6 joins the enumerated context-packet legacy-gap exemption

**Status:** RATIFIED 2026-08-24, under
[`../rulings/ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md`](../rulings/ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md)
Q1, an architecture ruling issued under explicit maintainer delegation whose own
`**Status:**` line records the seat's verdict as the recorded decision. That
ruling is registered in
[`../../../architecture-lock/ledger/authority-registry.toml`](../../../architecture-lock/ledger/authority-registry.toml)
as `RULING-2026-08-24-SIX-WAY-B6-CM1`, digest-bound to sha256
`1b8b52fa4706cdee7d3f59cf7dd0bb34226b466e31cfd824658f3a3083effafa`. See §7.

**Prepared against:** local `block/b6-acceptance-records` commit
`25939ea688602b65d5b71cfaae16ea4aebdd14ef`, tree
`bd7d5d8224dd558d022b1e4ad6a8d4356683251c`, whose history contains the
ratifying ruling's landing commit `0dabc659798292c034f3a86db1338b511c392c3b`.
Every `file:line` citation below was read directly on that tree.

**Amends on ratification:**
[`../rulings/MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md`](../rulings/MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md)
§1 — the enumerated `context_packet_digest` legacy-gap exemption gains a fourth
block id, `B6`. That ruling's own bytes are left untouched; this amendment is
the separate instrument §1 itself requires (see §1 below). The one mechanical
consequence is the `CONTEXT_PACKET_DIGEST_LEGACY_GAP_GRANDFATHER` set at
[`../../../../../scripts/validate-program-state.mjs`](../../../../../scripts/validate-program-state.mjs)
line 1696. **It accepts no block, amends no charter, changes no block status or
acceptance field, and adds or retires no DAG or ledger block.**

## 1. Why this amendment exists rather than a bare validator edit

`MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md` §1 grandfathers exactly
three ledger rows — `BV2`, `B5`, `CM1` — for the `context_packet_digest` field,
and closes the set against silent growth in its own words:

> "A fourth block cannot join this exemption by resembling BV2/B5/CM1; it can
> only join by an explicit amendment to this ruling naming it and stating why
> its own gap is equally unreconstructable."

Editing the validator's set without that instrument would contradict the text
the set is cited to. This document is that instrument. It names `B6` (§2) and
states why `B6`'s own gap cannot be honestly reconstructed (§3), on `B6`'s own
facts. **Resemblance to `BV2`/`B5`/`CM1` is expressly not the argument here**,
because §1 expressly rejects resemblance as grounds.

## 2. The block named

`B6` — "Prepared-first, prepared-repeat and direct-batch compiler-core routes",
the block authorised at
[`../../../architecture-lock/ledger/authority-registry.toml`](../../../architecture-lock/ledger/authority-registry.toml)
lines 541-546, whose ledger row is
[`../../../architecture-lock/ledger/program-state.toml`](../../../architecture-lock/ledger/program-state.toml)
lines 740-762.

## 3. Why B6's own gap is unreconstructable

1. **No packet was produced at dispatch.** `B6` was dispatched and its
   implementation landed without the immutable worker context packet
   [`../governance.md`](../governance.md) line 183 requires ("Every worker
   receives one immutable context packet and one writable worktree/branch").
   The ratifying ruling records this as its own Q1 finding: *"B6 was dispatched
   and landed without the mandatory immutable context packet."*

2. **None survives.** [`../evidence/B6/`](../evidence/B6/) holds eleven files —
   `deferred-to-publication-owner.md`, `mutation-replay-recipes.md`, and the
   nine-file `cell-lock/` subtree — and no `context-packet.md` or equivalent
   immutable dispatch packet among them. Sixteen other blocks carry one at
   `evidence/<ID>/context-packet.md`; `B6` has none, and
   `git log --all --diff-filter=A -- docs/arch/refactor/rev11/evidence/B6/context-packet.md`
   returns nothing on any branch — the file was never added and then removed,
   it was never written.

3. **Writing one now would be a falsified record, not a repair.** A packet
   authored after implementation is not a record of what was supplied at
   dispatch. `MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md` §1 states this
   directly for the class: "Reconstructing one now, after implementation, would
   not be a record of what was supplied at dispatch — it would be a fabricated
   input artifact backdated to look like one, which Stub Prevention and
   Verification Must Prove Execution both already forbid in spirit." Both
   `CLAUDE.md` rules apply to `B6` on their own terms: an artifact produced to
   satisfy a gate mechanically, whose content the gate then treats as proof of
   an execution that did not happen that way, is precisely what they forbid.

4. **The ratifying authority reached the same conclusion.** Q1 of
   `ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md` rules: *"Rule A: authorize a
   narrow amendment adding B6 to the enumerated legacy-gap set; B would
   preserve the historical violation as a permanent deadlock. This changes
   governance, not B6's charter; the missing historical packet remains
   unrecoverable."*

There is therefore no honest edit to `B6`'s row that closes this field: the
digest has no artifact to bind to, and no artifact can now be produced that
would be a true record. The alternative the ruling weighed and rejected —
leaving the field unsatisfiable — deadlocks `B6` permanently at its first
evidence-bound transition, which is a governance defect rather than a
correctness safeguard.

## 4. The exemption's exact scope

This exemption covers **only** the `context_packet_digest` field, and **only**
for block id `B6`. Every other required field on `B6`'s row stays fully
enforced, exactly as for any other block: `base_sha`, `candidate_sha` and
`candidate_tree`, `charter_digest`, `evidence_digest`, the three review
mandates (`conformance_review`, `architecture_review`, `adversarial_review`,
each with its `*_reviewed_sha`), and `accepted_sha`/`accepted_tree`. Review
mandates in particular are untouched here, as §4 of the ruling being amended
already holds for the original three: there is no review-mandate override for
`B6`.

The live requirement in §1 is likewise untouched. A block dispatched from today
onward that reaches an evidence-bound status without a real, digest-bound
context packet still fails the check exactly as before. The exemption remains
narrow and enumerated: it names four block ids and nothing else, so a gap
discovered tomorrow inherits nothing from it and can only join by a further
explicit amendment on its own facts.

## 5. The mechanical change

[`../../../../../scripts/validate-program-state.mjs`](../../../../../scripts/validate-program-state.mjs)
line 1696:

```diff
-  const CONTEXT_PACKET_DIGEST_LEGACY_GAP_GRANDFATHER = new Set(["BV2", "B5", "CM1"]);
+  const CONTEXT_PACKET_DIGEST_LEGACY_GAP_GRANDFATHER = new Set(["BV2", "B5", "CM1", "B6"]);
```

with an inline comment above it citing this amendment as the act that added
`B6`, in the voice of the existing comment block at lines 1677-1695.

The gate this touches: the `EVIDENCE_BOUND` status set at line 1698 —
`REVIEW`, `ACCEPTANCE_RECOMMENDED`, `ACCEPTED`, `PRIVATE_CHECKPOINT` — selects
which rows are checked, and lines 1724-1725 are the enforcement the set
exempts:

```js
      if (!CONTEXT_PACKET_DIGEST_LEGACY_GAP_GRANDFATHER.has(id)) {
        requireDigest("context_packet_digest");
      }
```

`B6`'s row is `LOCKED` today, which is not an `EVIDENCE_BOUND` status, so the
edit changes no current validator outcome. Its effect is forward: it removes
the deadlock that would otherwise refuse `B6` at every evidence-bound
transition it is required to make.

## 6. What this does NOT do

- It does not accept, unlock, or dispatch `B6`, or move its status.
- It does not write a `context_packet_digest` value into `B6`'s row; the field
  stays empty, and the amendment is what makes that honest rather than
  outstanding.
- It does not amend [`../charters/B6.md`](../charters/B6.md) or any other
  charter, contract, ADR, or capability-matrix cell.
- It does not change the bytes of
  `MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md`, and therefore does not
  disturb that ruling's registered digest. §1 asked for a separate explicit
  amendment; this is that document, and the ruling stands as written.
- It does not touch any other gate in `scripts/validate-program-state.mjs`, or
  any review mandate, for `B6` or any other block.
- It does not create a second `[[authorization]]` record for `B6`. `B6`'s
  authorization already exists (`B6-CHARTER` + `AMD-011-C2-B6-PREDECESSOR-EDGE`,
  ratified 2026-08-22). This amendment is registered as a `[[document]]` for
  citability and digest-binding only.

## 7. Ratification

**RATIFIED**, 2026-08-24, by the architecture ruling seat acting under explicit
maintainer delegation, in Q1 of
[`../rulings/ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md`](../rulings/ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md)
(landed at `0dabc659798292c034f3a86db1338b511c392c3b`; registry id
`RULING-2026-08-24-SIX-WAY-B6-CM1`, sha256
`1b8b52fa4706cdee7d3f59cf7dd0bb34226b466e31cfd824658f3a3083effafa`). That
ruling's Q1 authorises exactly this instrument — "a narrow amendment adding B6
to the enumerated legacy-gap set" — and records the substantive finding this
document carries into §1 of the amended ruling: the missing historical packet
remains unrecoverable.

## 8. Verification

1. Apply the §5 diff and register this document in
   `docs/arch/architecture-lock/ledger/authority-registry.toml` with its exact
   sha256, in the same change — a stale digest is itself a violation
   (`scripts/validate-program-state.mjs:2296-2308`).
2. ```
   node scripts/validate-program-state.mjs \
     --dag docs/arch/refactor/rev11/program-dag.toml \
     --state docs/arch/architecture-lock/ledger/program-state.toml \
     --mode live \
     --authority docs/arch/architecture-lock/ledger/authority-registry.toml
   ```
   The `--authority` flag is required for this amendment's own registry row,
   digest binding, and `**Status:**` parse to be exercised at all. Expected
   result: the run's violation count is unchanged from before the change —
   `CM1`'s landing-order rehearsal finding remains the sole violation, and no
   new one appears.
3. `node --test scripts/validate-program-state.test.mjs` — the validator's own
   suite, run because §5 edits executable code.
