# Landing-equivalence note: A5 and A6, post-squash

Maintainer-directed housekeeping squash (this session) collapsed the consecutive
`docs(arch)` bookkeeping commits from the work-attribution ledger record through
the last consecutive bookkeeping commit into one commit on top of the work-attribution
substrate commit (`1ab403c01`, unaffected — real product code, excluded from the squash).
This flattened the distinct A5-landing and A6-landing commit boundary in branch history,
so `program/architecture-lock`'s tip (post-squash) no longer has a commit that represents
"exactly post-A5, pre-A6" — both blocks' reviewed candidates became non-ancestor dangling
objects, protected from GC via tags `program-history/A5-reviewed-candidate` (9e053d014) and
`program-history/A6-reviewed-candidate` (aea727465).

## Facts verified directly against the repository (program orchestrator, this session)

```
git rev-parse bb9e9a283^{tree}   -> f709d67232f7dafecee76e92f5cc85202b2ea52c   (post-squash tip)
git rev-parse aea727465^{tree}   -> 7e2977ea31790630a9f93da51aafc8f9bdfc687a   (A6 reviewed candidate)
git rev-parse 9e053d014^{tree}   -> d06647dc7aa0991bdff07b4216d81520e27e2dd3   (A5 reviewed candidate)

git merge-base --is-ancestor 9e053d014 bb9e9a283   -> false (dangling, tag-protected)
git merge-base --is-ancestor aea727465 bb9e9a283   -> false (dangling, tag-protected)

git diff 9e053d014 bb9e9a283 --stat:
  23 files changed, 4118 insertions(+), 40 deletions(-)
  — exactly: A6's own evidence/lock-record/gate files (all newly added, zero
    pre-existing content touched), the D-1 machine-path fix already recorded
    separately in the A4/A5 context-packet.md digests, and maintainer-rulings.md
    updates recording the AMD-001 rescope and ratifications. No file outside
    docs/arch/refactor/rev11/, package.json, vitest.config.ts, and the new
    scripts/validate-performance-gates.* files changed. Zero crates/ changes.
```

## Disposition

A5's reviewed-candidate identity (9e053d014) is retained as `accepted_sha` in
program-state.toml — it was the genuinely reviewed and landed commit at A5's
own acceptance time; a later maintainer-directed cosmetic history squash does
not retroactively change what reviewers inspected or what was accepted then.
The object remains retrievable via the protective tag.

A6's `accepted_sha` is recorded as the live tip (bb9e9a283) since A6, as the
Implementation Lock Record, is definitionally the block that freezes the
cumulative state — its accepted identity legitimately advancing to include the
housekeeping squash's cosmetic history-flattening (verified tree-content-equal
to the pre-squash tip by `git diff bcc5358ce bb9e9a283` = empty, confirmed by
the track orchestrator and independently spot-checked here) is consistent with
governance's allowance for a differing accepted identity after a legitimate
squash, evidenced by this note.
