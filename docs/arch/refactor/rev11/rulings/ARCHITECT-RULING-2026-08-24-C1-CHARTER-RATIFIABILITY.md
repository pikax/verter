---
ruling_id: "C1-CHARTER-RATIFIABILITY-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["C1"]
source_file: "ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFIABILITY.md"
summary: "Three-question architecture ruling taken on ccda4256230, a C1 block-branch candidate. Q1 rules B: the charter is not ratifiable as written, because it retains mechanisms the binding ARCH-ADDENDUM-C1-THREE-GAPS.md invalidates; the minimal repair is ONE controlling sentence incorporating that addendum and specifying that its sealing, move/stay/split and closed-gateway dispositions supersede conflicting charter text — flipping the Status header alone is insufficient. Q2 rules B: Stage 2 requires a separately ratified, digest-bound execution plan binding an exact rebased baseline, a caller/deletion inventory, the Cargo-edge and guard transition, and atomic abort conditions; the branch's seven WIP commits are unapproved scratch and are not dispatch authority. Q3 rules A: the branch-local authority-registry edits are dropped entirely — a block branch may only inherit the trunk-owned registry byte-for-byte through rebase, and authorization is authored trunk-side by the registry/ledger owner once the governing documents and prerequisites are valid. The receipt records RESULT: FAIL with five P1 findings."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the ruling seat's verdict verbatim. The seat was acting under explicit maintainer delegation and its verdict is the recorded decision. The source artifact contained the ruling body twice (an echoed final turn from the producing tool, with a `tokens used` line and its count between the copies); the two copies were verified byte-identical before one was landed and the echo dropped. RESULT: FAIL is the receipt's finding count, not a defect in the ruling — the five P1 findings are open blockers carried by the reviewed candidate. Q1's named repair was subsequently made on the block branch and ratified at the reviewed tip by ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFICATION.md; Q2 and Q3 remain open against C1."
---

# Architect ruling — three questions on C1: whether its charter is ratifiable as written, whether Stage 2 may proceed, and the branch-local registry delta

**Date:** 2026-08-24
**Status:** RATIFIED — architecture ruling issued under explicit maintainer
delegation; the ruling seat's verdict is the recorded decision.
**Authority:** architecture ruling seat, lane `architecture-c1-charter`,
dispatched against `ccda425623061aeae04602ff485bb48d1921b473` under the
delegated ruling authority recorded for this program.
**Supersedes:** none.

## Provenance

- **Reviewed sha:** `ccda425623061aeae04602ff485bb48d1921b473` — a C1
  block-branch candidate ("wip(core): C1 Stage 2 step 1"). It is not an
  ancestor of `program/architecture-lock`, and the branch has since been
  rewritten, so it is not an ancestor of `block/module-resolver-core`'s
  current tip either; nothing from it is on trunk. The charter as reviewed
  is trunk's unedited `DRAFT` text (blob `4d5460f188c552c2d1ed945f5e24c9e2150a689b`).
- **Lane:** `architecture-c1-charter`. **Result:** FAIL, five P1 findings.
- **Result artifact.** The lane's result artifact is held out of tree in the
  session verification area. It is not committed, and by standing rule it is
  not cited here by path.
- **Receipt validation.** Re-run of `scripts/orchestration/check-results.mjs`
  over that artifact at transcription time reports it sound:

```text
OK      architecture-c1-charter FAIL  blockers=5 carried=0  4051B
          FINDING C1-01 | P1 | docs/arch/refactor/rev11/charters/C1.md:156 | Charter retains lifecycle and coverage mechanisms invalidated by the binding three-gaps addendum.
          FINDING C1-02 | P1 | docs/arch/architecture-lock/ledger/C1/phase7-cutover-sequencing.md:1197 | Stage 2 began without the separately ratified irreversible-cutover plan its sequencing record requires.
          FINDING C1-03 | P1 | docs/arch/architecture-lock/ledger/authority-registry.toml:584 | Unsupported TOML escapes make the branch authority registry inert.
          FINDING C1-04 | P1 | docs/arch/architecture-lock/ledger/authority-registry.toml:577 | The pinned C1 charter digest does not match the charter bytes.
          FINDING C1-05 | P1 | docs/arch/architecture-lock/ledger/program-state.toml:766 | CM1 is REVIEW, falsifying the branch authorization predicate that every C1 predecessor is ACCEPTED.
          ! the result appears 2 times, identical — an echoed final turn
ALL SOUND — 1 result(s) on ccda42562306 blockers=5 carried=0
```

- **Delegation.** The ruling seat was acting under explicit maintainer
  delegation. Its verdict is the recorded decision; this document transcribes
  it and does not re-decide any part of it.
- **Duplicate-copy handling.** The source artifact carried the ruling body
  twice — an echoed final turn from the producing tool, separated by a
  `tokens used` line and its count, which the result checker flags as "the
  result appears 2 times, identical". The two copies were compared and are
  byte-identical (sha256
  `b6e92abc86338cfb9f19419e605420320121fd43f4baac78b353a9806de167f9` for
  each). One copy is landed below verbatim; the echo, the `tokens used` line,
  and its count are dropped.

## The ruling

Q1: B. The charter retains mechanisms that the binding three-gaps addendum invalidates. The minimal edit is one controlling sentence incorporating that addendum and specifying that its sealing, move/stay/split, and closed-gateway dispositions supersede conflicting charter text. Changing only `DRAFT` is insufficient.

Q2: B. Stage 2 requires a separately ratified, digest-bound execution plan before further work. The existing migration table can be promoted cheaply, but it must bind an exact rebased baseline, caller/deletion inventory, Cargo-edge and guard transition, and atomic abort conditions. The seven WIP commits are unapproved scratch, not dispatch authority.

Q3: A. Drop the registry edits entirely. A block branch has no legitimate reason to carry a branch-local authority-registry delta; it may only inherit the trunk-owned registry byte-for-byte through rebase. Authorization must be authored trunk-side by the registry/ledger owner after the governing documents and prerequisites are valid.

## Receipt

```text
===VERTER-RECEIPT-BEGIN===
LANE: architecture-c1-charter
RESULT: FAIL
REVIEWED: ccda425623061aeae04602ff485bb48d1921b473
FINDINGS: 5
FINDING C1-01 | P1 | docs/arch/refactor/rev11/charters/C1.md:156 | Charter retains lifecycle and coverage mechanisms invalidated by the binding three-gaps addendum.
FINDING C1-02 | P1 | docs/arch/architecture-lock/ledger/C1/phase7-cutover-sequencing.md:1197 | Stage 2 began without the separately ratified irreversible-cutover plan its sequencing record requires.
FINDING C1-03 | P1 | docs/arch/architecture-lock/ledger/authority-registry.toml:584 | Unsupported TOML escapes make the branch authority registry inert.
FINDING C1-04 | P1 | docs/arch/architecture-lock/ledger/authority-registry.toml:577 | The pinned C1 charter digest does not match the charter bytes.
FINDING C1-05 | P1 | docs/arch/architecture-lock/ledger/program-state.toml:766 | CM1 is REVIEW, falsifying the branch authorization predicate that every C1 predecessor is ACCEPTED.
===VERTER-RECEIPT-END===
```
