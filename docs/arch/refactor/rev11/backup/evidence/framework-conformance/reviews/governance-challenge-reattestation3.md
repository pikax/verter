# AMD-005 governance/DAG narrow reattestation 3

## Scope

This is a single-point, impact-bounded recheck of finding 2 from
`governance-challenge.md`. Findings 1 and 3 were already confirmed resolved and are
out of scope. No package file was modified; the only write is this report.

## Git exclusion and candidate identity

`git check-ignore -v .agent-run/architect-report.yaml` reports:

```text
.gitignore:40:.agent-run/ .agent-run/architect-report.yaml
```

`git ls-files .agent-run` returns no paths. Resolving the exact landed fix tree with
`git ls-tree -r --name-only 7442bb9060b7faa0720e528d3f96ee1df1abff95 --
.agent-run` also returns no paths, and `git cat-file -e
7442bb9060b7faa0720e528d3f96ee1df1abff95:.agent-run/architect-report.yaml` confirms
that the on-disk file is absent from that commit.

Therefore `.agent-run/architect-report.yaml` is a gitignored, untracked scratch note.
It is not a member of the reviewed candidate's committed tree or identity and is not
one of the package's `files_changed` entries. Its former staleness could affect only
the readability of that local note; it could never change or invalidate the candidate
commit/tree envelope.

## Updated scratch binding

The updated identity fields at `.agent-run/architect-report.yaml:4-10` are internally
consistent:

- `base_commit` is `b3249d13d07806a14a4307954dfcc459cf7301ac`, and
  `git rev-parse b3249d13d07806a14a4307954dfcc459cf7301ac^{tree}` resolves to the
  declared `base_tree` `57e412549c24c903877b471000569c99591a49fc`.
- `git rev-parse ce1d0e4688af1b5bd548b6b68286632cc0f7ede8^` resolves to that exact
  accepted-B1 base commit, so it is the package candidate's parent.
- `candidate_commit` is the exact landed fix
  `7442bb9060b7faa0720e528d3f96ee1df1abff95`, and `git rev-parse
  7442bb9060b7faa0720e528d3f96ee1df1abff95^{tree}` resolves to the declared
  `candidate_tree` `69502487b55f87eb7c0c009876865b64397da660`.
- `git rev-list --reverse --ancestry-path b3249d13d07806a14a4307954dfcc459cf7301ac..7442bb9060b7faa0720e528d3f96ee1df1abff95`
  returns, in order, exactly the two entries under `commits`: the reviewed package
  candidate `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8` and the landed fix
  `7442bb9060b7faa0720e528d3f96ee1df1abff95`.

## Tracked-file stale-binding recheck

At commit `7442bb9060b7faa0720e528d3f96ee1df1abff95`, the tracked `reviews/`
directory contains six files and the committed tree contains no `.agent-run` paths,
so there are no tracked `.agent-run` YAML or prompt files to check.

A grep of the tracked review files for the obsolete candidate/tree identities found no
active stale binding:

- `architecture-challenge.md:3-8`, `conformance-challenge.md:10-14`, and
  `governance-challenge.md:5-12` bind the primary reviews to the exact reviewed package
  candidate `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
  `1ff1f83d8e994b6f1169b0b209c9f557c23f4728`, with base
  `b3249d13d07806a14a4307954dfcc459cf7301ac` where stated.
- `architecture-challenge-reattestation.md:3-9` and
  `governance-challenge-reattestation.md:3-9` explicitly mark their older bindings as
  `SUPERSEDED AFTER REBASE`, state that they do not approve the rebased bytes, and point
  to the current primary reports bound to `ce1d0e468...` / `1ff1f83d...`.
- The remaining obsolete hashes are quoted only as historical evidence inside those
  superseded reports or inside the original finding at `governance-challenge.md:44-61`.
  They do not purport to bind a current report or the landed fix.

No tracked, committed file exhibits the stale-current-binding defect from finding 2.

## Verdict

**PASS — finding 2 fully resolved; the stale scratch binding was never a
committed-identity defect, bound to commit
`7442bb9060b7faa0720e528d3f96ee1df1abff95`.**
