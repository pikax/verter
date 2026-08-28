# C1 landing record

Status: **ACCEPTED / LANDED**.

## Identities

- Target: `program/architecture-lock`.
- Pre-landing target: `e9b769d98972f22576f7661150b9d3a2f3ac15ba`, tree
  `a46c602ed221f8f34ecd37a520393a9972b70088`.
- Reviewed candidate: `c46c60c52f33784356a9f1d7fade31627486e874`, tree
  `031c84419aaa1bc851c24e31add987c9ad678ba8`.
- Gate/freeze carrier: `a2de5e39070da1ba5718b736f39d46d6f04fc398`, tree
  `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa`.
- Accepted C1 commit: `267cfd0079022dd278b2414e209f459f27d6a721`, tree
  `c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa`.

The designated maintainer explicitly ordered the already-gated carrier to be
squashed and landed without another review, performance, or verifier cycle. The
three mandate receipts remain attached to `c46c60c5…`; they are not restamped.
The `c46c60c5…` to `a2de5e39…` delta is exactly 13 paths, all under C1 evidence
or the program-state ledger. Production, tests, toolchain, configuration,
contracts, charters, DAG, and performance thresholds are byte-identical.

## Canonical gate

The default gate ran exactly once on `a2de5e39070da1ba5718b736f39d46d6f04fc398`
and exited 0. Surface 1 passed `25,539/25,539`; `598` tests were skipped and `35`
trybuild cases were excluded under the canonical interim policy. The shipped-cfg
guard remains temporarily skipped, so this is disclosed as a Surface-1-only pass.

- Surface-1 output SHA-256:
  `f0c6564fb8e6ca73aa13c036479beb456a5433c5d02cfa83a8e2a41dce101906`.
- Gate telemetry JSON SHA-256:
  `97385e45e6a7d4c307b4269816e2d1543c3ed854562f43a93a0737b81b9f9139`.

The raw files remain in the retained C1 worktree under
`target/gate-runner/gate-work/`. The gate was not rerun after the tree-identical
squash.

## Atomic landing

The prescribed squash message was used. The accepted commit has exactly one
parent, the pre-landing target. The range contains one commit and zero merge
commits. The target advanced by true fast-forward. No conflict resolution,
regeneration, or content edit occurred during squash or landing.

`landing-equivalence.md` records the canonical patch digest, full-tree equality,
and lightweight post-landing checks. A separate metadata commit binds this receipt,
the landing-equivalence digest, actual accepted SHA/tree, and maintainer acceptance
without changing the accepted C1 tree or pretending the metadata commit is part of
the atomic block delta.

Literal performance failures and their exact-subject dispositions remain recorded
unchanged. Their successor limitations remain owned by the end-of-C-train
consolidation obligation.
