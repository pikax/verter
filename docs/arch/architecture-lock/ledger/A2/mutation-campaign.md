# A2 comparator mutation evidence — candidate 80a7d9c328842f1457e866fb8588687e9f1d3118

Tree `eaffd3997f140c2c881179e8089ef6bd05b9bc8d`.
Comparator module blob: `44f572eb7fe61411a2734ab3d013125ebdb1ee63`.

Three mutation campaigns bear on this block. They differ in what they prove and in
whether an artifact survives. Under the repository's *Verification Must Prove
Execution* rule a figure attested only in prose is not evidence, and that applies
to every seat equally — including the reviewing ones.

## 1. Implementation-side campaign — UNPROVEN, no artifact

The implementation seat reported 79 mutations, 0 survived, 6 predicates deleted,
0 residuals, each run executing 51 tests with byte-restore proven.

**No artifact exists.** That seat ran under a sandbox whose writable roots
excluded this evidence directory, so `command-proofs/40-round4-mutation-matrix/`
was created empty; no driver, per-mutation log, results file or digest row was
written. The directory has been removed rather than left as a hollow marker.

This figure is recorded for completeness and is **not relied upon**.

## 2. Review-side campaign — reported, artifact not retained in this bundle

The adversarial mandate enumerated comparison predicates **from source,
independently of any manifest**, and ran its own campaign against this candidate:

| comparator | predicates |
|---|---|
| `lit_matches` | 3 |
| `set_matches` | 2 |
| `node_matches` | 21 |
| `checker_syntax::matches_node` | 19 |
| `check_boundary` | 5 |
| `first_call_cold_clauses` | 2 |
| `replay_clauses` | 7 |
| `check_boundary_refusal` | 8 |
| `check_cell_outcome_measured` | 7 |
| **total** | **74** |

Reported result: **74 caught / 0 survived / 0 non-runs**, every plant proven
unique-and-new before application and present-exactly-once after, every run
executing exactly 51 tests with the driver aborting otherwise, every restore
proven by SHA-256 and an empty `git status --porcelain`. Completeness was checked
by grepping every `-> bool` and `-> Vec<String>` helper, finding no predicate
outside the 74.

This is the strongest *claim* available — the predicate set was derived from
source by a party that did not write it. But its raw logs live in that review's
own scratch and are **not retained in this bundle**, so by the same rule applied
in §1 it is a reported result, not a retained artifact. It is recorded here as
review testimony, not as this bundle's proof.

## 3. Final-blob binding campaign — PROVEN, artifact retained

To leave at least one campaign that this bundle can prove, a bounded campaign was
run against the final blob with its driver and logs retained:

- driver: `mutation-driver.mjs` (in this directory, re-runnable)
- logs and results: `command-proofs/50-final-blob-mutation-matrix/`
- bound blob: `44f572eb7fe61411a2734ab3d013125ebdb1ee63`

Three representative predicates across both comparators:

| id | predicate neutered | guarding control | verdict |
|---|---|---|---|
| `M-LIT` | `lit_matches` string equality | `literal_expectation_rejects_a_different_value` | CAUGHT-BY-NAMED-CONTROL |
| `M-SIGKIND-NODE` | `node_matches` call/construct discriminant | `construct_signature_is_distinct_from_call_signature` | CAUGHT-BY-NAMED-CONTROL |
| `M-SIGKIND-CHECKER` | checker matcher call/construct discriminant | `construct_signature_is_distinct_from_call_signature` | CAUGHT-BY-NAMED-CONTROL |

`hardFailure=false`. Each run executed 51 tests; the driver fails hard below 40.
Each plant was proven present-once-and-new before application; each file was
byte-restored and the restoration proven by blob identity plus an empty
`git status --porcelain`.

The driver is deliberately fail-closed on its own operation: its first execution
**refused to run** because two needles matched 0 and 2 times rather than exactly
once, reporting `PLANT-NOT-APPLICABLE` and a hard failure instead of a green
result. An unapplied plant that reports success is the precise false-pass this
block exists to prevent.

This campaign is narrower than §1 or §2 — three predicates, not 74 or 79. It does
not claim comparator-wide completeness. It claims exactly what it proves: on the
final blob, these three predicates are guarded by controls that fail when the
predicate is neutered.

## What the bundle can and cannot prove

- **Proven here:** the three predicates in §3, bound to the final blob, with a
  re-runnable driver and retained logs.
- **Proven in-tree:** every control ships in the candidate and runs green (51/51
  in the U6 filterset; 24080/24080 and 8157/8157 on the canonical pair). Their
  presence and passing is retained, reproducible evidence.
- **Reported but not retained here:** comparator-wide completeness at 74 or 79
  predicates. Two independent seats concur on it; neither left an artifact in this
  bundle.

A future block wishing to rely on comparator-wide completeness should re-run
`mutation-driver.mjs` with the full predicate list rather than cite §1 or §2.
