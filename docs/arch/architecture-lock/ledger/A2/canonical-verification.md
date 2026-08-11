# A2 canonical verification — candidate 80a7d9c328842f1457e866fb8588687e9f1d3118

Tree: `eaffd3997f140c2c881179e8089ef6bd05b9bc8d`
Base: `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83`

The candidate commit precedes both runs; no tracked change followed them.

## Canonical pair (contracts/baseline-lock.md §4)

| # | command | cwd | env/features | exit | discovered | executed | passed | failed | skipped/ignored | binaries |
|---|---|---|---|---|---|---|---|---|---|---|
| 40 | `cargo nextest run --workspace` | worktree root | default profile, workspace feature unification | 0 | 24662 | 24080 | 24080 | 0 | 582 | 78 |
| 41 | `cargo test -p verter_session --tests` | worktree root | package-scoped, default features | 0 | 8720 | 8157 | 8157 | 0 | 563 ignored | 4 |

Row 41 note: the log contains nine `test result:` lines because five are nested
in-test re-invocations; four binaries actually ran, and the aggregate over all
nine lines is 8157 passed / 0 failed / 563 ignored.

Surface 40 is unfiltered — no `-E` selector, no `--skip`, no tolerated failure.
Surface 41 is package-scoped deliberately: the workspace form of
`cargo test --tests` drops the `verter_session` integration suite through
`session_metrics` feature unification, so the package-scoped form is the only
one that executes it.

## Why the canonical pair was required

Targeted selectors passed on this content while the canonical run failed
`phase_archaeology_test_files_count_zero` — seven comments in the new module
narrated implementation history instead of stating invariants. They were
rewritten in final-state terms, preserving the construct-signature reachability
fact, and the guard passes. An earlier round of this block was likewise caught
by `lib_rs_stays_under_line_ceiling` only under a full run. Both defects were
candidate-introduced and invisible to the targeted trio.

## Raw output digests

Digests for the two raw logs are carried in `digests.txt`, which covers every
file in this bundle and verifies with `shasum -a 256 -c digests.txt` (exit 0).
