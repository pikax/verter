# Revision 10 Implementation and Orchestrator Readiness Review

**Review target:** the released `verter-architecture-lock-master-plan-v10.md` and `verter-architecture-v10.zip`.  
**Decision:** Revision 10 is not safe for direct unrestricted implementation or a one-prompt orchestrator handoff.  
**Disposition:** superseded by Revision 11.

# 1. Executive finding

Revision 10's core target architecture is strong. The direct-core-first boundary, sealed compile semantics, one flow authority, staged project-aware compile, exact parse ownership, result-contract/flight separation, proof-carrying completeness, mapping taxonomy, deterministic identities, bounded memory, and performance gates should be preserved.

The released package was nevertheless not fully implementation-executable. It contained concrete sequencing and artifact-consistency defects, and it did not define how an orchestrator or stacked PR workflow could preserve the plan's exact-candidate and atomic-cutover guarantees.

Revision 11 is therefore an execution-closure revision rather than a semantic redesign.

# 2. Released Revision 10 blockers

## V10-B1 — Entry checkout and implementation baseline were conflated

The released README and baseline materials described the implementation baseline as the SHA captured by `A0`, while later Gate 0 blocks were allowed to strengthen tests, retract wrong-complete behavior, and add instrumentation. Later charters and measurements could therefore claim a baseline that no longer matched the code being implemented.

**Revision 11 correction:** `A0` records `EntryCheckoutSha`; `A6` accepts a distinct exact post-Gate-0 `ImplementationBaselineSha` and tree. Every affected evidence item is refreshed after a Gate 0 lineage change.

## V10-B2 — Measurement could precede the behavior-changing safety retraction

The released DAG placed measurement before the wrong-complete safety retraction. That made the baseline incomparable to the post-safety implementation state and could choose gates from behavior the program explicitly intended to remove.

**Revision 11 correction:** the Gate 0 lineage is strictly `A0 → A1 → A2 → A3 → A4 → A5 → A6`: command truth, discriminating harness, fail-closed safety retraction, then measurement and final inventories.

## V10-B3 — The new direct API could land on transitional source blanking

The released ordering introduced the borrowed direct compiler before compact source units and deletion of full-carrier blanking. That risked making a knowingly transitional source-sized copy part of the new public direct architecture.

**Revision 11 correction:** `B4` atomically introduces compact units/mapping taxonomy and deletes blanking before `B5` cuts over the borrowed direct compiler. `B6` then adds prepared/batch reuse over the surviving direct core.

## V10-B4 — Product/profile and subplan composability was not fully exact

The released product/materialization shape could be read as one global output/presentation/serialization profile applying to multiple requested products. It also bound compile facts primarily to one whole-plan token, which could over-invalidate unchanged semantic projections after a terminal-only request change.

**Revision 11 correction:** product requests are typed and product-local. Required mappings are inherent product outputs; optional terminal forms attach only to the affected product. Whole-plan anti-replay coexists with narrower projection, product, and terminal subplan tokens so unchanged semantic/code work can be reused safely.

## V10-B5 — External observation trust was not explicit enough

A caller-supplied environment could appear to supply authoritative fingerprints. A malicious or buggy adapter could therefore claim a digest inconsistent with supplied bytes/configuration.

**Revision 11 correction:** public observation adapters supply untrusted bytes and typed metadata. Verter computes or verifies fingerprints at the capture boundary; only sealed first-party snapshot authorities may mint trusted basis material.

## V10-B6 — Late waiter and budget semantics were incomplete

The released flight contract did not fully close whether a waiter could join after the producer had entered an irreversible terminal path or how multiple waiter budgets combine without changing semantic completeness.

**Revision 11 correction:** only `Running` accepts waiters. Terminal transition enters `Finalizing`; later requests use a successor flight. Effective producer budget is a bounded monotonic maximum while continuation remains possible, never a sum or a semantic approximation selector.

## V10-B7 — Revision 10 did not have one reproducible published authority tree

The distributed ZIP was internally self-consistent, but it represented an older tree than the later consolidated/source package published under the same Revision 10 name. Roughly thirty package files differed, and the distributed ZIP omitted the later validator tools. A separate local `v10-final` tree also had a stale manifest. The problem was not one corrupt ZIP; it was that “Revision 10” did not identify one singular reproducible authority.

**Revision 11 correction:** one canonical source tree includes package, performance-gate, program-state, and stack-window validators plus one release builder. It generates and revalidates the consolidated document and deterministic ZIP from the same manifest-listed source.

