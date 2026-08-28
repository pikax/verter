# Applying and activating the package

## Trusted-local runtime

Choose a dedicated runtime root outside every Git worktree. Runtime evidence remains external. One repo-global transaction mutex and mutable local anchor make evidence writes atomic across registered roots; they do not coordinate work ownership, conflicts, resources, or scheduling. This is an honest-operator local-consistency and audit model; it does not claim cryptographic harness authenticity, independent anti-rollback, malicious-owner resistance, or protection from intentional toolchain replacement.

## Phase-aware lifecycle

The canonical lifecycle is tested and fail-closed:

1. `DORMANT`: the exact J1 landing evidence is absent, or ORC0 has not been accepted and activated. No ordinary v2 node is READY.
2. `ORC0`: exact accepted C1 evidence plus the exact Git-verified J1 `LANDED_GRANDFATHERED` receipt and the superseding trusted-local directive make only ORC0 eligible. ORC0 is dispatchable while the static package remains dormant.
3. `REACTIVATION_REQUIRED`: ORC0 was activated against an ancestor authority, but a later ratified amendment has advanced the immutable authority lock. Ordinary successor dispatch remains blocked until an exact external ACTIVE-to-ACTIVE transition binds the prior activation head to the amendment and its receipt-backed ratifier.
4. `ACTIVE`: the external trusted-local transition chain binds the accepted ORC0 candidate, its exact canonical-branch landing receipt, activation directive, every covered authority amendment, and the current whole-authority digest/generation. The tracked authority stays `DORMANT`, so activation and reactivation never mutate reviewed source bytes. Strict validation rejects every partial, forked, skipped, orphaned, or mismatched transition.

Inspect the derived phase with:

```text
node tools/programctl.mjs phase --runtime-root PATH
```

Import the exact immutable J1 landing evidence without restamping it:

```text
node tools/programctl.mjs landed-receipt-import J1-LANDED-RECEIPT.toml --runtime-root PATH
```

The import verifies the commit, tree, parent, branch containment, landed live-charter bytes, evidence tree, and context packet before atomically installing the external runtime binding. It does not mutate the tracked J1 expected-reference pin, and it keeps J1 out of the accepted-receipt map. Without the external receipt the package remains validly `DORMANT`; only an exact match to the tracked pin unlocks ORC0, while a mismatch fails closed. Admit and dispatch ORC0, advance the implementation branch if required, then rebase and squash it to one conventional commit before freezing its exact candidate. The retained `lease-id` option is an opaque round handle, not a work lease:

```text
node tools/programctl.mjs admit ORC0 --holder NAME --candidate-ref refs/heads/BRANCH --runtime-root PATH
node tools/programctl.mjs dispatch ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
```

Admission automatically computes per-role effort, materializes implementation/review/verification/confirmation briefs, and returns their exact paths. Record each fresh harness task without a provider CLI in the lifecycle tool:

```text
node tools/programctl.mjs harness-record --role review --round-id ROUND --lease-id LEASE --holder NAME --lens LENS --task TASK --agent AGENT --provider PROVIDER --model MODEL --effort TIER --prompt PROMPT --report REPORT --runtime-root PATH
node tools/programctl.mjs harness-record --role verification --round-id ROUND --lease-id LEASE --holder NAME --task TASK --agent AGENT --provider PROVIDER --model MODEL --effort TIER --prompt PROMPT --report REPORT --runtime-root PATH
node tools/programctl.mjs harness-record --role confirmation --round-id ROUND --lease-id LEASE --holder NAME --task TASK --agent AGENT --provider PROVIDER --model MODEL --effort TIER --prompt PROMPT --report REPORT --runtime-root PATH
```

Only the complete current-round risk-scaled review profile plus verification and required confirmation can accept. Then fast-forward the canonical branch to the exact accepted squashed candidate and record that landing before activation:

```text
node tools/programctl.mjs round-accept ROUND --holder NAME --runtime-root PATH
git merge --ff-only refs/heads/CANDIDATE_BRANCH
node tools/programctl.mjs landing-record --round-id ROUND --holder NAME --runtime-root PATH
node tools/programctl.mjs activate --activated-by NAME --runtime-root PATH
```

Each landing requires canonical tip equality with that round's reviewed candidate SHA/tree at record time. Every validated accepted-and-landed trusted-local round is then projected as a stable successor receipt using its acceptance digest plus exact candidate and integration identities, so it can satisfy descendant prerequisites. Activation publishes the exact ORC0 transition through the repo-global trusted-local journal and does not edit tracked authority or regenerate projections. After activation, the exact ORC0 acceptance, landing, and transition remain immutable while the canonical tip may advance only as their Git descendant. Any partial, mismatched, or non-descendant transition remains strictly refused. ORC0 may verify receipts, bind the authority digest, and activate the external lifecycle only. It may not alter DAG semantics.

