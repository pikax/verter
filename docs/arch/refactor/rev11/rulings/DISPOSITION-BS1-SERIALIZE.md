---
ruling_id: "BS1-SERIALIZE-BEHIND-BV1"
type: "disposition"
date: "unknown"
date_source: "file-mtime 2026-08-20"
binds: ["BS1", "BV1"]
source_file: "DISPOSITION-BS1-SERIALIZE.md"
summary: "Unprimed codex consult verdict: BS1 serializes behind BV1 — notably not for code-overlap reasons (the Vue/Svelte oracle domains and descriptor/registry tables are already disjoint), but because the live validator fails closed at one IN_PROGRESS block, AMD-005 permits BV1/BS1 overlap only with a reviewed relaxation that does not exist, the A6 stack policy stays depth-one/sequential, and the BF2 golden store has one exclusive-writer combined manifest. Flags blockers 1-2 as governance artifacts, not physics, and escalates relaxing them as a maintainer decision."
supersedes: []
superseded_by: []
contradicts: []
notes: ""
---

# Disposition: BS1 SERIALIZES behind BV1

Source: codex consult, read-only, unprimed, instructed to say SERIALIZE plainly if that was the answer.

## Verdict
**Serialize. BV1 first, then BS1 rebased onto the accepted BV1 tip.**

Notably NOT for the reason expected: the Vue and Svelte oracle locks are separate immutable domains
(`contracts/official-core-oracles.md:12`) and the descriptor/registry tables stay B3/B4- and B2/B5-owned,
so neither train should be editing the other's files. Code overlap is not the blocker.

## The actual blockers, in order of hardness
1. **The live validator fails closed at one `IN_PROGRESS` block**
   (`scripts/validate-program-state.mjs:794-805`), bound to `current_block`. Its own comment states that
   `stacked-prs.md:39` and `max_active_workers = 3` WOULD allow more, and that a parallel regime "must
   relax this check under review, not ad hoc". No such reviewed relaxation exists at this tip.
2. **Performance-gate lease.** AMD-005:98 permits BV1/BS1 overlap only with disjoint code, manifests,
   generated roots AND heavy-machine leases. Both trains add new performance cells
   (`evidence/framework-conformance/performance-impact.md:51`) while the root gate
   (`performance-gates.toml:12`) needs a reviewed extension for later-block cells;
   `performance-impact.md:76` says one shared performance lease forces serialization.
3. **A6 stack policy** stays depth-one/sequential (`stack-window-policy.toml:5`). A BS1 reviewed from
   `051a42ae3` would need a restack once BV1 lands, and :43-46 makes a restack a new candidate that
   invalidates every SHA/tree-bound review verdict.
4. **BF2 golden store** has one combined committed manifest with an exclusive writer
   (`packages/framework-conformance-harness/src/golden-store.mjs`), so any golden-set expansion is a
   serialized write surface regardless.

## Consequence worth escalating
Blockers 1 and 2 are GOVERNANCE artifacts, not physics. The program's own ratified documents contemplate
up to three active workers; the validator and the performance-gate lease are what pin it to one. The
remaining DAG is therefore strictly serial, which is the direct cause of the throughput the maintainer
has already flagged as slower than expected. Relaxing it is a maintainer decision and a reviewed change
to the validator + gate lease, not something an orchestrator may do ad hoc. Proposal prepared separately.

## Action
- BV1 IN_PROGRESS, `current_block = "BV1"`.
- BS1 stays READY, unopened, until BV1 is ACCEPTED and fast-forwarded.