## V10-B8 — Consolidated and split authority had no reproducible coupling

The master Markdown and split package could drift because no source order or generator was part of the authority package.

**Revision 11 correction:** the split package is explicitly canonical. `consolidation-order.txt` and `tools/build_consolidated.py` generate the reading copy; it is never independently edited.

## V10-B9 — Maintainer and orchestrator authority were conflated

Revision 10 used “maintainer/orchestrator” as one role. An agent could interpret that as permission to scope, implement, review, change gates, accept, and merge its own work.

**Revision 11 correction:** the maintainer and orchestrator are separate. The orchestrator coordinates and recommends; only the designated maintainer adopts architecture, accepts A6/amendments/gate changes, formally rescopes, and authorizes landing where required.

## V10-B10 — No direct orchestrator bootstrap or first-run stop rule

A large package is not itself an executable prompt. Revision 10 did not say which files to read first, which commands to run, what permissions to inspect, what the first run may change, or when to stop.

**Revision 11 correction:** `ORCHESTRATOR.md` is the package entry point and authorizes A0 only. It defines trust bootstrap, repository inspection, program-state initialization, delegation limits, stop conditions, and an exact output record. `agents/opus-bootstrap.md` is ready to paste into an Opus orchestrator.

## V10-B11 — No durable machine-readable execution state

The DAG described legal dependencies, but there was no canonical ledger for READY/IN_PROGRESS/REVIEW/ACCEPTED state, exact candidates, evidence digests, stack placement, or review status. Conversation history and PR labels could silently diverge.

**Revision 11 correction:** `program-state.toml` contains every DAG block exactly once and is validated on every state transition. Before A6 it prevents all post-Gate-0 work; after A6 it enforces predecessor acceptance and bounded concurrency.

## V10-B12 — No worker context or worktree ownership contract

Subagents could receive the whole plan, widen scope, share a mutable checkout, or overwrite generated/central files.

**Revision 11 correction:** every worker receives one immutable digest-addressed context packet, one role, one allowed write set, and one writable worktree/branch. Shared generated/protocol/lockfile surfaces have one writer lease.

## V10-B13 — Stacked PRs were not operationally defined

Revision 10 allowed intermediate branch work but did not define stack size, DAG/stack authority, mergeability, sibling tracks, lower-layer changes, CI, review invalidation, merge queue behavior, or final tree equivalence.

**Revision 11 correction:** stacks are bounded windows over short dependency paths, never a program-wide chain. Each mergeable layer is independently releasable and proven on the cumulative tree. Sibling DAG branches normally use separate stacks.

## V10-B14 — Atomic cutovers needed a stack-specific landing rule

`D1` was private and `D2` atomic in prose, but an ordinary stacked PR tool could still make the private layer independently mergeable.

**Revision 11 correction:** private review layers target a private integration branch, remain draft/non-mergeable, and reach trunk only through the final atomic candidate. D1/D2 is the canonical pattern; internal layers of B4/B5/D2 or another atomic block follow the same rule.

## V10-B15 — Restacking was incompatible with unqualified exact-SHA approval

A lower-layer edit cascades new SHAs through the stack. Revision 10 said approval never transfers across a SHA change but did not define range-diff, cumulative-tree CI, or bounded reattestation. The result would be either unsafe approval preservation or impractical full re-review.

**Revision 11 correction:** every restack records base/tree, patch/range-diff, manual conflicts, generated changes, and evidence. All affected approvals become `REVALIDATION_REQUIRED`; required CI reruns; each mandate performs impact-bounded reattestation on the new exact candidate. No approval transfers automatically.

## V10-B16 — Reviewed candidate identity and landing identity were conflated

A merge queue, merge commit, or reviewed rebase can produce a landing commit SHA different from the candidate SHA that reviewers inspected. Requiring them to be literally identical is operationally brittle; silently treating the landing SHA as reviewed is unsound.

**Revision 11 correction:** program state records the reviewed base/candidate SHA/tree separately from the accepted base/commit/tree. Landing is legal only when a repository-validated, digest-addressed proof shows that the canonical binary candidate delta and generated-output digest are identical on the recorded reviewed and accepted bases, no manual conflict resolution occurred after review, and required post-landing checks pass. Full-tree equality is not assumed after a legitimate base advance.

## V10-B17 — Accepted-only predecessor state made real stacked review impossible

