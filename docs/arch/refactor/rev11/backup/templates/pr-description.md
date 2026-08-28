# Verter block `<BLOCK_ID>` — `<TITLE>`

## Program and stack identity

- Revision 11 package digest:
- A6 Implementation Lock digest:
- pre-stack program-state basis digest:
- current program-state digest:
- block charter digest:
- direct predecessors and accepted SHAs/trees:
- stack window / StackSnapshotId / unique layer_id:
- acceptance_block_id / block_id:
- layer kind: mergeable | private-review-only
- atomic landing group:
- base SHA/tree:
- head SHA/tree:
- patch digest / range-diff after latest restack:

## Required end state

Describe the one invariant/owner/cutover this layer establishes. Do not repeat the whole master plan.

## Scope

**In:**

**Out:**

**Allowed breaking changes:**

## Surviving path and deletion set

- surviving production implementation:
- deleted declarations/implementations/caches/tasks/flags/metrics/dependencies/docs:
- proof that no runtime switch or shadow path remains:

## Behavior and compatibility

- capability rows affected:
- semantic/output/profile/protocol changes:
- migration/rejection behavior:

## Evidence

| Proof | Command/run ID | Non-vacuous count | Raw artifact + digest | Result |
|---|---|---:|---|---|

## Work, performance, and memory

- locked cells/gates:
- baseline versus candidate:
- work amplification/copies/allocations:
- peak/retained memory and pins:
- exceptions: none, or pre-candidate ratified record only

## Review state on this exact candidate

| Mandate | Reviewer/context | Status | Candidate SHA/tree | Report digest |
|---|---|---|---|---|

## Restack history

Record every lower-layer change, old/new base, range-diff, manual conflict resolution, re-run CI, and review reattestation.

## Discoveries and deviations

- blocking:
- non-blocking and disposition:
- architecture deviation: none | link/digest

## Landing

- merge mode:
- reviewed candidate SHA/tree:
- predicted target SHA/tree:
- eventual validated landing-equivalence artifact/digest (reviewed delta = accepted delta on recorded bases):
- required post-merge checks:
- maintainer decision:
