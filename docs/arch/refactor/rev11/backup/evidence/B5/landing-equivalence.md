# Landing-equivalence note: B5

Reviewed candidate `7b840cb3d3a79e6c66d8977a46370d0f03fbe940` (tree
`f1dc018b11ff1a5b8ad451b7ccc19b1f7b2ead26`) was squashed to a single commit at
landing, producing accepted commit `c68fe61e3c2dacf89e712fcf60b875bc55ca8ea9` on
`program/architecture-lock` (parent `3f663584e519b208623a311b41367b67d97a6e89` —
BV2's own final accepted commit, so B5 landed as a direct fast-forward
immediately on top of BV2's accepted state, consistent with the DAG edge BV2 ->
B5 the maintainer ruling ratifies).

## Facts verified directly against the repository (this session)

```
$ git rev-parse 7b840cb3d^{tree}
f1dc018b11ff1a5b8ad451b7ccc19b1f7b2ead26   (reviewed candidate)
$ git rev-parse c68fe61e3^{tree}
f1dc018b11ff1a5b8ad451b7ccc19b1f7b2ead26   (accepted, identical)
$ git log -1 --format='%H %P' c68fe61e3
c68fe61e3c2dacf89e712fcf60b875bc55ca8ea9 3f663584e519b208623a311b41367b67d97a6e89
$ git merge-base --is-ancestor 7b840cb3d c68fe61e3 && echo yes || echo no
no
```

The accepted tree is byte-identical to the reviewed candidate tree — this is a
pure history-flattening squash (commit identity changed because the branch's
incremental fix-round commits were flattened to one; content did not). The same
byte-identical-tree pattern is already accepted for A0 and B1 (see B1's own
landing-equivalence note, the most recent precedent). **Corrected after
adversarial review:** A6 was originally cited alongside them as a third
precedent; A6's own row does NOT have this property — its candidate_tree
(`7e2977ea...`) and accepted_tree (`f709d672...`) differ — so A6 is not a
precedent for this specific pattern and the earlier citation was wrong.

## Disposition

Zero content divergence. `accepted_sha` differs from `candidate_sha` by squash
mechanics only; `accepted_tree` equals `candidate_tree` exactly.
