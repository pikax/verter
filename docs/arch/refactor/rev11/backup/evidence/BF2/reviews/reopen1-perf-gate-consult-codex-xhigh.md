RULING: (c)

# Maintainer-ratification recommendation

Ratify a narrow **first-implementation bootstrap-gate protocol** for this cell.
BF2's existing 10-run session and every timing/RSS threshold derived from it are
invalid as gate-setting evidence and cannot be rehabilitated by a later review.
However, the cell does not need a second implementation or a later program block.
The BF2 implementation may be the measured subject if gate authority is separated
from implementation, all discretionary acceptance criteria are frozen before a
fresh measurement, and a disjoint post-freeze holdout session supplies the BF2
pass/fail evidence.

This is deliberately neither (a) nor (b):

- It is not (a), because an absolute limit such as `2x measured BF2 time` still
  lets an arbitrarily slow first implementation manufacture a passing absolute
  gate. A fixed multiplier removes after-the-fact tuning but does not create an
  implementation-independent product budget. Nor may the current 10-run session
  be selected retroactively as the calibration session.
- It is not (b), because Revision 11 defines independence by role, clean context,
  evidence access, and freedom to return `NOT PROVEN`; it does not require a
  different implementation or an arbitrary later block. A distinct,
  maintainer-authorized gate-setting and measurement process can be completed
  during the BF2 reopen before BF2 is re-reviewed and accepted.

Until the procedure below completes, the row must be restored to an explicitly
open/deferred state and BF2 must not claim that its performance exit passed.

## Why this satisfies the locked architecture

The controlling rule is `governance.md`'s Gate authority sentence: candidate
measurements cannot choose their own pass criteria. BF1 reinforces it twice:
required exit 6 says the complete cell must be frozen before BF2 work, and the
abort rule names a criterion selected after candidate measurement. BF2, in turn,
says the cells locked by BF1 must pass. The accepted BF1 evidence resolved the
otherwise-impossible first-implementation case by leaving this exact workload
visible and deliberately open; it did not authorize BF2 to fill the numbers from
its own implementation/review loop.

The A6 lock record also establishes a useful distinction:

- absolute wall/RSS limits are product budgets, not fits to measured candidate
  behavior; and
- relative limits may be derived mechanically from measured noise under a rule
  frozen in advance.

For a first implementation there is no prior same-cell executable for a meaningful
relative comparison. The initial BF2 performance decision must therefore rest on
independently frozen absolute budgets plus correctness/non-vacuity/work gates. A
measurement of the accepted first implementation may bootstrap the relative
baseline for later candidates, but that relative baseline must not be represented
as independent evidence that BF2 was fast enough in the first place.

The current cell violates that separation. Its 5-second wall budget was selected
and rationalized after observing approximately 0.3 seconds, its RSS headroom was
rationalized against the observed maximum, and its relative wall/RSS limits were
computed from the same 10 samples. The ledger therefore correctly invalidates the
prior verdicts. An independent reviewer merely approving those already-visible
numbers would review a post-hoc choice; it would not undo it.

## Exact bootstrap procedure

### 1. Maintainer authorizes the one-time protocol before any fresh run

The maintainer must ratify this procedure as a narrow architecture/Implementation
Lock Record amendment under governance sections 1.1 and 10 and ADR-016. The
ratification must state that this is the only first-implementation bootstrap
exception: it does not authorize later candidates to set or relax their gates.

The existing raw session and derived numeric performance limits remain retained and
labelled **invalid/superseded** for auditability. They are not inputs to the new lock.
Functional facts independently derivable without timing the candidate—such as the
declared 48/36/12 fixture-by-axis counts—may be reused only after the independent
gate authority derives and pins them from the corpus contract.

### 2. Freeze the measurement subject

After all BF2 functional fixes are complete, the orchestrator records one exact
candidate SHA and tree and pins:

- the `generate-goldens.mjs` blob, package manifest/lock closure, six fixture blobs,
  official Vue/Svelte package versions, and committed golden oracle;
- the exact command and driver blob;
- the A6 runner class, Node version, zero-network sandbox, start/end control
  benchmark, and the 3% maximum control drift; and
- the exact validity assertions and fixture-derived work counts.

Any subsequent change to the generator, fixtures, package closure, driver,
correctness oracle, or candidate tree invalidates the resulting BF2 performance
evidence. It does not permit threshold adjustment.

### 3. Appoint an independent gate authority

