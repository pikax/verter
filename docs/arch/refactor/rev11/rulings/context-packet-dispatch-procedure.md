---
ruling_id: "CONTEXT-PACKET-DISPATCH-PROCEDURE"
type: "procedure"
date: "2026-08-22"
date_source: "stated"
binds: []
source_file: "context-packet-dispatch-procedure.md"
summary: "Mechanical procedure closing the context_packet_digest dispatch-timing gap MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md grandfathers for BV2/B5/CM1: the context packet must be written and its digest recorded before a block leaves LOCKED, including for directive-dispatched blocks, so the gap this ruling grandfathers cannot recur."
supersedes: []
superseded_by: []
contradicts: []
notes: "Not itself a ruling — a short procedure with no block bindings, filed alongside MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md for discoverability. This frontmatter was added after the fact (2026-08-23) so this file parses uniformly with the rest of the rulings/ corpus scripts/effective-state.mjs discovers; it does not change the procedure's own content."
---

# Context-packet dispatch procedure

Filed alongside MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md. Not a
ruling — a short, mechanical procedure that closes the gap the ruling
grandfathers for BV2/B5/CM1: `contracts/agent-orchestration.md` §6 already
requires a context packet for every non-trivial worker and says it is
"addressed by digest and stored with evidence," but never states *when*
relative to a ledger transition, so a directive-dispatched block (as all
three grandfathered rows were) could skip it entirely with nothing catching
the omission until `REVIEW`. This procedure states the timing explicitly so
that is the failure mode this closes.

## Procedure

1. **Before a block's status moves to `IN_PROGRESS`**, the dispatching
   orchestrator fills `templates/context-packet.md` for that block and saves
   it as `evidence/<BLOCK_ID>/context-packet.md` in the live evidence root
   (the same convention already used by A0, A4, A5, A6, B2, B3, B4, BA0,
   BF1, BF2, BF3, BRT0, BS0, BS1, BV0, BV0A, BV1 — see those directories for
   worked examples).
2. **Compute its SHA-256 immediately** (`sha256sum evidence/<BLOCK_ID>/
   context-packet.md`) and record it in that same change as the block's
   `context_packet_digest` in `program-state.toml`, alongside the
   `base_sha`/`charter_digest` fields the block already needs to enter
   `IN_PROGRESS`. Do not defer this to the block's `REVIEW` transition —
   that is exactly how the gap this ruling grandfathers went unnoticed until
   too late to fix honestly.
3. **A directive-dispatched block is not exempt.** If the maintainer directs
   work directly (bypassing the normal charter-first flow, as the beta.4
   regression-intake directive did for BV2/B5/CM1), the orchestrator still
   writes a context packet from the directive's own content before the
   block leaves `LOCKED` — the packet's job is to bound what the worker may
   touch and record what it was told, not to prove the work was solicited
   through a particular ceremony. Skipping it is what created the gap this
   ruling now has to grandfather instead of enforce.
4. **The validator is the backstop, not the procedure.** `scripts/
   validate-program-state.mjs` already fails closed on a missing/malformed
   `context_packet_digest` for any block reaching `REVIEW` or later, except
   the three names explicitly grandfathered in
   MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md §1. Following steps 1-3
   means that check never has anything to catch; it stays in force
   regardless.
