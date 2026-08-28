# Landing-equivalence note: B1

Reviewed candidate `6ac6ab4d21a1a1b4fbd4ec37f5eafc77fc66f55c` (tree
`7f8230066735db17650b5d594a95d597540b3729`) was squashed to a single commit at
landing, producing accepted commit `03b2fdbfc6d12452824768d9e389a5f6f3d680df`
on `program/architecture-lock` (base `5c24d22a550ce90d369471ea1d00590a0e1b726a`,
the landed gate-memory-ceiling tooling prerequisite).

## Facts verified directly against the repository (program orchestrator, this session)

```
git rev-parse 6ac6ab4d2^{tree}   -> 7f8230066735db17650b5d594a95d597540b3729   (reviewed candidate)
git rev-parse 03b2fdbfc^{tree}   -> 7f8230066735db17650b5d594a95d597540b3729   (accepted, identical)
git merge-base --is-ancestor 6ac6ab4d2 program/architecture-lock -> false (squashed away, tag-protected)
```

The accepted tree is byte-identical to the reviewed candidate tree — this is a
pure history-flattening squash (commit identity changed, content did not).
This is the same pattern already accepted for A0 (per that block's own
landing-equivalence note) and A6 (this program's most recent precedent). The
reviewed candidate is preserved from GC via tag
`program-history/B1-reviewed-candidate`.

## Disposition

Zero content divergence. `accepted_sha` differs from `candidate_sha` by squash
mechanics only; `accepted_tree` equals `candidate_tree` exactly.