Revision 10 and the first Revision 11 draft said no block could begin until every predecessor was accepted, while also recommending dependent stacked PRs. Under that rule, an upper layer could not be implemented or reviewed until the lower layer had already landed, reducing the “stack” to a sequence of ordinary PRs and contradicting the delivery contract.

**Revision 11 correction:** a block may remain contingent at `READY`, `IN_PROGRESS`, or `REVIEW` when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the actual accepted base. The program-state and stack-window validators enforce both sides of this rule.

## V10-B18 — Atomic review layers and acceptance blocks were not represented independently

An atomic block can require several private review-sized layers that all belong to the same acceptance unit. A schema keyed only by unique block ID cannot represent those layers without inventing fake program blocks or overwriting state. It also cannot correctly represent the special D1 private checkpoint plus D2 final acceptance.

**Revision 11 correction:** every stack layer has a unique `layer_id`, while `block_id` identifies its program acceptance unit. `ATOMIC_REVIEW` permits repeated private layers for the same acceptance block and an explicit `foundational-private-checkpoint` predecessor, but exactly one final layer is mergeable. Program state stores only the final/current layer for the acceptance block; D1 retains its explicit private-checkpoint state.

## V10-B19 — Stack snapshot and mutable program state could drift

A stack file that referenced the mutable current program-state digest while the current program state also referenced the stack would create a hash cycle. Omitting one side would allow branch/PR state and the durable ledger to diverge silently.

**Revision 11 correction:** every stack window binds one immutable pre-stack `program_state_basis_digest`; current program state then binds the resulting StackSnapshotId. `ACTIVE`, `FROZEN_FOR_REVIEW`, and `LAND_READY` validation cross-checks both records. Restacks chain through `previous_stack_snapshot_digest`.

## V10-B20 — The first landing proof had a post-state hash cycle and a false full-tree premise

A landing proof cannot contain the digest of the post-landing program state when that state itself stores the landing-proof digest. Requiring the accepted full repository tree to equal the reviewed candidate tree is also wrong after a legitimate target-base advance, even when the candidate change survived exactly.

**Revision 11 correction:** the proof binds the pre-landing program-state digest only; the post-landing state stores the validated proof digest. Repository verification compares the canonical binary Git delta from reviewed base to reviewed candidate with the delta from accepted base to accepted commit, plus generated-output digests and post-landing checks. Any changed delta or post-review manual conflict requires a new candidate and review.

# 3. Architecture judgment

No core product-architecture reversal was justified by this review. Revision 11 preserves the Revision 10 target and strengthens its execution boundary.

The architecture is best described as:

> the strongest presently justified design, with explicit empirical falsification points—not a claim of mathematically provable global optimality before implementation.

That distinction is essential. A system that refuses to update after contrary source or benchmark evidence would not be “no compromise”; it would be dogmatic.

# 4. Stacked PR judgment

Stacked PRs are a good fit for this program when used selectively:

- Gate 0 remains sequential under its own lock-building rules; after A6, short dependency paths can be reviewed in bounded layers;
- direct compiler and contract work benefits from small cumulative diffs;
- parallel DAG siblings should remain separate stacks;
- atomic cutovers use private review layers and one mergeable final candidate;
- no stack should span the entire fifty-block program.

The stack tool is replaceable. Correctness depends on the Revision 11 contract, not a particular CLI or GitHub UI.

# 5. Opus handoff judgment

Revision 10 should not be sent to an Opus orchestrator with only “implement this.” Revision 11 may be sent directly when:

- the split ZIP is supplied;
- `ORCHESTRATOR.md`/`agents/opus-bootstrap.md` is used;
- an actual local checkout and required tools are available;
- a human maintainer is designated;
- the first run executes A0 only;
- independent review mandates are not collapsed into the orchestrator's self-assessment.

# 6. Readiness decision

Revision 11 is **ready for direct orchestrator handoff and implementation entry at A0**.

It is not honest to label all later blocks pre-authorized. They become implementation-ready only through the accepted A6 lock and per-block readiness state. That gate is part of a complete architecture, not an admission that the plan is unfinished.

# 7. Review limitation

This review validates the supplied architecture artifacts, their internal program, current public repository state relevant to entry, and current agent/stack workflow requirements. It does not claim that Verter's unimplemented final architecture has already passed its Rust/TypeScript/NAPI/WASM suites, TypeScript differential corpus, provider matrix, benchmarks, or multi-week soak. Revision 11 requires those proofs on the actual implementation candidates.
