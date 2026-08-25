# J1 — parse-allocation slice: acceptance evidence

Written so that J1's acceptance, which happens later and by other hands, does not have to
re-derive any of this. Evidence is referenced by **path**; digests in prose are measurements,
not bindings, and go stale as soon as anything moves.

## 1. What is ready

The `verter_css_syntax` allocation predecessor is integrated into `block/j1-integration` and
the branch is rebased onto trunk.

- Candidate `c4d63433ca4daa6ea9857d72bd107ef7b439f2b2`, 46 commits, linear on trunk.
- Rebase clean, 46/46 applied in one uninterrupted run. Both equality proofs pass: the change
  the branch introduces against its old base is byte-identical to the change it introduces
  against the new one, and per-file blob identity holds for every file the branch touches
  except the single file trunk also touched (see §3).
- Ledger delta against the merge-base is empty.
- The cross-block pinned blob is intact (see §3).
- Four independent review lanes PASS. Verdicts, receipts included, are committed under
  [`lanes/`](lanes/): `605c3764a-integration-fidelity.md`, `605c3764a-css-parse-authority.md`,
  `b75140abf-fix-review.md`, `b75140abf-guard-discrimination.md`.
- Allocation canaries 14/14. Measured per-category counts are **better** than the numbers
  recorded on the donor branch — see
  [`debt-J1-css-parse-allocation-ceiling-residual.md`](debt-J1-css-parse-allocation-ceiling-residual.md),
  which carries the composed-tree re-measurement of all eleven categories.

## 2. What is genuinely open

**"Ready" here means ready-not-gate-proven.** No gate has been run on this candidate.

- **Post-rebase build verification has not run.** Nothing has been compiled since the rebase.
  Every test, canary, clippy and fmt result recorded anywhere for this branch was taken
  *before* it, so none of them is evidence about the current candidate. This is the landing
  agent's step and it is not optional.
- **The parse-allocation ceiling is not met.** DEFER-ed, durable owner is the cutover
  reconstruction, resolution gate is after the arena-lifecycle predecessor's evidence exists
  and before cutover acceptance or any legacy deletion. Full terms, including what happens if
  the cutover also cannot reach the bound, are in the debt row linked above. Absent a fresh
  maintainer ruling the default outcome is that this work does not land.
- **Inherited formatting debt is carried, not fixed:** eleven `cargo fmt --check` diffs across
  eight files. Part of it predates every contributing branch. One affected file is the pinned
  blob in §3 and must not be reformatted locally; the rebase resolved it by inheriting trunk's
  version.
- **The 41-acceptance-ID coverage map is not started** and is not owned by any slice. It is
  part of the acceptance package.

## 3. What a successor must NOT re-derive

Each of these was expensive to establish and cheap to get wrong.

### The lanes bind to the shas they reviewed, and remain applicable

Two lanes passed at `605c3764a` and two at `b75140abf`; the candidate is `c4d63433c`. **A lane
binds permanently to the sha it reviewed — that is what a verdict is, not a defect in it.** The
lanes remain *applicable* to the candidate because the intervening delta cannot change the
answer to the question each lane asked: the rebase delta is byte-identical (both patches
872,680 bytes; all 15,504 `+`/`-` content lines identical; whole patches identical once index
lines and hunk numbers are normalised; zero residual).

**This applicability argument was made by the block orchestrator. It is a judgement, not a lane
PASS at the candidate sha.** The attribution is part of the record: strip it and a judgement
silently acquires a verdict's authority.

**This is not the situation where three block mandates were needed on one sha and none existed.**
That was an acceptance-level identity failure. These are slice-level discovery and closure
lanes. Reading this case as that one buys a re-run that tests nothing.

### The cross-block pinned blob — two facts, and both are needed

`crates/verter_bench/examples/route_overhead_baseline.rs` is the pinned `corpus_fingerprint` of
another block's locked performance cell.

1. Its blob **did** change between the pre-rebase tip and the candidate.
2. The branch **did not** change it — zero commits touch it anywhere in the branch range. It
   inherited trunk's version, which is the pinned blob.

Recording only the reassuring half misleads in either direction: "the blob changed" reads as a
violation, "zero commits" hides a fact that needs explaining. Do not reformat this file locally
to clear its formatting diff; that risks producing bytes that differ from the pinned blob.

### The integration-induced regression, and its mechanism

Composing the two contributing branches produced a defect that **git auto-merged with no
conflict**. The selector-region early return in the style-IR sink meant selector-prelude tokens
never reached the retained token vector, and two whole-source facts — comment spans and the
unpaired CDO span — were derived from that vector. Each change was correct on its own branch;
neither branch was red alone.

The consequence was not cosmetic: the unused-rule renderer escapes `*/` inside a wrapped rule by
walking the comment inventory, so a missing prelude comment left its comment-close unescaped and
terminated the wrapping comment early — malformed emitted CSS.

**A clean merge is not a correct merge.** No conflict marker, no failing donor branch, and no
review of either branch in isolation could have surfaced this. It was found by running the
composed tree. The fix derives both facts incrementally as the parser observes tokens; the
position that had no test coverage at all now has it.

## 4. The acceptance obligation

**Scheduled act, not a discovery.** J1's mandates must bind to its **final candidate**, at a
**fresh review by an agent that has not seen it** — `orchestration/review.md` phase 3. Nothing
recorded in this document substitutes for that, and the §3 applicability argument explicitly
does not extend to it: it covers slice-level lanes, not acceptance-level mandates.

The single independent review lane ratified for this block applies once acceptance review
begins. A slice landing into the integration branch is not an acceptance event.