The maintainer appoints a performance-gate reviewer who did not implement BF2, did
not author or review the invalid BF2 performance session, and has not inspected its
numeric timing/RSS results or candidate-derived limits. The reviewer works in a
clean session from a packet containing the exact subject and product/CI operating
contract while excluding the invalid performance-result files and threshold
commentary. The reviewer must have direct source and command access and authority to
return `NOT PROVEN`.

If no reviewer with that clean context is available, the cell remains open and the
maintainer must appoint a new context; a reviewer who already knows the result
direction cannot recreate blindness by declaration.

### 4. Commit and ratify a pre-measure registration

Before executing the generator under any timer or RSS observer, the independent
gate authority commits a digest-addressed registration containing all of the
following:

1. exact candidate SHA/tree and all cell-identity pins from step 2;
2. **exact numeric absolute wall and peak-RSS limits**, justified solely as
   operational product/CI budgets and not as a multiple or margin over BF2;
3. 30 full cold invocations, because this is a sub-second short cell and
   `[statistics].short_min_samples = 30`; the invalid session's choice of 10
   long-cell runs is not carried forward;
4. median wall time, maximum peak RSS, no discretionary sample exclusion, whole-run
   invalidation on a failed validity assertion, and whole-session invalidation when
   the predeclared machine/control checks fail;
5. the exact correctness oracle, zero-network assertions, and exact-equality work
   counters derived before the performance run; and
6. the relative-baseline algorithm, fixed as
   `max(3.0000%, 2 * population coefficient of variation)`, independently for wall
   and RSS across the 30 valid calibration observations, with the result rounded
   upward—not downward—to four decimal places.

The maintainer ratifies the registration digest **before the first calibration
invocation**. After that ratification, neither the reviewer nor implementer may
change a budget, formula, sample count, statistic, precision, corpus, or validity
rule in response to any observed value. This pre-ratification is the
freeze-before-measure event.

The absolute budgets in item 2 require a real maintainer/reviewer product decision;
the BF2 implementer must not propose or fill them. A rule based only on BF2's
observed runtime or RSS is not eligible for ratification.

### 5. Run one selected calibration session and mechanically finalize the lock

The independent performance reviewer, or a separate neutral runner appointed by
the maintainer, selects one session before starting it and executes exactly the 30
registered invocations on the locked runner. The BF2 implementer does not run or
curate the session. Every sample is retained. A session may be discarded only for a
predeclared validity/control failure identified before its result statistic is
read; a slow or failing valid session may not be replaced.

The final numeric relative limits are produced only by the ratified formula. The
raw data, validity record, arithmetic, candidate identity, and resulting
`performance-gates.toml` digest are retained. Independent review verifies only that
the pre-ratified transformation was followed; it has no discretion to tune the
result. The maintainer then ratifies the new Implementation Lock Record digest.

This calibration establishes the accepted-candidate baseline for **future**
no-regression comparisons. It is not, by itself, BF2's pass evidence. In the lock
record, the first-implementation disposition must say explicitly that BF2 admission
is controlled by the pre-frozen absolute/correctness/work limits and that the
relative values are a bootstrapped future-candidate baseline.

### 6. Run a disjoint post-freeze holdout gate

Only after the numeric cell and new lock-record digest are frozen, a neutral runner
executes a second session with 30 invocations per arm. It alternates the frozen
calibration-baseline checkout and the exact BF2 acceptance candidate in the locked
ABBA policy, retains every sample, and applies the same machine controls, sandbox,
correctness oracle, and work assertions. On the initial bootstrap the two arms may
have identical generator blobs; that is expected and does not substitute for the
absolute product-budget checks.

All pre-frozen absolute limits, correctness assertions, zero-network assertions,
and exact work counters must pass. The holdout comparison must also fall within the
mechanically frozen relative noise limits. A valid failure blocks BF2; it cannot
trigger another calibration or a relaxed margin. Only a genuine benchmark-premise
or environment change may reopen the lock through the ordinary blind recalibration
rules.

### 7. Re-review BF2 on one exact candidate

The complete registration, maintainer ratification, raw calibration, mechanical
derivation, final lock digest, raw holdout result, and pass/fail report become
digest-addressed BF2 evidence. BF2 may then enter fresh conformance, architecture,
and adversarial performance/memory review on the exact candidate. Any relevant
candidate change requires the affected gate evidence to be rerun; it does not let
the candidate select new criteria.

## Ratification boundary

I recommend that the maintainer ratify this bootstrap protocol, not the current
BF2 cell values. If the maintainer does not ratify the protocol, the only compliant
fallback is to keep `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` explicitly
open/deferred and withhold BF2 acceptance. Ratification must not be construed as
acceptance of BF2's other reopened findings or as authority to expose BF3; those
remain subject to their own corrected evidence and exact-candidate review.
