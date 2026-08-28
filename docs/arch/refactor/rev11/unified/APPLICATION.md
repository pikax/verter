# Applying and activating the package

## Trusted-local runtime

Choose a dedicated runtime root outside every Git worktree. Runtime evidence remains external. One repo-global lock and mutable local anchor coordinate all registered roots. This is an honest-operator local-consistency and audit model; it does not claim cryptographic harness authenticity, independent anti-rollback, malicious-owner resistance, or protection from intentional toolchain replacement.

## Phase-aware lifecycle

The canonical lifecycle is tested and fail-closed:

1. `DORMANT`: the exact J1 landing evidence is absent, or ORC0 has not been accepted and activated. No ordinary v2 node is READY.
2. `ORC0`: exact accepted C1 evidence plus the exact Git-verified J1 `LANDED_GRANDFATHERED` receipt and the superseding trusted-local directive make only ORC0 eligible. ORC0 is dispatchable while the static package remains dormant.
3. `ACTIVE`: the active static state binds the current accepted trusted-local ORC0 round, activation directive, and whole-authority digest. Strict validation rejects every partial transition.

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
node tools/programctl.mjs admit ORC0 --holder NAME --candidate-ref refs/heads/BRANCH --runtime-root PATH
node tools/programctl.mjs dispatch ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ORC0 --holder NAME --lease-id LEASE_ID --runtime-root PATH
```

Admission automatically computes per-role effort, materializes implementation/review/verification/confirmation briefs, and returns their exact paths. Record each fresh harness task without a provider CLI in the lifecycle tool:

```text
node tools/programctl.mjs harness-record --role review --round-id ROUND --lease-id LEASE --holder NAME --lens LENS --task TASK --provider PROVIDER --model MODEL --effort TIER --prompt PROMPT --report REPORT --runtime-root PATH
node tools/programctl.mjs harness-record --role verification --round-id ROUND --lease-id LEASE --holder NAME --task TASK --provider PROVIDER --model MODEL --effort TIER --prompt PROMPT --report REPORT --runtime-root PATH
node tools/programctl.mjs harness-record --role confirmation --round-id ROUND --lease-id LEASE --holder NAME --task TASK --provider PROVIDER --model MODEL --effort TIER --prompt PROMPT --report REPORT --runtime-root PATH
```

Only the current round's clean three-of-three reviews plus verification and confirmation can accept and activate:

```text
node tools/programctl.mjs round-accept ROUND --holder NAME --runtime-root PATH
node tools/programctl.mjs activate --activated-by NAME --runtime-root PATH
```

The activation transaction uses an exclusive lock, immutable transition journal, staged root/activation bytes, and regenerated projections. Any partial/interrupted transition remains strictly refused. ORC0 may verify receipts, bind the authority digest, retire the selected mutable orchestration path, and flip activation only. It may not alter DAG semantics.

## Atomic admission and leases

```text
node tools/programctl.mjs admit ID --holder NAME --candidate-ref refs/heads/BRANCH --runtime-root PATH
node tools/programctl.mjs dispatch ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs lease-renew LEASE_ID --holder NAME --runtime-root PATH
node tools/programctl.mjs lease-release LEASE_ID --holder NAME --outcome FIX_REQUIRED --runtime-root PATH
```

Admission is low-admin: omitted effort overrides are normal. `--effort ROLE=TIER` may raise but never lower the deterministic tier. Invalid calls refuse before anchor/runtime mutation; every locked operation recomputes before journaling. If the anchor is lost, use `trusted-local-reinitialize --operator NAME --reason TEXT`; the new lineage is visibly `unknown/lost`.

## Acceptance

Acceptance binds real candidate Git identity, the current exact lease/round/finalization, the deterministic effort policy, three distinct fresh review task identities and their exact provider/model/effort/prompt/report digests, plus verification and confirmation. These bindings are operator-attested local audit records. Final acceptance is current-round exact-candidate clean 3/3; P0/P1 block, and P2/P3 follow owning policy. A content or effort change invalidates the evidence. A closed or superseded round can never become accepting later.

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
