---
ruling_id: "LSP-DURABLE-FENCE-OWNERSHIP-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["H2", "H3", "K3"]
source_file: "ARCHITECT-RULING-2026-08-24-LSP-DURABLE-FENCE-OWNERSHIP.md"
summary: "Three-question architecture ruling taken on 7dac2b614, the tip of the unowned branch block/lsp-durable-fence — ten commits closing a torn-pair defect in the LSP durable carrier-publication gateway, on a branch that has no DAG node, no ledger row and no authorization. Q1 rules the work belongs to planned block H2, which owns provider bindings, carrier lifecycle and exact readiness receipts; H3 consumes that basis and K3 only audits residue, so no new block is required and the branch remains off trunk as H2 evidence. Q2 rules the design architecturally sound on its face: a project-bound gateway producing one verified (ide, pin) pair preserves single authority, and aborting a stale durable publication is the required fail-closed behaviour, because publishing a narrowed mixed-state result would violate the external-TS contract. Q3 rules B: preserve the branch as evidence and rederive the minimum fix within H2 — the defect closure is load-bearing correctness work rather than an optimization, but the exact 1,081-line implementation is not sacred, and landing that tip would require authorization, rebase, gate and a fresh current-tip review, the branch's two superseded BLOCK traces granting no approval. The receipt records RESULT: PASS with no findings."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the ruling seat's verdict verbatim. The seat was acting under explicit maintainer delegation and its verdict is the recorded decision. The source artifact contained the ruling body twice (an echoed final turn from the producing tool, with a `tokens used` line and its count between the copies); the two copies were verified byte-identical before one was landed and the echo dropped. RESULT: PASS with FINDINGS: none is the receipt's own finding count — it records that the ruling artifact is sound, not that the reviewed branch is accepted: Q3 explicitly refuses it as a landing candidate. This ruling grants ownership of a question, not execution authority: H2 stays LOCKED, no ledger row moves and no authorization row is created for it."
---

# Architect ruling — ownership, soundness and disposition of the unowned LSP durable-fence branch

**Date:** 2026-08-24
**Status:** RATIFIED — architecture ruling issued under explicit maintainer
delegation; the ruling seat's verdict is the recorded decision.
**Authority:** architecture ruling seat, lane `architecture-lspfence-ownership`,
dispatched against `7dac2b61453dee3c026f611053393f716e1fe52e` under the
delegated ruling authority recorded for this program.
**Supersedes:** none.

## Provenance

- **Reviewed sha:** `7dac2b61453dee3c026f611053393f716e1fe52e` — the tip of
  `block/lsp-durable-fence`, ten commits over merge-base `2d84020bcc6`. It is
  not an ancestor of `program/architecture-lock`; none of it is on trunk.
- **Subject.** The branch closes a torn-pair defect in the LSP durable
  carrier-publication gateway: the direct-open sites published their own
  independently captured IDE/API/pin locals rather than the bytes the gateway
  itself self-verified, and a refusal from the fenced IDE-surface record was
  ignored rather than aborting the publication. It carried no DAG node, no
  ledger row and no `[[authorization]]` when the ruling was taken.
- **Lane:** `architecture-lspfence-ownership`. **Result:** PASS, findings none.
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
  `9721109e0aec3720f65224ccc5da2d7416b96d2ab4e59b9494eb48e449824d6e` for
  each). One copy is landed below verbatim; the echo, the `tokens used` line,
  and its count are dropped.

## What this ruling does not decide

It decides which block owns the question, that the design is sound on its face,
and that the branch is preserved as evidence rather than landed. It accepts no
block, moves no ledger status, and grants no execution authority: H2 remains
`LOCKED` with an unsatisfied predecessor set, and no `[[authorization]]` row is
created for it. It does not ratify the branch's implementation as the fix H2
must adopt — Q3 states the opposite — and it does not review the reviewed tip
against the current trunk. Landing any part of that work still requires a
separate authorization, a rebase, the canonical gate, and a fresh review at the
current tip.

## The ruling

Q1: This belongs to planned block H2. H2 owns provider bindings, carrier lifecycle, and exact readiness receipts; H3 consumes that basis, while K3 only audits residue. No new block is required. The branch remains off trunk as H2 evidence.

Q2: The design is architecturally sound on its face. A project-bound gateway producing one verified `(ide, pin)` pair preserves single authority, and aborting a stale durable publication is the required fail-closed behavior; publishing a narrowed mixed-state result would violate the external-TS contract.

Q3: B. Preserve the branch as evidence and rederive the minimum fix within H2. The defect closure is load-bearing correctness work, not an optimization, but the exact 1,081-line implementation is not sacred. Landing this tip would require authorization, rebase, gate, and fresh current-tip review; the superseded BLOCK traces grant no approval.

## Receipt

```text
===VERTER-RECEIPT-BEGIN===
LANE: architecture-lspfence-ownership
RESULT: PASS
REVIEWED: 7dac2b61453dee3c026f611053393f716e1fe52e
FINDINGS: none
===VERTER-RECEIPT-END===
```
