# AMD-009 ratification packet — RATIFIED

This packet is **RATIFIED** by the designated maintainer, Carlos Rodrigues / pikax,
through the 2026-08-16
[`product ruling`](maintainer-product-ruling-no-error-on-bad-output.md). The
maintainer accepted through that binding product ruling, not by quoting the packet
accept line in chat. The ratification is bound to the reviewed package content at
`9e457ca781d3684e562d6eaea24c401e2d9849a7`. It is not BF3 acceptance and
does not unlock B2 or B3.

## Package identity

- Amendment: [`AMD-009-bf3-audit-and-immediate-correction-blocks.md`](../../amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md)
- Documentation range: `885961a76..9e457ca78`
- Packet parent: `9e457ca781d3684e562d6eaea24c401e2d9849a7`
- Worktree branch: `work/bf3-implementation`

The harness test commits after `b6aa54699` are not amendment text. In particular,
`b4f497fb6` and `273584e57` are harness fixes; `9e457ca78` is the later AMD-009
documentation correction. This packet is the only addition after `9e457ca78`.

## Effect and limits

Ratification reshapes BF3 into a conformance-exhaustion and
correction-dispatch audit; create BS0, BA0, BCSS0, and BRT0 as mandatory B2/B3
predecessors; and supersede BF3's retraction procedure, AMD-006 §4 and §8.1,
AMD-005 §5 and §12 plus their conflicting recorded-ratification effect, and the
`BF3-RET-*` production-record scheme, exactly as bounded by AMD-009 §2 and §7.

Ratification must **not** accept BF3, unlock B2 or B3, authorize or write a
production retraction path, land this worktree onto `program/architecture-lock` by
itself, write `program-state.toml`, or change `current_block` away from `BF3`.

## Review summary

- H-delta `4b2bf8d94..a1ef593d1`: conformance **PASS**; architecture P1s were
  classified **REJECT/DEFER**, except the adversarial `NAVIGATOR` and
  `UNTRACKED-PATH` findings, which were fixed.
- Harness `a1ef593d1..885961a76`: adversarial **PASS**; architecture P1s for
  rejected-promise memoization and Windows-shaped path identity were fixed in
  `b4f497fb6` and `273584e57`.
- AMD-009 `885961a76..b6aa54699`: architecture **PASS**; adversarial P1s for
  AMD-005 supersession and CSS-1/AT-2 characterization-versus-target wording were
  fixed in `9e457ca78`.
- Those later fixes did **not** receive a full clean 3/3 re-review.

## Recorded maintainer ratification

The original packet requested that the maintainer quote exactly one line. The
maintainer instead accepted via the binding product ruling cited above. The
following reviewed accept line remains the exact scope record for package commit
`9e457ca781d3684e562d6eaea24c401e2d9849a7`; it is not represented as a
verbatim maintainer chat response.

**Reviewed accept line:**

> Ratify AMD-009 for worktree commit `9e457ca781d3684e562d6eaea24c401e2d9849a7` on exactly the scope and terms of AMD-009 §7: supersede BF3's safety-retraction title, objective, procedures, exits, abort logic, ledger note, and `BF3-RET-*` production-record scheme; supersede AMD-005 §5 and §12 and the conflicting AMD-005 §15.1 recorded-ratification wording, and AMD-006 §4 and §8.1, only as §7 states; ratify BF3 as a conformance-exhaustion and correction-dispatch audit; create BA0, BS0, BCSS0, and BRT0 after BF3 and require all four, together with BV0 and BF3, as mandatory predecessors of B2 and B3; authorize no production defect-recognition refusal or retraction path; keep B2 and B3 locked until every predecessor is accepted; and leave the separate `svelte@5.56.3` pin migration unauthorized.

**Reviewed reject line — not selected:**

> Reject AMD-009; do not apply the ledger snippet.

Ratification does **not** accept BF3 or any correction block and does **not** unlock
B2/B3. The returning program orchestrator may apply **only** the ledger snippet in
the historical [`ratified package record`](amd009-unratified-package.md); this
docs-only ratification record does not apply it.
