---
ruling_id: "BS1-COMPLETION-AUTHORITY"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["BS1"]
source_file: "MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY.md"
summary: "Issued after an escalation that BS1's charter Required-exits rest on unratified AMD-005. BS1 remains IN_PROGRESS; its seven completed Svelte corrections are authorized/retained but do not alone establish completion. The earlier BV1/BS1 authorisation ruling did not ratify AMD-005's acceptance criteria. Requires a standalone, exact-byte, self-contained, digest-bound BS1 completion packet (executable FC-* definitions, exact BF3-guard removals, a row-by-row evidence matrix). Conformance/architecture verdicts may carry only where byte-equivalence proves unchanged subject matter; a fresh adversarial seat must independently attest the exact final candidate."
supersedes: []
superseded_by: []
contradicts: []
notes: "Extended (not superseded — 'Supersedes nothing') by MAINTAINER-RULING-BS1-COMPLETION-CORRECTION.md. This document's own 'Orchestrator execution state' section records the §5-first-half discharge (byte-identity across the f46de1b6a rebase) later re-verified against a different base in EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md."
---

# Maintainer ruling — BS1 completion authority

**Date:** 2026-08-20. Issued in response to the orchestrator's escalation that BS1's charter "Required
exits" rest on unratified AMD-005.

1. **BS1 remains `IN_PROGRESS`.** Its seven completed Svelte corrections are AUTHORIZED, RETAINED, and
   must not be reverted — but they do not alone establish block completion.
2. The earlier narrow ruling authorized BV1/BS1 **execution**; it did NOT ratify AMD-005 nor implicitly
   adopt AMD-005's acceptance criteria.
3. The orchestrator must prepare a **standalone, exact-byte BS1 completion packet** containing:
   - the exact `svelte@5.56.8` capability cells BS1 must close;
   - **executable** definitions for FC-SVELTE-001, FC-HYDRATION-001, FC-TS-001, FC-ATOMIC-001,
     FC-ZERO-WORK-001, and the applicable FC-PERF-001 cells;
   - the exact corresponding **BF3 guards eligible for removal**;
   - a **row-by-row evidence matrix** against the final candidate.
4. The packet MAY extract existing AMD-005 text, evidence and tests without redoing that work, but it
   must be **self-contained and digest-bound**. Ratifying it authorizes only **BS1's completion
   contract** — not AMD-005, its other blocks, product claims, contracts, or amendments.
5. Existing **conformance and architecture** verdicts may carry ONLY where byte-equivalence proves their
   reviewed subject matter unchanged. The newly dispatched **adversarial** seat must independently attest
   the exact final candidate and bind its verdict to that candidate SHA.
6. After the completion packet is ratified: any **unproven** exit remains BS1 work; **proven** exits need
   not be repeated. BS1 returns for acceptance only when every packet row AND all three exact-candidate
   mandates pass. **B5 remains LOCKED meanwhile.**

## Orchestrator execution state

- Ledger already reflects §1 and §6: BS1 `IN_PROGRESS`, B5 `LOCKED`. No ledger edit required.
- §5 first half is DISCHARGED for the rebase onto `f46de1b6a`: the candidate's own diff over
  `crates/` + `packages/` is byte-identical pre- and post-rebase (68,598 bytes both), so the conformance
  and architecture seats' reviewed subject matter is provably unchanged by the three CI/validator commits
  that landed underneath. That proves the REBASE carried them; it does NOT cover the fix-round rewrite.
- §5 second half is IN FLIGHT: a bounded adversarial re-attestation against the exact final candidate
  `9786e756b`, dispatched unprimed and told not to treat the earlier pass as evidence.
- §3/§4 packet authoring dispatched separately.

Source material located for the packet author:
`evidence/framework-conformance/capability-matrix.tsv`, `contracts/capability-matrix.md`,
`evidence/BF3/{context-packet,dispositions,disposition-ruling}.md`,
`evidence/B3/debt-FC-SVELTE-001-svelte-output-liveness.md`, `evidence/BV1/{context-packet,landing-record}.md`,
and the unratified `amendments/AMD-005-framework-compiler-conformance-rescope.md` (extractable, but the
packet must stand alone).
