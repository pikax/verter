# AMD-009 ratification packet — RATIFIED on the full §7 scope

**Status:** **RATIFIED**, on the full scope and terms of
[AMD-009](../../amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md) §7, by the
designated maintainer, Carlos Rodrigues / pikax, on 2026-08-16.

## The ratification act

The maintainer's own text, given directly and reproduced verbatim at
[`maintainer-ruling-section7-ratification.md`](maintainer-ruling-section7-ratification.md):

> Ratify AMD-009 §7 in full: BF3 is a conformance-exhaustion and correction-dispatch audit;
> create BA0, BS0, BCSS0, and BRT0 as mandatory B2/B3 predecessors together with BV0 and BF3;
> supersede the retraction procedure and the conflicting AMD-005/AMD-006 text as AMD-009 §7
> states; authorize no production error-on-bad-output path; do not accept BF3 or unlock B2/B3.

**Ratifier:** Carlos Rodrigues &lt;carlos@hypermob.co.uk&gt; (GitHub: `pikax`), designated
maintainer.

**What the act ratifies, and what it does not bind.** The act ratifies **§7's TEXT**. It names
no commit and blesses no tree; it PREDATES the post-cure content recorded below. The content
identity in the next section is therefore a RECORDING device — the exact bytes the §7 direction
is being applied to at the time of landing — not something the maintainer inspected byte by byte.
It is not backdated onto the superseded reviewed package commit
`9e457ca781d3684e562d6eaea24c401e2d9849a7`.

**What the act explicitly withholds.** BF3 is NOT accepted and `maintainer_decision` stays
`PENDING`. B2 and B3 are NOT unlocked. BA0, BS0, BCSS0 and BRT0 are created, NOT accepted; each
still owes its own work. No production error-on-bad-output path is authorized in any block.
Ratifying §7 settles AUTHORITY; it does not make this block's candidate correct, and it is not
license to green any outstanding verification.

## Correction of the record

An earlier version of this packet recorded the package as RATIFIED with the full effect of
AMD-009 §7 on the strength of the 2026-08-16
[`product ruling`](maintainer-product-ruling-no-error-on-bad-output.md) alone. That was an
**overstatement**, and it is recorded here rather than quietly removed. The direct act above is
what cured it.

The product ruling's own "Ratification effect" section says it "ratifies the AMD-009 §1 and
§2 no-retraction direction", that it does not accept BF3, and that "the live program ledger
is unchanged by this evidence record." The executed package nevertheless applied full §7
structural effect: four new blocks, a `program-dag.toml` amendment, five charter rewrites,
and a ledger write. A bounded closing re-attestation found the discrepancy on all three
mandates, and an independent governance consult confirmed that the §7 effect did not stand
on the product ruling alone and that no track-level actor could cure it.

The maintainer then ruled, on 2026-08-16, that the intended ratification **was** the full
§7 — that the structural reshape stands as intended and the defect is one of RECORDING, not
of substance — and directed the cure order this packet completes, before issuing the direct
ratification act quoted above:

1. fix the verified in-delta test defects first, so the package binds to correct content;
2. re-review the charters that changed after the earlier bound identity `9e457ca78` and
   never received a later acceptance;
3. rebind the package to the resulting content and record an explicit §7 ratification
   against that new bound identity;
4. only then re-attest and propose BF3 acceptance.

**The product ruling remains valid for exactly what it says.** It is a genuine maintainer
artifact, it is not rewritten or weakened here, and it continues to be the authority for
AMD-009 §1/§2 — the no-production-error product boundary. What is superseded is the
*reading* of it as full-§7 ratification.

## Rebound package identity

The package is exactly these seven documents. Its identity is content-addressed rather than
commit-addressed, so it is fixed before, and independent of, the commit that lands it.

<!-- PACKAGE-MANIFEST-START -->

**Combined package digest (SHA-256):** `0bdef4b095cf6fac264a133507c4a835e4cb98a86e0f1587383c725e1c9066b8`

| package file | git blob OID |
|---|---|
| `docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md` | `a4f9143073af8bc274ecc8bcd13afd93f4b9cdf0` |
| `docs/arch/refactor/rev11/charters/BA0.md` | `c1bc7e1b2c086e79cc6afe110afc51010c368dde` |
| `docs/arch/refactor/rev11/charters/BCSS0.md` | `76f5c8e74efe0afebdd41bb3c68516c279d141d4` |
| `docs/arch/refactor/rev11/charters/BF3.md` | `589cfdb7a599498a5450b5e94db37b9ca601f334` |
| `docs/arch/refactor/rev11/charters/BRT0.md` | `66ff6f5d8aaca6ad0b24b16f0d06b43bb91b1ca6` |
| `docs/arch/refactor/rev11/charters/BS0.md` | `be3b9d863d221629d1a0dba0dcc8ab89a2266d6b` |
| `docs/arch/refactor/rev11/program-dag.toml` | `20fe75c784a3196f14fe770b35477631e1db93a2` |