## Atomic admission and round handles

```text
node tools/programctl.mjs admit ID --holder NAME --candidate-ref refs/heads/BRANCH --runtime-root PATH
node tools/programctl.mjs dispatch ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs lease-renew LEASE_ID --holder NAME --runtime-root PATH
node tools/programctl.mjs lease-release LEASE_ID --holder NAME --outcome FIX_REQUIRED --runtime-root PATH
```

Admission is low-admin: omitted effort overrides are normal. The legacy `lease-id`/renew/release spelling is wire compatibility for a round handle and grants no scheduling or exclusion right. `--effort ROLE=TIER` may raise but never lower the deterministic tier. Invalid calls refuse before anchor/runtime mutation; every transaction-mutex operation recomputes before journaling. If the anchor is lost, use `trusted-local-reinitialize --operator NAME --reason TEXT`; the new lineage is visibly `unknown/lost`.

## Acceptance

Acceptance binds real candidate Git identity, the current exact lease/round/finalization, the deterministic effort policy, every risk-profile-assigned fresh review task and its exact agent/provider/model/effort/prompt/report digests, plus verification and risk-scaled confirmation. The implementation evidence is the holder-authored Git identity frozen by finalization; no redundant self-attested implementation harness receipt is required. These bindings are operator-attested local audit records. A PASS contains zero findings; P0/P1 block, and P2/P3 follow owning policy. A content or effort change invalidates the evidence. A closed or superseded round can never become accepting later.

## Amendments

Check static authority against its immutable lock before any edit:

```text
node tools/programctl.mjs amendment-check
```

An authorized authority change uses:

```text
node tools/programctl.mjs amendment-create AMD-ID --before-root PATH --ratified-by ID --ratification-receipt SHA256 --activation-predecessor-authority SHA256 --runtime-root PATH
```

The command computes before/after authority digests, changed paths/nodes, full descendant impact closure, stale receipts, and required revalidation; validates the exact receipt-backed `authority-amendment` slot named by the custody boundary; binds the exact prior ACTIVE authority digest that the amendment supersedes; writes one append-only amendment; and advances the authority lock. The same slot and receipt bytes must already exist in the locked before root and candidate. Omit `--activation-predecessor-authority` only when that prior ACTIVE digest is exactly the locked before-authority digest. Unknown-purpose, after-only, forged, placeholder, or digest-mismatched slots or predecessors, incomplete closure, direct static edits, and attempted history rewrite fail closed.

If ORC0 was already ACTIVE when the amendment lands, the program derives `REACTIVATION_REQUIRED`. Preserve the same external runtime root and publish the exact amendment-bound continuation:

```text
node tools/programctl.mjs amendment-reactivate --runtime-root PATH
```

The command accepts no caller-selected ratifier or receipt. It derives them from the validated amendment head, requires the clean canonical branch and exact current activation head, then atomically writes one immutable ACTIVE-to-ACTIVE transition through the repo-global journal. The transition must advance exactly one authority generation and cannot fork, skip, roll back, or detach from the amendment chain. Legacy receipt files remain immutable history while reactivation is pending; only the exact covered legacy references return to the active receipt map after reactivation. Invalidated runtime v2 receipts are never revived and require new acceptance. Mutable tracked activation state is not rewritten.

The successor-DAG/legacy-cleanup/GitHub-control-plane candidate is one atomic amendment. Rebasing it after ORC0 lands establishes Git ancestry but does not authorize the authority mutation. Before landing, custody must provide an exact trusted authority-amendment slot that is present unchanged in the before and after roots; then `amendment-create` must bind the 71 added nodes, amended existing owners, 418-path deletion/transfer catalog, descendant impact closure, invalidated receipts, and regenerated authority lock. After landing, the retained ORC0 runtime must publish the exact `amendment-reactivate` transition before any successor node becomes executable. The 13 GH/FB/REL nodes remain non-executable until the current accepted ORC0 receipt and complete activation-transition chain validate. Do not synthesize a local ratification row or bypass the strict gate. The optional EPR2 and EPR3 execution paths separately remain fail-closed until their maintainer-managed acquisition/bundling authorizations exist.

## Successor authorization split

BR0 is the sole source-canonical successor entry. It remains blocked until both custody-separated external decisions are present: the repair-scoped freeze lift and a distinct successor-genesis authorization issued after accepted L4. Product branches remain independently promotable because BR0 is their common ancestor, not a global product convergence join.
