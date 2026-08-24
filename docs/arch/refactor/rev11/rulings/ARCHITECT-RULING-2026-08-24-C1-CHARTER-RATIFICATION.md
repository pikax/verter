---
ruling_id: "C1-CHARTER-RATIFICATION-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["C1"]
source_file: "ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFICATION.md"
summary: "Targeted confirmation on 8799580e161, taken after the repair ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFIABILITY.md Q1 named was made. It rules that the single controlling sentence added to the C1 charter exactly incorporates ARCH-ADDENDUM-C1-THREE-GAPS.md's sealing, move/stay/split and closed-gateway dispositions, making them govern conflicting charter text without overreach; and that this ruling ratifies the C1 charter at the reviewed tip. This is the ratification of record for the C1 charter. It ratifies the charter text only — it accepts no block, unlocks nothing, and leaves the Q2 (Stage 2 execution plan) and Q3 (trunk-side authorization) findings of the preceding ruling open. The receipt records RESULT: PASS with no findings."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the ruling seat's verdict verbatim. The seat was acting under explicit maintainer delegation and its verdict is the recorded decision. The source artifact contained the ruling body twice (an echoed final turn from the producing tool, with a `tokens used` line and its count between the copies); the two copies were verified byte-identical before one was landed and the echo dropped. The charter's own `**Status:**` header still reads DRAFT and is deliberately left that way: the ratification lives in this ruling and in the digest-bound authority registry, not in the charter's own prose, and the registry gate never classifies a charter by its Status line. The reviewed tip's charter bytes are the bytes now on trunk."
---

# Architect ruling — confirmation that the C1 charter's controlling sentence is exact, and ratification of the charter at the reviewed tip

**Date:** 2026-08-24
**Status:** RATIFIED — architecture ruling issued under explicit maintainer
delegation; the ruling seat's verdict is the recorded decision.
**Authority:** architecture ruling seat, lane `architecture-c1-charter-confirm`,
dispatched against `8799580e16165897ce1dac3f1cc16f8bd42431a4` under the
delegated ruling authority recorded for this program.
**Supersedes:** none.

## Provenance

- **Reviewed sha:** `8799580e16165897ce1dac3f1cc16f8bd42431a4` — a commit on
  `block/module-resolver-core`, and an ancestor of that branch's tip. It is
  not an ancestor of `program/architecture-lock`; nothing else from it is on
  trunk.
- **Charter bytes ratified.** At the reviewed sha the charter is blob
  `d49ea886f09bfa322e77649d92f922055f63a803`, sha256
  `99c0beb6ac0c36fb56a0c9ae13228db7e5b84b11da21089b79508db17b3cfbff`. Those
  exact bytes were taken from the branch and placed on trunk in the same
  change that lands this record, so the trunk charter, the reviewed charter
  and the `C1-CHARTER` digest registered in
  `docs/arch/architecture-lock/ledger/authority-registry.toml` are one and
  the same bytes, and stay so across the block's eventual landing.
- **Delta ratified.** Against trunk's prior text the change is a seven-line
  insertion and nothing else — one controlling sentence, no deletions, no
  status change. The charter's `**Status:**` header still reads DRAFT and is
  deliberately unchanged: a block branch does not ratify its own charter, and
  the authority registry never classifies a CHARTER document by its own
  Status prose (only AMENDMENT and RULING documents are read that way). The
  ratification is this ruling plus the registry's digest binding.
- **Lane:** `architecture-c1-charter-confirm`. **Result:** PASS, findings none.
- **Result artifact.** The lane's result artifact is held out of tree in the
  session verification area. It is not committed, and by standing rule it is
  not cited here by path. It is a bare verdict, not a
  `scripts/orchestration/check-results.mjs` result directory, so no receipt
  re-run is recorded for this lane; the receipt itself is reproduced verbatim
  below.
- **Delegation.** The ruling seat was acting under explicit maintainer
  delegation. Its verdict is the recorded decision; this document transcribes
  it and does not re-decide any part of it.
- **Duplicate-copy handling.** The source artifact carried the ruling body
  twice — an echoed final turn from the producing tool, separated by a
  `tokens used` line and its count. The two copies were compared and are
  byte-identical (sha256
  `9e291221291168ca9420233fee1617c7b9cff27f36dda00c9a17e39c7d51603c` for
  each). One copy is landed below verbatim; the echo, the `tokens used` line,
  and its count are dropped.

## What this ruling does not decide

It ratifies the charter text at the reviewed tip. It accepts no block, moves
no ledger status, and unlocks nothing: C1 remains `LOCKED` with an unsatisfied
predecessor set. The two other findings of the preceding ruling — Stage 2's
missing separately ratified, digest-bound execution plan, and the requirement
that authorization be authored trunk-side by the registry/ledger owner —
remain open against C1 and are not touched here.

## The ruling

1. Yes. The single controlling sentence exactly incorporates the addendum’s sealing, move/stay/split, and closed-gateway dispositions, making them govern conflicting charter text without overreach.
2. Yes. This ruling ratifies the C1 charter at the reviewed tip.

## Receipt

```text
===VERTER-RECEIPT-BEGIN===
LANE: architecture-c1-charter-confirm
RESULT: PASS
REVIEWED: 8799580e16165897ce1dac3f1cc16f8bd42431a4
FINDINGS: none
===VERTER-RECEIPT-END===
```
