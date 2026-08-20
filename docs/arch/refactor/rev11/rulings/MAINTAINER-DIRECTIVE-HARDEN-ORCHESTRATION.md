---
ruling_id: "HARDEN-ORCHESTRATION"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["program-wide orchestration machinery"]
source_file: "MAINTAINER-DIRECTIVE-HARDEN-ORCHESTRATION.md"
summary: "RATIFIED, gates further block progress beyond BS1. Directs four workstreams before advancing: machine-enforced block authorization/ratification; one generated effective-state view from DAG+amendments/rulings+ledger with contradictions failing loudly; mutation tests for validator/oracle failure modes; internal review checkpoints for large blocks with final acceptance staying atomic. Tabulates seven same-day defects the orchestration machinery falsely certified as evidence for the diagnosis."
supersedes: []
superseded_by: []
contradicts: []
notes: "This is the directive ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md answers, and RULING 2 of that answer is what this migration task itself executes."
---

# Maintainer directive — harden the orchestration layer before advancing beyond BS1

**Date: 2026-08-20. RATIFIED. Gates further block progress.**

> Before advancing beyond BS1, harden the orchestration layer: make block authorization/ratification
> machine-enforced, generate a single effective-state view from DAG + amendments/rulings + ledger, add
> mutation tests for validator/oracle failure modes, and break large blocks like BS1 into internal review
> checkpoints while keeping final acceptance atomic. The architecture itself looks sound; the main
> remaining risk is the orchestration machinery incorrectly certifying an invalid transition.

## Why this is the right diagnosis — the evidence from one day

Every serious defect found today was the orchestration machinery certifying something invalid, not the
architecture being wrong:

| defect | how long it held | what certified it falsely |
|---|---|---|
| `evidence_root = "<EVIDENCE>"` placeholder | weeks | no evidence digest was ever verified; 5 accepted blocks bound to artifacts that never existed |
| identities regex-checked only | weeks | A5 accepted on a DANGLING commit; A6/BF2 base SHAs not ancestors of their own accepted SHA |
| review verdicts unbound to candidates | since inception | a `PASS` could survive any number of fix rounds; B3 nearly closed on an implicit pass |
| 45 conformance tests behind a non-default feature | weeks | Svelte blocks reported "canonical gate clean" while nothing covering their subject matter ran |
| AMD-005 unratified | until caught by hand | BV1 was dispatched and landed with no authority; nothing checked |
| C1 charter vs D1 Fork 2 contradiction | until caught by hand | two ratified rulings disagreed about which files move; nothing cross-checks rulings |
| BS1's completion contract | until escalated | the block's own "Required exits" rested on the unratified AMD-005 |

Each was found by a human-directed audit, not by the machinery. That is the risk the directive names.

## The four workstreams

1. **Machine-enforced authorization/ratification.** A block must not leave `LOCKED` without a
   machine-checkable authority chain: ratified charter, ratified enabling amendment (landed at
   `5b899200b`), and any binding rulings. Today charter/amendment status is PROSE in a `**Status:**` line
   and rulings live OUTSIDE the repository in a scratchpad — so the authority chain is not even fully
   present in the tree, let alone checkable.
2. **One generated effective-state view** from DAG + amendments/rulings + ledger, with contradictions
   failing loudly. The C1/D1 flow-file conflict — same file, opposite dispositions, in two binding
   documents — is the case this must catch.
3. **Mutation tests for validator and oracle failure modes.** Today discrimination is proven ad hoc, one
   planted mutation at a time, by whoever remembers. Systematise it: for every check, a mutation that
   SHOULD trip it, asserted to trip it.
4. **Internal review checkpoints for large blocks; final acceptance stays ATOMIC.** BS1 is the case
   study: fix rounds invalidated an adversarial verdict, the completion contract surfaced only under
   escalation, and 14 UNPROVEN rows were discovered at the end rather than progressively.

## Standing constraints

Landed enforcement is structural or tool-based, never a name-keyed source scanner. Zero-selection is a
failed verification, never a pass. A mechanism that cannot demonstrate it discriminates is not
enforcement. Nothing may compile Rust while `gate.mjs` runs.
