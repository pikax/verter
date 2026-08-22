# Landing-equivalence note: CM1

CM1 did not land as a squash of the recorded `implementation_candidate_sha`
(`47e85159063b0ea841548f0d29aa0eb1d22c7fad`, `block/cm1`'s tip). That pin is
stale and does not describe what is actually on trunk for this block. This note
establishes what CM1's real accepted identity is, and why the ledger's prior
pin was wrong rather than merely outdated.

## Accepted identity

`base_sha` = `eadec2dc0ebc5f9d9ffb9c325b71ecd6a255bc7d` (trunk tip immediately
before CM1's first landed commit — the real integration point, not the
branch's original 2026-08-21 dispatch base).

`candidate_sha` = `candidate_tree` = `accepted_sha` = `accepted_tree` identity
is carried by `13eafb2ab218cbdb762dda02e1f383b5bd6ec040` (tree
`9f848e6cbb90f22df3ed9bac023b983163275302`) — the second and final of CM1's two
real trunk commits. There is no candidate/accepted divergence to prove: nothing
landed after `13eafb2ab` that changed CM1's own files.

```
$ git log -1 --format='%H %P' eadec2dc0
eadec2dc0ebc5f9d9ffb9c325b71ecd6a255bc7d parents=<trunk tip before CM1>
$ git merge-base --is-ancestor eadec2dc0 0e5177931 && echo yes
yes   # eadec2dc0 is 0e5177931's direct parent
$ git merge-base --is-ancestor 0e5177931 13eafb2ab && echo yes
yes   # real, linear, unrewritten trunk ancestry
$ git merge-base --is-ancestor 13eafb2ab HEAD && echo yes
yes   # genuinely landed, reachable from the live trunk tip
```

Both commits are real, linear, unmodified trunk commits — not a rebase, not a
rewrite, not a dangling object. This is the same identity guarantee BV2's note
established: the reviewed bytes are addressable at their own original SHAs.

## Why `implementation_candidate_sha` (47e8515) was wrong, not merely stale

`47e85159063b0ea841548f0d29aa0eb1d22c7fad` is `block/cm1`'s branch tip.
Two independent facts disqualify it as CM1's landed identity:

```
$ git merge-base --is-ancestor 47e8515 0e5177931 && echo yes || echo no
no
$ git merge-base --is-ancestor 13eafb2ab block/cm1 && echo yes || echo no
no
$ git merge-base --is-ancestor block/cm1 13eafb2ab && echo yes || echo no
no
```

**Asymmetric review-identity precision — disclosed, not smoothed over.** The
two slices do not have equally strong tree-identity proofs:

```
$ git log -1 --format='%H %T' 47287d9dd   # tip of the RootBindingIndex lineage
47287d9dd86b9ef692ae306b01b884c6d3da46ba 9f848e6cbb90f22df3ed9bac023b983163275302
$ git log -1 --format='%H %T' 13eafb2ab
13eafb2ab218cbdb762dda02e1f383b5bd6ec040 9f848e6cbb90f22df3ed9bac023b983163275302
```

Finding C's tree is IDENTICAL between the last commit of its own reviewed
lineage (`47287d9dd` — after `2f0379039`'s adversarial-review bug fixes and
`47287d9dd`'s own coverage-gap closure) and `13eafb2ab` as landed: the same
byte-identity guarantee B5's candidate/accepted split rests on.

Finding B has no equivalent proof:

```
$ git log -1 --format='%H %T' 0e5177931
0e517793169329113415009578ecec86e1ce9b5a 9f7da120dc44419e17070b8622f1b0073894cf0c
$ git log -1 --format='%H %T' 47e8515   # block/cm1 tip
47e85159063b0ea841548f0d29aa0eb1d22c7fad 9d17fe54f02f26009a26a75f5791d87dc6f50757
$ git log -1 --format='%H %T' a7bf8c696   # block/cm1's post-revert state
a7bf8c696b132269f27bf9d24c8a72ff0744d997 ea31045accbe575050f5f1b1f436ce7b257ac7a5
```

None of these three trees match `0e5177931`'s. Trunk moved far enough
between `53d6c3157` (dispatch) and `eadec2dc0` (actual landing parent) that a
squash onto the later base does not reproduce the branch's own tree, unlike
Finding C's cleaner, later-dispatched lineage. Finding B's review evidence is
therefore narrative and commit-history-based (successive review-driven fix
commits on `block/cm1`, correctly identifying and removing the unsound
shadow scanner) rather than a hash proof. `conformance_review` /
`architecture_review` / `adversarial_review` on the ledger row bind
`reviewed_sha = 13eafb2ab` for BOTH findings, as the schema requires a single
candidate identity per block — that binding is exact for Finding C and is
the closest available honest anchor for Finding B, not a claim that
`13eafb2ab` itself was the literal object any reviewer looked at for the
expose-binding/admission-gate work. This asymmetry is recorded here rather
than smoothed into a single undifferentiated "reviewed" claim.

1. **`0e5177931` is a divergent squash of `block/cm1`, not an ancestor
   relationship.** `block/cm1`'s own history shows an internal revert
   (`a7bf8c696 revert(core): drop runtime-constructor shadow detection, keep
   expose-binding fixes`) landing before the branch's own tip. `0e5177931`'s
   commit message states the same outcome independently ("Runtime-constructor
   shadow detection was removed rather than repaired... `defineProps` and
   Options-API runtime-constructor behaviour are therefore byte-identical to
   before this change"). The trunk commit is content-consistent with
   `block/cm1`'s own corrected end state for the expose-binding/admission
   scope, but git has no ancestry between them — it is a squash onto a moving
   trunk, not a fast-forward or rebase, so no single SHA on `block/cm1`
   equals what landed.

2. **`13eafb2ab` (the runtime-constructor / `RootBindingIndex` repair) is not
   on `block/cm1` at all.** It is the "own change" `0e5177931`'s commit
   message explicitly deferred ("a real owner-aware binding index is the
   correct replacement and belongs in its own change"). It was built on a
   wholly separate lineage (`bf61e676b` "build the owner-aware root
   value-binding index" through `47287d9dd` "close the two residual coverage
   gaps", including a 3-round codex xhigh adversarial design review — see
   `evidence/CM1/binding-index-design*.md` — and a post-implementation
   adversarial pass that found and fixed three real bugs, `2f0379039`),
   squashed directly onto trunk as `13eafb2ab` with parent `120eede71` (a J1
   commit). `block/cm1`'s tip is neither an ancestor nor a descendant of
   `13eafb2ab`.

So CM1's real delivery is two disjoint slices landing on two different days
from two different lineages, neither of which is `block/cm1`'s recorded tip.
Repinning `implementation_candidate_sha` a second time to another branch tip
(as the prior 2026-08-22 landing record did, from a dangling rebase target to
`47e8515`) would repeat the same category of error: the true identity is the
two real trunk commits, not any WIP branch's tip.

## Why the fixed-landing-order rehearsal actually conflicts

The recorded violation cites `contracts/stacked-prs.md` /
`MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md` as the rehearsal's own
governing authority — that citation is the validator's static explanation of
*why the check exists*, not the path of the file that actually conflicts.
Reproducing the exact rehearsal step by hand:

```
$ git merge-tree --write-tree --merge-base=53d6c315701e81647fad77a6970f6f2c7c218aaf \
    c02b23093bfe28d7ebe46ef24d1fa0d38b6f63c9 47e85159063b0ea841548f0d29aa0eb1d22c7fad
CONFLICT (content): Merge conflict in
  docs/arch/architecture-lock/ledger/authority-registry.toml
```

The real conflict is in `authority-registry.toml`, and it is a pure adjacency
collision, not a semantic one:

```
$ git diff --stat 53d6c3157..47e8515 -- .../authority-registry.toml
 1 file changed, 71 insertions(+)
$ git diff --stat 53d6c3157..c02b23093 -- .../authority-registry.toml
 1 file changed, 36 insertions(+)
```

`53d6c3157` (`block/cm1`'s original 2026-08-21 dispatch base) predates the
commit that appended CM1's own dispatch-authorization rows to
`authority-registry.toml` on trunk. `block/cm1` independently carries that
identical 71-line addition (its own environment already had it via a rebase
or branch-create snapshot), appended at end-of-file. The cumulative side
(replaying BV2's and B5's own ledger acceptance edits, 36 lines) appends its
own, different rows at the same end-of-file location. Two independent
appends at the same point, diffed against a base neither addition shares
context with, is exactly the shape `git merge-tree` cannot auto-resolve — it
is not evidence of any real disagreement about CM1's content, only of a stale
`base_sha` too far behind a fast-moving governance file to replay cleanly.

Once CM1 is `ACCEPTED`, this is moot on two independent grounds: (a) the
rehearsal only runs over `IN_PROGRESS ∪ REVIEW ∪ ACCEPTANCE_RECOMMENDED`
blocks, so an `ACCEPTED` CM1 drops out of the active set entirely; (b) even
rehearsed, the corrected `base_sha`/`candidate_sha` pair above
(`eadec2dc0..13eafb2ab`) is a real, linear, already-merged trunk delta with
nothing left to replay.

## Disposition

CM1's real, accepted identity is the pair of trunk commits
`0e5177931`/`13eafb2ab`, base `eadec2dc0`, with no post-landing fix to
account for (no candidate/accepted divergence). `implementation_candidate_sha`
is retained on the ledger row as the historical WIP-dispatch pin it always
was (per the field's documented meaning once a block leaves `IN_PROGRESS`)
and is not authoritative for what landed.
