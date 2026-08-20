---
ruling_id: "BF3-SECTION7-RATIFICATION"
type: "maintainer-directive"
date: "2026-08-16"
date_source: "stated"
binds: ["BF3", "AMD-009", "AMD-010", "BA0", "BS0", "BCSS0", "BRT0"]
source_file: "MAINTAINER-RULING-BF3-SECTION7.md"
summary: "Cures a recording defect: the only recorded maintainer act (a narrow product ruling) had been over-read to license full AMD-009 §7 structural effect (four new blocks BA0/BS0/BCSS0/BRT0, a program-dag.toml amendment, five charter rewrites, a ledger write) that it did not actually authorize. Ruling 1 confirms the intended ratification WAS the full §7 — the structural reshape stands, cured by direct maintainer act rather than unwound — but requires fixing five verified in-delta test defects, re-reviewing charters changed after the previously-bound package identity, and rebinding before BF3 can be acceptance-recommended. Ruling 2: the audit amendment keeps identifier AMD-009; the separately-ratified B3/B2 option-conversion amendment is renumbered AMD-010 (substance unchanged, no re-ratification needed). Includes a verbatim maintainer ratification act for AMD-009 §7 in full."
supersedes:
  - document: "evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md (in-tree, not part of this migration)"
    claim: "The over-claimed reading that this narrow product ruling alone licensed full §7 structural effect. The product ruling's own actual text remains valid for exactly what it says."
superseded_by: []
contradicts: []
notes: "Explicitly withholds: BF3 is NOT accepted, B2/B3 are NOT unlocked, no production error-on-bad-output path is authorized, and BA0/BS0/BCSS0/BRT0 are created but not accepted. This is the document renumbering part of AMD-009 (this file's own MAINTAINER-RULING-AMD-009.md sibling) into AMD-010 — see that document's notes."
---

# Maintainer ruling — full §7 ratification and amendment renumbering (2026-08-16)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
Solicited by the program orchestrator after a bounded closing re-attestation returned BLOCKING
on all three mandates, and an independent consult ruled the executed structural changes did not
stand as ratified by the recorded product ruling alone.

## The defect being cured

The only recorded maintainer act was
`evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md`, a PRODUCT ruling whose own
text limits it to the AMD-009 §1/§2 no-retraction direction and states explicitly that it does
NOT accept the block and leaves the live ledger unchanged. The executed package nevertheless
applied full §7 structural effect: four new blocks (BA0, BS0, BCSS0, BRT0), a `program-dag.toml`
amendment, five charter rewrites, and a ledger write.

## Ruling 1 — intent was FULL §7

The maintainer confirms the intended ratification was the FULL AMD-009 §7, not §§1-2 alone. The
structural reshape stands as intended: BF3 becomes a conformance-exhaustion and
correction-dispatch audit; BA0/BS0/BCSS0/BRT0 exist as mandatory B2/B3 predecessors; the
supersessions listed in §7 take effect.

This is a RECORDING defect, not a substantive one. Cure it — do not unwind the work:

1. Fix the five verified in-delta test defects FIRST, so the package binds to correct content.
2. Re-review the five charters that changed after the previously bound package identity
   `9e457ca78` and never received a later acceptance.
3. REBIND the package to the resulting content and record an explicit maintainer ratification
   of §7 against that new bound identity, superseding the over-claimed reading of the product
   ruling. The product ruling itself remains valid for what it actually says.
4. Only then re-attest and propose BF3 acceptance.

The live ledger currently records BF3 as BLOCKED with all three mandates BLOCKING. That status
stands until the cure completes and a genuine clean re-attestation exists.

## Ruling 2 — amendment renumbering

The audit amendment KEEPS the identifier `AMD-009`; it is already committed and cross-referenced
from many evidence files. The separately-ratified amendment covering B3's option-conversion
ownership, B2's E1 parse-facet exit, and B3's predecessor correction (3 documents / 4 deltas,
ratified earlier the same day, draft held in this scratchpad) is renumbered to **AMD-010** and
lands under that identifier. Substance is unchanged and it does NOT require re-ratification —
this is a clerical renumbering only.

## Unchanged by this ruling

- The four correction blocks' own acceptance still requires their own work; nothing here accepts
  BA0, BS0, BCSS0, or BRT0.
- B2 and B3 stay LOCKED until BV0, BF3 and all four correction blocks are accepted.
- JS-1 and Verter's Vue public TypeScript surface remain OUT OF PROGRAM SCOPE, maintainer-owned
  post-program.
- B2 and B3 still SERIALIZE (B2 first, then B3 rebases onto B2's accepted tree).

## VERBATIM MAINTAINER RATIFICATION ACT (2026-08-16)

Supersedes the paraphrased intent capture above. This is the maintainer's own text, given
directly in session, and is the authoritative ratification record for AMD-009 §7:

> Ratify AMD-009 §7 in full: BF3 is a conformance-exhaustion and correction-dispatch audit;
> create BA0, BS0, BCSS0, and BRT0 as mandatory B2/B3 predecessors together with BV0 and BF3;
> supersede the retraction procedure and the conflicting AMD-005/AMD-006 text as AMD-009 §7
> states; authorize no production error-on-bad-output path; do not accept BF3 or unlock B2/B3.

Ratifier: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.

### What this act settles

The AUTHORITY question is now closed. AMD-009 §7 has full ratified effect: the BF3 reshape,
the four correction blocks and their mandatory B2/B3 predecessor edges, and the listed
AMD-005/AMD-006 supersessions all stand. The earlier over-claimed reading of the product
ruling is cured by this direct act; the product ruling itself remains valid for exactly what
its own text says.

### What this act explicitly WITHHOLDS

- BF3 is NOT accepted. `maintainer_decision` stays `PENDING`.
- B2 and B3 are NOT unlocked.
- No production error-on-bad-output path is authorized, in any block.
- BA0, BS0, BCSS0 and BRT0 are created, NOT accepted; each still owes its own work.

### What still remains before BF3 can be acceptance-recommended

Ratifying §7 settles authority; it does not make the candidate correct. Still outstanding:
the five verified in-delta test defects, the post-binding charter-drift review, the template
ledger fix, and a genuine clean bounded re-attestation. Ratification must NOT be read as
license to green those.
