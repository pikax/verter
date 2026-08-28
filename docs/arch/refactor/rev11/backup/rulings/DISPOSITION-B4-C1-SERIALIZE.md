---
ruling_id: "B4-C1-SERIALIZE"
type: "disposition"
date: "unknown"
date_source: "file-mtime 2026-08-19"
binds: ["B4", "C1"]
source_file: "DISPOSITION-B4-C1-SERIALIZE.md"
summary: "Unprimed codex concurrency consult verdict: B4 and C1 serialize (B4 first, then C1 rebased onto B4's accepted tip) — virtual_file_pipeline.rs is co-owned (C1's macro/type projection at ~:3000, B4's final map/publication assembly at ~:3281, both touch compile_entry ~:3071-3103), and the ratified stack-window policy fixes depth at 1 with sequential landing regardless. C1 is not blocked on B3 semantically; this is a tree-ownership constraint, not a dependency one."
supersedes: []
superseded_by: []
contradicts: []
notes: ""
---

# Disposition: B4 and C1 SERIALIZE (do not run concurrently)

Source: codex concurrency consult, unprimed, instructed to say "serialize" if that was the honest answer.

## Verdict
**Serialize. B4 first, then C1 rebased onto the accepted B4 tip.**

## Grounds
1. **Production collision, not just a rebase risk.** `virtual_file_pipeline.rs` is co-owned:
   C1's macro/type projection is invoked at ~:3000; B4 owns final map/publication assembly at ~:3281.
   B4 must rewrite `compile_entry` (~:3071-3103), which is also where B3's request construction now lives.
   The underlying conflict — B4's source-unit deletion vs C1's resolver/index/fact convergence —
   predates B3 and is not an artifact of it.
2. **Ratified stack policy forbids it anyway.** `evidence/A6/stack-window-policy.toml:5` fixes stack
   depth at 1 with sequential landing; :43 states a restack creates a new candidate and INVALIDATES
   review. Two siblings cut from one tip cannot both fast-forward; the second would need a restack and
   a full re-review even with zero textual overlap.
3. C1 is not blocked on B3 semantically (C1 has no B3 predecessor; C2 is the later join,
   `program.md:199-203`). The serialization is a tree-ownership constraint, not a dependency one.

## Action
- B4 dispatched alone on `block/b4` from `664cab091`.
- C1 stays READY, unopened, until B4 is ACCEPTED and fast-forwarded; C1 then cuts from that tip.
