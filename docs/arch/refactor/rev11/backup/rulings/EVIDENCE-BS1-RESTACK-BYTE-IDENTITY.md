---
ruling_id: "BS1-RESTACK-BYTE-IDENTITY"
type: "attestation"
date: "unknown"
date_source: "file-mtime 2026-08-20 (no in-document date)"
binds: ["BS1"]
source_file: "EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md"
summary: "Discharges BS1-COMPLETION-CORRECTION item 6: proves the seven-fix Svelte code diff is byte-identical (68,598 bytes both) before and after restacking BS1 from base f46de1b6a onto the landed gate-correction commit 9275f0e40 (restacked candidate 761651109). Confirms the gate correction landed independently reviewed (first review BLOCKING on an unmitigated race, root-caused and fixed, delta review reproduced the race with the fix reverted and confirmed clean with it restored). States conformance/architecture verdicts carry across the restack by this proof; the adversarial verdict does NOT (bound to the pre-restack SHA only) — a fresh adversarial pass is owed."
supersedes: []
superseded_by: []
contradicts: []
notes: "Lists BS1's remaining outstanding items as of this document: corrected completion contract awaiting maintainer ratification; two stale conformance records needing regeneration from the pinned oracle; 14 UNPROVEN rows; FC-HYDRATION-001/FC-PERF-001 BLOCKED/UNPROVEN."
---

# BS1 §6 discharge — restack onto the landed gate correction, byte-identity proven

Discharges item 6 of MAINTAINER-RULING-BS1-COMPLETION-CORRECTION: *"Land and review that gate correction
independently, then rebase BS1 and mechanically prove the seven-fix code diff remains byte-identical."*

## Prerequisite: the gate correction is landed and independently reviewed

`9275f0e40` (trunk) carries the full correction chain: the offline Svelte-oracle prerequisite probe that
fails setup loudly instead of injecting a comparison note; `verter_session/bf2-authoritative` enabled in
the canonical archive so the 45 previously-hidden conformance tests execute; Surface 3 extended to
`verter_svelte_conformance`, `verter_vue_conformance` and `verter_compiler`; the per-process scratch fix
that removed a real cross-process race; and the interim trybuild exclusion.

It was reviewed INDEPENDENTLY before landing. The first review returned BLOCKING on an unmitigated
Surface 2 race; the fix round root-caused it (a shared, specifier-keyed scratch directory whose cleanup
deleted the shared parent out from under siblings) rather than serialising around it; the delta review
then reproduced the original race 2-in-3 with the fix reverted and confirmed 4/4 clean with it restored.

## The restack

| | |
|---|---|
| pinned reviewed candidate (pre-restack) | `9786e756b` — tag `protect/bs1-reviewed-candidate` |
| pre-restack base | `f46de1b6a` |
| new base | `9275f0e40` |
| restacked candidate | `761651109` — tag `protect/bs1-restacked-candidate` |
| restacked tip (evidence commit) | `966eedf9b` |

Rebase was CLEAN — no conflicts.

## Byte-identity proof

    git diff <base>..<tip> -- crates packages

| | bytes |
|---|---|
| pre-restack (`f46de1b6a..a48d92e82`) | **68,598** |
| post-restack (`9275f0e40..966eedf9b`) | **68,598** |
| `cmp` byte-identical | **YES** |

The seven-fix code delta is unchanged by the restack. The three CI/validator commits it now sits on top
of touch `scripts/` and the ledger only — no overlap with the Svelte compiler surface.

## What this does and does NOT carry

Per the correction ruling item 5: conformance and architecture verdicts may carry ONLY where
byte-equivalence proves their reviewed subject matter unchanged. That is now proven for the RESTACK, so
those two verdicts carry across it.

**The adversarial verdict does NOT carry by this proof.** It was bound to `9786e756b` and re-attested on
that exact candidate after the fix rounds. The candidate SHA has changed to `761651109`, and the
review-identity binding landed at `71fb82dec` will mechanically refuse a `PASS` whose reviewed SHA is not
the current `candidate_sha`. Byte-identity of the code delta is a strong argument that the adversarial
subject matter is unchanged — but the ruling requires the adversarial seat to attest THE EXACT FINAL
CANDIDATE, and the final candidate is not settled until BS1's remaining completion-contract work lands.
A fresh adversarial pass is owed at that point, not now.

## Still outstanding for BS1

The corrected completion contract awaits maintainer ratification; §3's two stale conformance records must
be regenerated FROM THE PINNED ORACLE (never from candidate output) and independently verified; the 14
UNPROVEN rows remain BS1 work; and FC-HYDRATION-001 / FC-PERF-001 stay BLOCKED/UNPROVEN.
