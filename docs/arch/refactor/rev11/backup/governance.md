# Verter Revision 11 Program Governance

**Status:** Normative execution/review process.  
**Not production architecture:** this document governs how the convergence program is scoped and proven. It must not leak into runtime APIs or source comments.

**Sequencing authority:** no block may begin before every direct predecessor in `program-dag.toml` is accepted, except contingent `READY`/`IN_PROGRESS`/`REVIEW` work on an upper layer whose unaccepted predecessor is below it in the same validated immutable stack snapshot. Such work cannot be acceptance-recommended or accepted until the predecessor lands and the upper candidate is restacked/revalidated. Document order, available implementor capacity, or a convenient branch is not permission.

**Gate authority:** the A6 Implementation Lock Record freezes numeric and compatibility decisions before non-safety cutovers. Candidate measurements cannot be used to choose their own pass criteria.

# 1. Roles

## 1.1 Maintainer

The maintainer is the designated human or explicit repository authority. It alone may:

- adopt or supersede the architecture package;
- accept `A6`, architecture/ADR amendments, formal rescopes, and gate recalibrations;
- approve irreversible public compatibility/protocol decisions;
- accept/land/merge when repository policy requires maintainer authority.

The maintainer remains accountable for evidence quality. An orchestrator or collection of model approvals cannot self-create repository authority.

## 1.2 Orchestrator

Owns execution coordination under `contracts/agent-orchestration.md`:

- entry-checkout, implementation-baseline, candidate SHA/tree, program-state, and stack records;
- block classification proposals, predecessor verification, and legal sequencing;
- authority/contract/accepted-ADR consistency checks;
- immutable context/evidence packets and writable worktree assignment;
- integration/restack conflicts and proof that commands executed non-vacuous work;
- review convergence, discovery triage, and deviation/rescope recommendations;
- maintaining exact evidence and review state for the maintainer decision.

The orchestrator may implement when assigned, but cannot count its own synthesis as independent review, cannot self-accept a block, and cannot silently amend architecture or gates.

## 1.3 Architecture challenger

For Foundational work, independently challenges the proposed target before implementation using product requirements, repository facts, measurements, compatibility obligations, and blast radius. It should inspect before reading the implementor's preferred rationale where practical.

## 1.4 Scoper

Enumerates:

- current authorities/readers/writers/callers;
- lifetime/thread/platform boundaries;
- identities, caches, compatibility domains, persistence, and memory consequences;
- cutover closure and deletion set;
- proof gates and exclusions;
- review scope cone and abort conditions.

## 1.5 Implementor

Executes the ratified charter. It may choose private representation details but may not reopen locked ownership, add a second production path, widen scope silently, or weaken proof. A materially false charter assumption produces a deviation memo and stop.

## 1.6 Review mandates

Foundational work has three distinct evidence mandates:

1. **Conformance:** exact charter, required behavior/failure, deletion, and non-vacuous proof.
2. **Architecture:** authority, identity, ownership, dependency direction, lifetime, platform, compatibility, public boundary, determinism, and conceptual complexity.
3. **Adversarial performance/memory:** races, cancellation, stale state, budgets, pressure, deep input, long churn, task grain, work amplification, latency tails, copies, allocations, and failure containment.

A person/tool/context may fill different mandates on different blocks, but one context must not scope, implement, and provide the only substantive approval for the same non-local block.

Independence is established by distinct mandate, clean context, exact SHA/tree, direct evidence access, and freedom to return `NOT PROVEN`—not by product/model brand.

# 2. Block classification

## 2.1 Local

Examples: obsolete comment/guard/doc cleanup, mechanical re-export/dependency removal, isolated internal rename with no runtime/semantic/lifetime consequence.

- implementor may author charter;
- one independent bounded review;
- promote if owner, semantics, lifetime, compatibility, platform, public API, hot path, retained state, or broad blast radius changes.

## 2.2 Subsystem

Examples: one cache family, CSS artifact convergence, one framework capability, bounded API cutover, one persistence domain.

- independent scoper;
- conformance plus adversarial review on one exact SHA/tree;
- architecture review added when authority/lifetime risk warrants it.

## 2.3 Foundational

Examples:

- A6 Implementation Lock Record and gate changes;
- input/snapshot authority;
- typed identity hierarchy;
- direct compiler foundation;
- parse ownership/affinity;
- `CompileTypeInfo`/TypeInfo kernel;
- flow/effective typing;
- semantic graph lifetime;
- QueryRuntime/shared flights/executor;
- incremental invalidation/publication;
- ProviderHub route/epoch correctness;
- host/Engine authority;
- persistence architecture;
- unsafe or intentional auto-trait changes;
- native/WASM execution-profile semantics.

Requires independent scoping, pre-implementation architecture challenge, and all three review mandates on one exact candidate SHA/tree.

Classification guidance: mark whether a block changes authority, semantics, lifetime/concurrency/platform, public/cross-crate/wire contract, hot/retained path, or broad blast radius. Three or more normally means Foundational, but named foundational areas remain Foundational unless the charter proves a strictly mechanical change.

# 3. Mandatory block charter

Every block begins with a concise expiring charter:

```text
Block title:
Review class: Local | Subsystem | Foundational
Frozen implementation baseline SHA:
Authority package digest / A6 lock ID:
Direct predecessors and accepted SHAs:
Target branch/candidate SHA/tree:
Program-state digest:
Context-packet digest:
Stack window/layer and mergeability:
Writable worktree/worker:

Owner/invariant:
Product operation(s):
Semantic profile(s)/execution profile(s):
Current authorities/readers/writers/callers:
Current identity/compatibility/persistence basis:
Applicable locked performance/memory cells and immutable thresholds:
Consumers in scope:
Explicit exclusions:
Final ownership/dependency/lifetime direction:
Determinism and hermeticity contract:
Rejected alternatives:
APIs/modules/caches/tasks/guards/comments/docs to delete:
Correctness/conformance/failure proof:
Incremental/stale-publication proof:
Work/copy/allocation/performance/memory/soak gates:
Native/WASM/FFI implications:
Private construction/testing boundary before cutover:
Clean-cutover sequence:
Review mandates and reviewers/contexts:
Review scope cone:
Causal-blocker criteria specific to this block:
Abort/rescope conditions:
```

The charter states what must become true and what may change. It does not dictate every private implementation detail. It may not remove a predecessor edge, loosen an A6 gate, create a second production authority, or reinterpret an accepted ADR.

After landing, the charter is deleted or archived as historical evidence. Production code does not refer to it.

# 4. Predecessor and gate enforcement

Before a charter is ratified, the orchestrator verifies:

- every direct predecessor from `program-dag.toml` is accepted on the intended integration lineage, or contingent upper-layer work is bound to an exact lower predecessor in the same validated stack snapshot;
- predecessor contracts and generated identities/protocols are present in the candidate base;
- no later branch contains an unreviewed conflicting authority;
- all applicable A6 numeric cells and compatibility dispositions are populated;
- the block will not measure against or depend on an implementation that its own cutover deletes;
- parallel unlocked blocks have disjoint or explicitly coordinated cutover closures.

An edge may be added when reality reveals a dependency. Removing an edge requires an authority amendment, a clean architecture challenge, and re-ratification of every already-open affected charter.

## 4.1 Gate recalibration

A locked threshold/corpus/machine class may change only when the environment or benchmark definition changed materially. Recalibration requires:

1. a written cause unrelated to candidate direction;
2. retained old/new calibration data;
3. a reviewer who has not inspected the candidate result direction where practical;
4. amendment of the Implementation Lock Record;
5. invalidation and rerun of affected candidate evidence.

A threshold is never relaxed because a candidate missed it. A benchmark bug is fixed and both baseline and candidate are rerun.

# 5. Program state, worktrees, and stacked delivery

`program-state.toml` is the durable execution ledger. Conversation history, a project board, or PR labels may mirror it but cannot replace it. The orchestrator is its sole writer; the maintainer accepts state transitions that require authority. `tools/validate_program_state.py` must pass after every transition and before a block starts, enters review, is recommended for acceptance, or is accepted.

Every worker receives one immutable context packet and one writable worktree/branch. No two workers share a mutable checkout. Shared generated files, lockfiles, protocol schemas, dependency firewalls, and central manifests have one active writer lease.

Stacked PRs follow `contracts/stacked-prs.md`:

- the program DAG remains semantic authority;
- only bounded stack windows are allowed;
- every mergeable layer is independently safe and proven on its cumulative tree;
- private atomic layers are draft/non-mergeable and land only through the final atomic candidate;
- lower-layer changes cascade restack, CI, state, and review invalidation;
- no approval transfers automatically to a new SHA/tree;
- a program-wide mega-stack is prohibited.

The orchestrator records old/new base/tree, patch/range-diff, evidence digest, conflicts, CI reruns, and reviewer reattestation after every restack.

# 6. Scope cone

Review scope has four tiers:

1. **Changed surface — mandatory:** changed/deleted/replacement code, tests, touched public/wire APIs, promised deletion set.
2. **Required closure — mandatory:** direct callers/readers/writers, owner lifetime, identity/validity, cache/persistence, platform/concurrency/publication, semantic dependencies needed to prove the charter.
3. **Investigative context — allowed:** any area needed to prove/disprove an in-scope concern.
4. **Unrelated repository quality — non-blocking:** pre-existing unrelated defects, later-program work, alternate preferences, opportunistic cleanup.

Inspection breadth does not automatically expand blocking scope. Only the maintainer authorizes formal rescope; the orchestrator records and recommends it.

# 7. Causal blocker rule

A finding blocks the current candidate only when it establishes at least one:

1. **Candidate introduction:** candidate introduced the defect.
2. **Candidate amplification:** candidate materially worsens or newly exposes a pre-existing defect.
3. **Charter/architecture/proof violation:** explicit invariant, exclusion, deletion, determinism, correctness, failure, performance, memory, compatibility, platform, or verification requirement is violated.
4. **Incomplete cutover:** claimed final owner/path/lifetime/compatibility model is not complete.
5. **Adjacent correctness exception:** a pre-existing consumer/owner makes the candidate's new invariant unsafe and therefore belongs to the true cutover closure.

A blocker records:

```text
Finding:
Severity:
Candidate cause:
Authority/charter requirement violated:
Affected behavior/invariant:
Evidence/reproduction:
Minimum correction condition:
```

The reviewer specifies the minimum condition, not an unrelated redesign.

If an adjacent issue means the charter missed the true cutover closure, implementation stops and returns to scoping. That is scope correction, not reviewer-driven scope expansion.

# 8. Review result contract

Each mandate returns only:

```text
PASS
BLOCKING FINDINGS
NOT PROVEN
NON-BLOCKING DISCOVERIES
```

`NOT PROVEN` blocks when the missing evidence is an explicit architecture/charter/verification condition. Otherwise the maintainer records and dispositions the gap.

Reviewers record findings before editing code. The implementor normally applies bounded fixes. A reviewer acting as fixer does not count its own patch as independent re-approval.

# 9. Exact-SHA convergence

Approval is attached to one exact candidate SHA **and tree**, its exact base tree, and the evidence digest. Any code/test/generated-output change or restack creates a new candidate and invalidates landing approval until impact-bounded reattestation binds to the new identity.

Recheck is impact-bounded:

- prior blockers;
- fix delta;
- newly modified code;
- directly affected invariants;
- required regression/performance evidence.

A full fresh review is required when the fix materially changes architecture, authority, identity, compatibility, lifetime, platform behavior, or real blast radius.

Foundational state:

```text
independent scope
-> blind architecture challenge
-> ratified charter
-> implementation
-> Candidate A
-> conformance review
-> architecture review
-> adversarial performance/memory review
-> bounded fixes
-> Candidate B
-> impact-bounded recheck by all three mandates
-> all three PASS on Candidate B
-> maintainer acceptance/land
```

Three generic approvals do not satisfy the three distinct mandates.

The **reviewed candidate identity** and the **accepted landing identity** are separate records. A merge commit, merge-queue commit, squash, or one-commit fast-forward may produce a different accepted commit SHA and full tree because the landing base advanced or the reviewed branch was reconstructed before final freeze. That is legal only when a repository-validated landing-equivalence artifact proves exact canonical candidate-delta equality from reviewed base to reviewed candidate versus accepted base to accepted commit, generated-output digests match, no manual conflict resolution occurred after review, and all required post-landing checks pass. A single program block is not co-batched with unrelated changes in the same landing delta. The accepted identity never replaces or retroactively changes the SHA/tree reviewers actually inspected.

# 10. Architecture deviation

When source or measurement disproves a charter assumption, the implementor stops and records:

```text
Failed assumption:
Measured/source evidence:
Affected architecture/verification invariants:
Compatibility or consumer consequences:
Alternatives:
Recommended amendment:
Work that remains valid:
```

The maintainer decides whether to amend the ADR/architecture/charter, split the block, or abort. A deviation cannot be hidden behind a flag, shim, parallel path, new cache, broader trait, or weaker test.

# 11. Non-blocking discovery queue

Useful out-of-scope findings are recorded as:

- `DISC-CORRECTNESS` likely correctness/security bug;
- `DISC-ARCH` architecture cleanup;
- `DISC-PERF` performance/memory opportunity;
- `DISC-DOC` documentation/cleanup;
- `DISC-INVESTIGATE` investigate later.

The finding states why it is outside the current causal boundary. The maintainer dispositions it into an existing future block, a new bounded block, urgent interruption, ordinary issue/backlog, or rejection.

The review artifact is not a permanent campaign database. Discoveries are moved to normal project tracking and cleared after the block lands.

# 12. Clean-cutover governance

An accepted cutover must prove:

- all declared predecessors were accepted before implementation began, or any contingent stacked work remained at draft/review state and was restacked/revalidated after predecessor landing before acceptance;

- one surviving production implementation;
- every in-scope caller migrated;
- old declaration/implementation/cache/task/metric/flag/dependency/re-export/wire field/fixture removed or explicitly retained under a real compatibility obligation;
- compiler/build errors used as caller inventory;
- no runtime switch, shadow evaluator, parser fork, alternate resolver, or duplicate cache-correctness authority;
- no migration-only scanner/allowlist/guard;
- historical comments cleaned;
- API/auto-trait/compatibility changes documented;
- behavior, work, performance, memory, and failure evidence executed;
- negative net conceptual complexity unless explicitly ratified.

Intermediate branch commits may be broken. Private replacement machinery may exist on the implementation branch and be exercised by hermetic tests. It must remain unreachable from production entry points until the atomic cutover, and it is deleted or becomes the sole path in the accepted merge. The accepted merge is not releasable until the cutover is complete.

`D1` is the sole exception to the ordinary predecessor-integration rule: it is a reviewed private checkpoint on the bounded `D2` branch and cannot land independently. The `D2` sole-flow-solver cutover is indivisible: routing all public consumers and deleting the old evaluator occur in one accepted candidate. Later semantic coverage is added only inside the surviving solver.

# 13. Evidence independence

Independent evidence requires:

- clean or intentionally bounded context;
- distinct mandate;
- exact baseline and candidate SHA;
- direct access to diff, source, tests, benchmarks, profiles, and raw outputs;
- explicit scope cone and causal-blocker rule;
- permission to challenge plan assumptions;
- permission to return `NOT PROVEN`;
- no reliance solely on implementor summary.

Multiple automated/model instances with identical prompt/context and no independent inspection are not automatically independent. A strong human or automated reviewer may satisfy a mandate when the evidence discipline is met.

# 14. Documentation and comment governance

Durable production comments explain current invariants, safety/memory rationale, concurrency/ordering, observable compatibility, or why a simpler implementation is incorrect.

Disallowed final-state comments include:

- program track/block/phase instructions;
- model/reviewer assignments;
- branch/SHA/line-number migration notes;
- links to expiring plans as the only explanation;
- “landed/deferred/follow-up” history for work expected to disappear;
- obsolete legacy descriptions.

A one-time inventory script may assist cleanup but is deleted after use. Ordinary durable documentation link checking may remain.

# 15. Final maintainer decision

Review convergence is necessary but not sufficient. The maintainer confirms from the orchestrator's exact evidence record:

- architecture/ADR consistency;
- exact-SHA evidence completeness;
- unresolved discoveries and compatibility obligations dispositioned;
- no hidden deviations or unreviewed generated changes;
- predecessor DAG, validated program state, stack-window, and A6 gate integrity preserved;
- clean cutover/deletion complete and target tree equivalent to the reviewed candidate;
- release/product maturity statements updated.

Only then is the candidate accepted.
