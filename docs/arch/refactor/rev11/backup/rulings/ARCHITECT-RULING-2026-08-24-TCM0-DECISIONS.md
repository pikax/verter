---
ruling_id: "TCM0-DECISIONS-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["TCM0", "TCM2", "TCM3", "TCM4"]
source_file: "ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md"
summary: "Eight-question architecture ruling taken on 6e1777b4c, TCM0's round-3 candidate. Q1 rules A: the source-backed inventories, probes and transcript land as a NON-ACCEPTANCE evidence package, excluding the program-state.toml hunk, the ADR-021 changes, and every summary/gap/disposition passage still claiming the superseded Q2-Q7 state; the incomplete contract remainder becomes a SUCCESSOR BLOCK with fresh verification, not a round 4. Q2 ratifies the topology transfer: TCM0 owns candidate screening, survivor sets, metrics, harness, baseline method and selection rule, while TCM2 and TCM3 own evidence-based projection- and semantic-topology selection as blocking exits. Q3 rules A: requirements 6-8 are the complete TCM0 Scope-10 performance contract and no dedicated-machine absolute baseline is required. Q4 retains feature-ownership-ledger rows 25 and 26 under VerterWithTypeSemanticOracle; TCM4 may remove the tsserver-specific methods only after TCM3 supplies and tests equivalent semantics. Q5 rules A: get_diagnostics_background, its forwarding implementations and row 31 are deleted. Q6 rules that TCM3 already owns diagnostic-mapper convergence and no new block is authorized; until TCM3 lands, severity taxonomy, canonical positioning and unpositionable-diagnostic behaviour remain divergent across the CLI and oracle paths. Q7 rules the transcript-staleness gap acceptable, the transcript being immutable evidence for its exact pinned package, with TCM4 owning future-package verification at the certified-engine gate. Q8 rules A: the candidate's program-state.toml hunk is reverted and the program orchestrator records the corrected notes directly on trunk. The receipt records RESULT: PASS with no findings."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the ruling seat's verdict verbatim. The seat was acting under explicit maintainer delegation and its verdict is the recorded decision. The source artifact contained the ruling body twice (an echoed final turn from the producing tool, with a `tokens used` line and its count between the copies); the two copies were verified byte-identical before one was landed and the echo dropped. RESULT: PASS with FINDINGS: none is the receipt's own finding count — it records that the ruling artifact is sound, not that the reviewed candidate passed: Q1 returns that candidate as wrongly scoped."
---

# Architect ruling — eight questions on TCM0's round-3 candidate: scoping, topology transfer, performance contract, ledger rows 25-26, a dead diagnostics API, mapper convergence, transcript staleness, and the ledger hunk

**Date:** 2026-08-24
**Status:** RATIFIED — architecture ruling issued under explicit maintainer
delegation; the ruling seat's verdict is the recorded decision.
**Authority:** architecture ruling seat, lane `architecture-tcm0-decisions`,
dispatched against `6e1777b4c60cc163c7c24a77e6ef82bad9ac6190` under the
delegated ruling authority recorded for this program.
**Supersedes:** none.

## Provenance

- **Reviewed sha:** `6e1777b4c60cc163c7c24a77e6ef82bad9ac6190` — TCM0's round-3
  candidate, the tip of `block/tcm0-contract-lock`. It is not an ancestor of
  `program/architecture-lock`; nothing from it is on trunk at the time of this
  transcription.
- **Lane:** `architecture-tcm0-decisions`. **Result:** PASS, findings none.
- **Result artifact:** `~/.claude/briefs/rev11/verify/results/RULING/6e1777b4c60cc163c7c24a77e6ef82bad9ac6190/architecture-tcm0-decisions.out`.
- **Receipt validation.** Re-run at transcription time:

```text
$ node scripts/orchestration/check-results.mjs \
    ~/.claude/briefs/rev11/verify/results/RULING/6e1777b4c60cc163c7c24a77e6ef82bad9ac6190 \
    6e1777b4c60cc163c7c24a77e6ef82bad9ac6190 architecture-tcm0-decisions
OK      architecture-tcm0-decisions PASS  blockers=0 carried=0  4419B
          ! the result appears 2 times, identical — an echoed final turn
ALL SOUND — 1 result(s) on 6e1777b4c60c blockers=0 carried=0
```

- **Delegation.** The ruling seat was acting under explicit maintainer
  delegation. Its verdict is the recorded decision; this document transcribes
  it and does not re-decide any part of it.
- **Duplicate-copy handling.** The source artifact carried the ruling body
  twice — an echoed final turn from the producing tool, separated by a
  `tokens used` line and its count, which the result checker flags as "the
  result appears 2 times, identical". The two copies were compared and are
  byte-identical (sha256
  `e672d08a460e4a1c7860b6a3d77fff4a79a47e93b3a9cea64f8d63555351c8c0` for each).
  One copy is landed below verbatim; the echo, the `tokens used` line, and its
  count are dropped.

## The ruling

Q1: A. Land the source-backed inventories, probes, and transcript as a non-acceptance evidence package. Exclude the `program-state.toml` hunk, the `ADR-021` changes, and every summary/gap/disposition passage that still claims the superseded Q2–Q7 state; rewrite those from these rulings. The incomplete contract remainder becomes a successor block with fresh verification, not round 4.

Q2: Ratified. TCM0 owns candidate screening, survivor sets, metrics, harness, baseline method, and selection rule; TCM2 and TCM3 respectively own evidence-based projection- and semantic-topology selection as blocking exits.

Q3: A. Requirements 6–8 are the complete TCM0 Scope-10 performance contract; no dedicated-machine absolute baseline is required. Independently owned correctness and lifecycle gates remain applicable.

Q4: Retain both rows under `VerterWithTypeSemanticOracle`. Row 25 preserves local content/position conversion and carrier-to-project routing; row 26 preserves oracle working-set activation. TCM4 may remove the tsserver-specific methods only after TCM3 supplies and tests equivalent semantics.

Q5: A. Delete `get_diagnostics_background`, its forwarding implementations, and row 31. Dead API surface has no capability owner and must not be labelled `DisabledByExplicitApprovedContract`.

Q6: TCM3 already owns convergence through its `TypeSemanticOracle` and `VerterWithTypeSemanticOracle` diagnostic contract; no new block is authorized. Until TCM3 lands, severity taxonomy, canonical positioning, and unpositionable-diagnostic behaviour remain divergent across the CLI and oracle paths.

Q7: The gap is acceptable. The transcript is immutable evidence for its exact pinned package, so a future package cannot make it stale. TCM4 owns future-package verification at the certified-engine gate: its mapper-conformance and semantic-capability probes must pass before activation.

Q8: A. Revert the hunk. The program orchestrator, as sole ledger writer, records the corrected notes directly on trunk from these rulings.

## Receipt

```text
===VERTER-RECEIPT-BEGIN===
LANE: architecture-tcm0-decisions
RESULT: PASS
REVIEWED: 6e1777b4c60cc163c7c24a77e6ef82bad9ac6190
FINDINGS: none
===VERTER-RECEIPT-END===
```