<!-- PACKAGE-MANIFEST-END -->

Reproduce the identity from a checkout of this tree with:

```sh
for p in \
  docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md \
  docs/arch/refactor/rev11/charters/BA0.md \
  docs/arch/refactor/rev11/charters/BCSS0.md \
  docs/arch/refactor/rev11/charters/BF3.md \
  docs/arch/refactor/rev11/charters/BRT0.md \
  docs/arch/refactor/rev11/charters/BS0.md \
  docs/arch/refactor/rev11/program-dag.toml
do printf '%s %s\n' "$(git hash-object "$p")" "$p"; done | shasum -a 256
```

This packet, the maintainer ruling it records, and the evidence files under this directory
are NOT part of the package identity — they record it, so including them would be circular.

## How §7's changed-byte clause is satisfied, and its stated limit

AMD-009 §7 ends: *"Any challenged or changed byte requires fresh reviewed identities and the
designated maintainer's explicit acceptance."* Against the post-cure content that clause is
discharged as follows, and the residual is stated rather than papered over.

1. **Fresh reviewed identity.** The content manifest above — seven files, their git blob OIDs and
   a combined digest — is computed over the exact post-cure bytes and is reproducible from any
   checkout.
2. **Independent review of every changed byte.** Every post-binding charter change received a
   bounded review by two independent external seats
   ([`charter-drift-review.md`](charter-drift-review.md)), and the whole cure delta received a
   bounded per-mandate re-attestation ([`reattestation.md`](reattestation.md)). No changed byte in
   this package is unreviewed.
3. **Explicit maintainer act.** The maintainer's ruling ORDERED this rebind — fix, re-review,
   rebind, record — and the maintainer then issued the act quoted above, ratifying AMD-009 §7 in
   full, in that context.

**The limit, stated plainly.** The act ratifies §7's DIRECTION. The maintainer did not inspect
these bytes individually and the act names no digest, so this record claims exactly that and no
more: a ratified direction, an independently reviewed and content-addressed package, and no
byte-level maintainer inspection. Both review seats raised this distinction; their reports are
committed beside this file rather than resolved by rewording. It is also why the act withholds
block acceptance and why `maintainer_decision` stays `PENDING` — settling AUTHORITY is not the
same act as accepting a candidate.

## Effect and limits

Ratification reshapes BF3 into a conformance-exhaustion and correction-dispatch audit;
creates BS0, BA0, BCSS0 and BRT0 as mandatory B2/B3 predecessors; and supersedes BF3's
retraction procedure, AMD-006 §4 and §8.1, AMD-005 §5 and §12 plus their conflicting
recorded-ratification effect, and the `BF3-RET-*` production-record scheme — exactly as
bounded by AMD-009 §2 and §7.

Ratification must **not** accept BF3, accept any correction block, unlock B2 or B3,
authorize or write a production retraction path, or by itself write
`program-state.toml`. B2 and B3 remain LOCKED until BV0, BF3, BA0, BS0, BCSS0 and BRT0 are
all accepted, and B2 and B3 still serialize.

## Review record

**Reviews bound to the earlier identity** (historical; they qualified the package for the
maintainer's decision and authorized nothing on their own):

- H-delta `4b2bf8d94..a1ef593d1`: conformance **PASS**; architecture P1s classified
  REJECT/DEFER except the adversarial `NAVIGATOR` and `UNTRACKED-PATH` findings, which were
  fixed.
- Harness `a1ef593d1..885961a76`: adversarial **PASS**; architecture P1s for rejected-promise
  memoization and Windows-shaped path identity fixed in `b4f497fb6` and `273584e57`.
- AMD-009 `885961a76..b6aa54699`: architecture **PASS**; adversarial P1s for AMD-005
  supersession and CSS-1/AT-2 wording fixed in `9e457ca78`.
- Those later fixes did not receive a full clean 3/3 re-review. That is precisely why the
  post-binding drift needed its own review.

**The post-binding drift review** — the five charters that changed after `9e457ca78`
(`BA0.md`, `BCSS0.md`, `BF3.md`, `BRT0.md`, `BS0.md`) — is recorded at
[`charter-drift-review.md`](charter-drift-review.md). Two independent external review seats
read the exact drift against AMD-009 §7's authorized scope, the ratified
[`dispositions.md`](dispositions.md), and the tests as they stand in the tree.

## What the returning program orchestrator may do

The program orchestrator owns `docs/arch/architecture-lock/ledger/program-state.toml`; this
packet does not write it. The four correction-block rows are already present in the live
ledger and in
[`../../templates/program-state.template.toml`](../../templates/program-state.template.toml),
and both validate clean in their respective modes. The ledger transition proposed for BF3
itself is recorded in [`landing-record.md`](landing-record.md).
