# A0 Branch Screen — Open Architecture-Affecting Changes (`contracts/baseline-lock.md` §3)

**Scope:** local branches of the `pikax/verter` checkout, screened against the A0 entry
lock `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` (committer date `2026-08-09 18:09:15 +0100`,
unix `1786295355`).

## Counts

- Local branches at screen time: **597** (596 at the original A0 capture, plus the
  program's own worktree branch `docs/rev11-architecture-plan`).
- Unmerged into the base commit (`git branch --no-merged 9af553dd2…`): **572**
  (571 pre-existing + the program's own branch).

## Screening method

The 571 pre-existing unmerged branches were **not individually screened** — no
per-branch content review was performed. Instead a mechanical tip-date screen was run:

```sh
git for-each-ref refs/heads --format='%(committerdate:unix) %(refname:short)' \
  | awk -v b=1786295355 '$1 > b {print $2}'
```

i.e. list every local branch whose TIP commit is newer than the base commit's
committer date, then inspect those tips for `crates/` or `packages/` paths.

## Result

Exactly **one** branch tip is newer than the base commit's date:

| branch | tip vs base | touches `crates/` or `packages/` | disposition |
| --- | --- | --- | --- |
| `docs/rev11-architecture-plan` | newer (the program's own A0 landing branch) | no — docs/arch/refactor/rev11/, scripts/, package.json (root), vitest.config.ts, .github/workflows/ci.yml only | the A0 candidate itself; not a competing change |

Every one of the 571 pre-existing unmerged branches has a tip commit dated **at or
before** the base commit's date (newest: `feat/scanners-css-formatter`, tip dated
2026-08-05). None is newer than the base commit's date, so per the screen's filter
none required a per-branch `crates/`/`packages/` inspection.

## Honest limitation

Tip date is a screen, not a review: a stale-dated branch could still contain an
architecture-affecting change that someone resumes later. The honest answer for the
571 pre-existing branches is: **not individually screened**; they are stale relative
to the entry lock, and any future resumption of one of them must be dispositioned
under `contracts/baseline-lock.md` §3 before the A6 implementation-baseline freeze.
The only branch-shaped change that WAS individually dispositioned at A0 is PR #98
(`agent/rsvelte-runtime-engine`) — dispositioned ABANDON by maintainer ruling R-5.
