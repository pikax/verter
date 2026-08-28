# C1 landing equivalence

Status: **PROVEN**.

This is the post-landing proof required by the Revision 11 landing mechanism. It
binds the pre-landing program state only; the post-landing program-state update
records this file's SHA-256, avoiding a hash cycle.

| Field | Value |
|---|---|
| Block | `C1` |
| Target branch | `program/architecture-lock` |
| Merge method | one-commit squash followed by true fast-forward |
| Reviewed candidate | `c46c60c52f33784356a9f1d7fade31627486e874` / `031c84419aaa1bc851c24e31add987c9ad678ba8` |
| Authorized gate/freeze carrier | `a2de5e39070da1ba5718b736f39d46d6f04fc398` / `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa` |
| Accepted base | `e9b769d98972f22576f7661150b9d3a2f3ac15ba` / `a46c602ed221f8f34ecd37a520393a9972b70088` |
| Accepted commit | `267cfd0079022dd278b2414e209f459f27d6a721` / `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa` |
| Candidate and accepted patch SHA-256 | `d917d006dbca173fe5e976462752bed752819661ce00e13cce52145cbaa265ae` |
| Candidate evidence digest before landing | `c477c09336b208c2540b4b22b6bb89d3221b186884908d1f9cd88dc12eb5072a` |
| Step-6 manifest SHA-256 | `5a845946da8f9956d325172e66fe51754bc8b0e3cae5dbc83c760a65b8d6e630` |
| Program state before landing SHA-256 | `0355b3e4c79edd77f438158ef97d8d6bf345bb5f5f1a75286d86a5eb9af9c4ac` |
| Manual conflict resolution after review | `false` |
| Generated tree clean | `true`; full-tree identity is stronger than a separate generated subset digest |

The canonical patch digest was computed for both sides with:

```text
LC_ALL=C git -c core.quotePath=true diff --binary --full-index --no-renames \
  --no-color --no-ext-diff --no-textconv <base> <head> -- .
```

using `e9b769d98…` to `a2de5e39…` for the authorized carrier and
`e9b769d98…` to `267cfd007…` for the accepted commit. Both SHA-256 values are
the value in the table. In addition:

- `git diff --exit-code a2de5e390… 267cfd007… -- .` returned 0;
- both commits name tree `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa`;
- `267cfd007…` has first parent `e9b769d98…`;
- `git rev-list --count e9b769d98…..267cfd007…` returned `1`;
- `git log --merges e9b769d98…..267cfd007…` returned no commits;
- the target and retained source worktrees were clean at the landing boundary.

The exact-review bridge is kept distinct from squash equivalence. The three PASS
receipts name `c46c60c5…` exactly. Its single descendant `a2de5e39…` adds only the
13-path exact evidence/program-state carrier described in `landing-record.md`.
The maintainer expressly authorized landing that already-gated carrier without
restamping those reviews. No receipt is rewritten to claim otherwise.

The canonical gate evidence is bound by the exact carrier tree and the following
raw SHA-256 identities:

- surface-1 output:
  `f0c6564fb8e6ca73aa13c036479beb456a5433c5d02cfa83a8e2a41dce101906`;
- telemetry JSON:
  `97385e45e6a7d4c307b4269816e2d1543c3ed854562f43a93a0737b81b9f9139`.

No heavy command, review, performance run, or verifier cycle was rerun during
landing. The only post-landing checks were the identity, patch, ancestry, commit
count, merge absence, validator, and clean-worktree checks recorded here and in
the metadata commit.
