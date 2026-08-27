# Applying and activating the package

## Runtime custody

Choose a dedicated runtime root outside every Git worktree. Receipts, external authorizations, gate/review evidence, amendments-in-progress, and leases are runtime evidence; they are never static authority inputs or committed worktree state. Every operational command takes `--runtime-root PATH` and refuses an unsafe/in-worktree root.

## Phase-aware lifecycle

The canonical lifecycle is tested and fail-closed:

1. `DORMANT`: the exact J1 landing evidence is absent, or ORC0 has not been accepted and activated. No ordinary v2 node is READY.
2. `ORC0`: exact accepted C1 evidence plus the exact Git-verified J1 `LANDED_GRANDFATHERED` receipt and the immutable, narrowly scoped 2026-08-27 maintainer directive make only ORC0 eligible. The directive is not a J1 acceptance receipt and authorizes neither BR0, TCM0R, review findings, nor amendments. ORC0 is dispatchable while the static package is still dormant.
3. `ACTIVE`: the active static state binds the exact accepted ORC0 receipt, candidate/integration Git tree, and whole-authority digest. Strict validation rejects every partial or forged transition.

Inspect the derived phase with:

```text
node tools/programctl.mjs phase --runtime-root PATH
```

Import the exact immutable J1 landing evidence without restamping it:

```text
node tools/programctl.mjs landed-receipt-import J1-LANDED-RECEIPT.toml --runtime-root PATH
```

The import verifies the commit, tree, parent, branch containment, landed live-charter bytes, evidence tree, and context packet before atomically binding `j1_state`. It keeps J1 out of the accepted-receipt map. Admit and dispatch ORC0, advance the implementation branch if required, then freeze its exact candidate:

```text
node tools/programctl.mjs admit ORC0 --holder NAME --candidate-ref refs/heads/BRANCH --gate-runner NAME --reviewer LENS=NAME --runtime-root PATH
node tools/programctl.mjs dispatch ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs authorization-create ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
```

`authorization-create` derives the candidate-bound ORC0 authorization from the immutable directive slot only after dispatch and finalization; importing a fabricated directive-mode authorization is refused. Gate/review PASS imports are also refused. The canonical runners must create the evidence themselves for the frozen candidate: `gate-run` executes the exact profile/charter command plan with per-child timeouts and records every argv, exit, signal, timeout, elapsed time, stdout/stderr, and digest; `review-run` executes only the exact executable named by a separately ratified immutable reviewer capability and reconciles its schema-valid report.

```text
node tools/programctl.mjs gate-run ORC0 --scope candidate --integration-sha CANDIDATE_SHA --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs gate-run ORC0 --scope integration --integration-sha CANONICAL_INTEGRATION_SHA --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs review-run ORC0 --lens LENS --custody-binding review-capability:SHA256 --holder NAME --lease-id LEASE_ID --runtime-root PATH
```

Repeat `review-run` for every exact assigned lens. The committed trusted-reviewer ledger is intentionally empty because no authentic external reviewer credential can be proven locally. The maintainer directive does not grant reviewer identity: ORC0 therefore remains fail-closed at this boundary until externally ratified capabilities are added through owning authority. Only after both gates and all three custody-proven reviews exist may the validated ORC0 acceptance receipt be imported and the atomic transition performed:

```text
node tools/programctl.mjs receipt-import ORC0-RECEIPT.toml --runtime-root PATH
node tools/programctl.mjs activate --orc0-receipt ORC0:SHA256 --authorization maintainer_unified_v2_activation:SHA256 --activated-by TRUSTED_GRANTED_BY --runtime-root PATH
```

The activation transaction uses an exclusive lock, immutable transition journal, staged root/activation bytes, and regenerated projections. Any partial/interrupted transition remains strictly refused. ORC0 may verify receipts, bind the authority digest, retire the selected mutable orchestration path, and flip activation only. It may not alter DAG semantics.

## Atomic admission and leases

```text
node tools/programctl.mjs admit ID --holder NAME --candidate-ref refs/heads/BRANCH --gate-runner NAME --reviewer LENS=NAME --ttl-seconds 3600 --runtime-root PATH
node tools/programctl.mjs dispatch ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs lease-renew LEASE_ID --holder NAME --ttl-seconds 3600 --runtime-root PATH
node tools/programctl.mjs lease-release LEASE_ID --holder NAME --runtime-root PATH
```

Repeat `--reviewer` in the exact profile-lens order with distinct identities. Admission binds the start SHA/tree, base SHA/tree, checked-out candidate worktree, authority digest, scope, exact epoch/ref/holder/timestamps, gate runner, and reviewers. It derives domains from static authority, excludes a second holder for the same node, and enforces every path/symbol overlap and resource capacity in one atomic operation. Packet construction is part of the transaction; failure rolls the lease back. Dispatch emits immutable packet/dispatch receipts and refuses missing, expired, foreign, stale-ref, or noncanonical leases. Finalization allows the branch to advance after dispatch, preserves the full base-to-candidate delta, and freezes the SHA/tree against which authorization, gates, reviews, and acceptance are validated.

## Acceptance

Acceptance evidence must bind real base/candidate/integration Git objects and ancestry; exact predecessor receipt ID/digest pairs; current authority/control/charter digests; trusted external authorization digests; the base→candidate changed-path/blob delta and lease receipt; the candidate gate plus the separate touched-domain integration gate; and the exact custody-proven reviewer identities/count/lenses/independence/model/effort/report digests required by the node profile. Final-tree equivalence means that `integration_tree` exactly names the Git tree of `integration_sha` and preserves every reviewed candidate-delta blob; unrelated conflict-free integration changes are allowed, so `candidate_tree` need not equal `integration_tree`. Final acceptance is exact-candidate clean 3/3. P0/P1 block. A deferred P2 requires two separately trusted, schema-applied immutable artifacts: an unexpired one-time disposition bound to node, candidate SHA/tree, profile, lens, severity, fingerprint, owner, bounded sweep, and obligation; and a digest-bound `CLOSED` next-cycle receipt for that exact obligation before expiry. P3 does not consume P2 authorization. A content change invalidates gates and reviews. Accepted state cannot bypass activation, predecessor, release, authorization, admission, or amendment blockers.

## Amendments

Check static authority against its immutable lock before any edit:

```text
node tools/programctl.mjs amendment-check
```

An authorized authority change uses:

```text
node tools/programctl.mjs amendment-create AMD-ID --before-root PATH --ratified-by ID --ratification-receipt SHA256 --runtime-root PATH
```

The command computes before/after authority digests, changed paths/nodes, full descendant impact closure, stale receipts, and required revalidation; validates trusted ratification; writes one append-only amendment; and advances the authority lock. Forged authorization, incomplete closure, direct static edits, or attempted history rewrite fail closed.

## Successor authorization split

BR0 is the sole source-canonical successor entry. It remains blocked until both custody-separated external decisions are present: the repair-scoped freeze lift and a distinct successor-genesis authorization issued after accepted L4. Product branches remain independently promotable because BR0 is their common ancestor, not a global product convergence join.
