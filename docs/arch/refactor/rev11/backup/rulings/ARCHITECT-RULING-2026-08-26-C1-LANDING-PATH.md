## VERDICT

C1 can land as one atomic commit, but `6b5a87144` is not yet a verified candidate. Measured against trunk `ea401650a`: **156 commits ahead, 5 behind, 345 files, +33,523/−6,724, 2 deletions**. Size does not force a split; this is one cohesive ownership/dependency cutover. The genuine blockers are: C1 is still ledger-`LOCKED` under an authorization that explicitly “dispatches nothing”; the A6 performance cell remains internally mis-pinned; the severed-governance class is not closed; historical preservation of the 24 converted cases is not independently evidenced; and none of the three mandatory reviews has run. No production rewrite is ordered.

## Six rulings

### 1. One landing — YES

Review the cumulative base-to-candidate tree, not 156 individual commits. The only plausible split is before relocation—`1d048bf40` followed by `57da4d8e5`—but that would land preparatory resolver machinery without the atomic ownership/dependency/deletion flip. It creates an incomplete intermediate trunk state and requires two gates and two acceptance lifecycles. Splitting costs more verification than it saves and violates the ruling’s single-cutover boundary.

### 2. Severed-governance sweep — REQUIRED BEFORE LANDING

Three confirmed instances, including a silent correctness defect, establish a class rather than isolated mistakes. It must close before final freeze.

The minimum sufficient sweep is a **join, not a Cartesian product**:

1. Build the complete source→destination identity map for every relocated file/definition.
2. Enumerate every repository governance attachment keyed to either identity across the four named classes.
3. Record, for every moved unit, `followed`, `retargeted`, `retired`, `defect`, or explicit `none`.
4. Repair every correctness/invariant defect and prove each repair structurally or with a discriminating test.
5. Independently verify the origin universe, destination universe, and zero-undispositioned result.

Walking every origin against all 60 destinations produces meaningless null pairs and is cut.

### 3. S2-F4 instrument — DROP THE INSTRUMENT, KEEP THE OBLIGATION

A permanent source/body-reading metaguard is not required and would conflict with the structural-enforcement rule. S2-F4 itself is not deferrable.

The current file contains 24 tests but only 18 `HISTORICAL_WITNESSES` rows, and `assert_historical_witness_if_present` silently does nothing when no expected row matches. The `[R]` assertion that all expected values were recovered by executing the deleted engine is load-bearing and currently supported only by commit prose.

C1 must therefore independently reconstruct the pre-deletion engine in a throwaway checkout, capture all 24 historical outcomes, map each old case to its current fixture/assertion semantically, prove registration and execution, and run discrimination mutations for every materially converted test. Fix any mismatch inside C1. No follow-on instrument is owed once that evidence is complete.

### 4. Review attachment — POST-REBASE, PRE-SQUASH

All three mandates attach to one frozen, post-rebase `FINAL_REVIEW_SHA` and tree, while the 156-commit history still exists:

- conformance;
- architecture;
- adversarial performance/memory.

Every receipt must name exactly `FINAL_REVIEW_SHA`. Before accepting it, run `git merge-base --is-ancestor <reviewed-sha> <tip>`; ancestry is necessary but not sufficient—final PASS receipts must also name the current frozen candidate exactly. Any fix or rebase invalidates the old candidate and requires impact-bounded reattestation by all three mandates; material architectural change requires fresh review.

Squash happens afterward. Verdicts remain attached to the pre-squash SHA and must never be restamped. Exact tree hash, canonical delta equality, generated-output digests, and no manual conflict resolution form the landing-equivalence bridge to the squashed SHA. Any content difference voids the bridge and requires review again.

### 5. What must be redone

No production implementation must be redone merely because the history is large or execution was unauthorized. Evidence must be redone on the final lineage:

- the 24-case historical comparison;
- every `[R]` suite result;
- applicable revert/mutation discrimination proofs;
- both compile-fail rails and the cache-fence assertion;
- the locked A6 performance cell;
- final canonical gate evidence.

The relocation, driver unification, cycle fix, and carrier behavior change are reviewed and retested, not rewritten.

### 6. Scope and handoff correction

TCM0 is not the authority for requiring three mandates; C1’s own ratified Foundational charter is. Its authority move, dependency inversion, broad cross-crate surface, and hot-path/performance exposure prohibit reducing the mandate set. TCM0’s 36 findings do **not** become a C1 checklist.

The handoff misses two genuine prerequisites:

- At trunk `ea401650a`, C1 remains `LOCKED`, all candidate/review fields are blank, and its authorization omits the Stage-2 ruling while stating that execution authority requires another record.
- `A6_META_COMPILE_40_COLD_RUST` remains internally contradictory: the locked blob is `efa9ea54…`, whose actual SHA-256 is `5e06d35d…`, while the lock records `1d208e61…`.

It also lacks the final material-change→acceptance mapping across the entire C1 charter (`C1-AC-1` through `C1-AC-9`), not merely Stage 2.

## Ordered landing sequence

STEP 1 — RECORD AND RATIFY THE HISTORICAL AUTHORITY DEVIATION / The maintainer-delegated architecture seat rules; the trunk registry/ledger owner records it and validates program state / A new registered act states honestly that implementation occurred while C1 was `LOCKED`, adopts the existing branch for remediation and review, binds the charter/addendum/Stage-2 ruling, supersedes the now-impossible pre-dispatch sequence without pretending it occurred, and exposes C1 as active / **S**

STEP 2 — REPAIR THE A6 LOCK / The maintainer/A6 lock authority corrects and reviews the cell; its validator checks both identities and runner availability / `A6_META_COMPILE_40_COLD_RUST` names one actual harness by matching Git blob and SHA-256, and its complete protocol is callable without C1 reconstructing it / **M**

STEP 3 — REBASE AND DEFINE THE COMPLETE SUBJECT / A rebase agent operates only in the C1 worktree; Git proves ancestry, delta-of-deltas, per-file blob identity, and absence of manual conflict resolution / A new base SHA, candidate SHA/tree, complete changed/deleted path set, relocation identity map, and draft mapping of every material change to `C1-AC-1..9`, Stage-2 outcomes, or an explicit exclusion / **M**

STEP 4 — CLOSE SEVERED GOVERNANCE / A scoped implementer repairs findings; independent conformance and architecture readers verify the universe and dispositions / The joined origin/destination governance table, explicit disposition for every moved unit, zero open governance defects, and structural or discriminating evidence for every repair / **L**

STEP 5 — RECONSTRUCT S2-F4 PRESERVATION / An author-independent verifier runs the pre-deletion engine in a throwaway checkout and compares it with the current production-driver tests / A 24-row semantic correspondence with captured historical outcomes, current fixtures/assertions, registration/execution proof, and RED→GREEN discrimination evidence; no unmatched or silently skipped case / **M**

STEP 6 — COMPLETE AND FREEZE THE FINAL CANDIDATE / The block manager closes every charter row, runs targeted affected checks and health checks, and the A6 owner executes the locked cell on base and candidate; then the landing lease is acquired, the branch is finally rebased, and frozen / One `FINAL_REVIEW_SHA`/tree, complete acceptance-coverage map, raw locked-cell receipt, re-derived suite evidence, compile-fail/cache-fence evidence, no open in-scope finding, and the final squash message / **L**

STEP 7 — RUN THE THREE MANDATES TO CONVERGENCE / Three independent contexts review the same immutable `FINAL_REVIEW_SHA`; the results checker validates lane, SHA, and receipt / Three PASS verdicts—conformance, architecture, adversarial performance/memory—on exactly one SHA. Any fix returns through affected Steps 4–6 and all three mandates reattest the replacement candidate / **L**

STEP 8 — INDEPENDENTLY VERIFY, THEN ISSUE READY / A fresh author-independent verifier re-executes the non-canonical acceptance and discrimination commands, checks the governance and 24-case enumerations, validates the performance receipt, evidence digests, ancestry, and unchanged tree / A `READY AND VERIFIED` report carrying the candidate SHA/tree, evidence paths/digests, complete acceptance mapping, three mandate receipts, and squash message / **M**

STEP 9 — GATE, SQUASH, LAND, AND BIND IDENTITIES / The program orchestrator dispatches the landing agent; the protected canonical gate runs once under the machine resource controls / The landing agent verifies no trunk/candidate drift, health checks, gate selection and review receipts; squashes to exactly one conventional commit; proves identical tree hashes before/after squash; true-fast-forwards with zero merges; records landing equivalence, accepted SHA/tree and maintainer acceptance; and validates the final ledger / **L**

## Cut list — 3 items

1. **Cut:** the origin-set × 60-destination Cartesian sweep. Replace it with the complete identity/governance join in Step 4.
2. **Drop:** a permanent S2-F4 source/body-reading instrument. The C1-owned historical reconstruction and discrimination evidence in Step 5 closes the obligation.
3. **Defer outside C1:** re-ratification restoring the 14 gutted ruling cells. Commit `565a652d2` correctly restored the registered bytes, and the ledger already records the stale §6.4 registration row as discharged. Any prose restoration belongs to the authority/ruling owner as separate governance maintenance; it does not gate code acceptance.

===VERTER-RECEIPT-BEGIN===
LANE: c1-architect-ruling
RESULT: RULED
STEPS: 9
CUTTABLE: 3
===VERTER-RECEIPT-END===