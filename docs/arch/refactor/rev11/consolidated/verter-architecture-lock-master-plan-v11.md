# Verter Architecture Lock and Tooling Convergence Master Plan

## Revision 11 — orchestrator-ready implementation lock and bounded stacked delivery

**Status:** Ratified implementation-entry and orchestrator-handoff authority; ready to execute A0.  
**Canonical authority:** the split package, not this generated reading copy.  
**Split-package digest:** `af11392f5f9eeea75cbd82def85adadfee41b3c8032b5248c09e96aba13123a7`.


---

# Consolidated source: `README.md`

# Verter Architecture Lock — Revision 11 Authority Manifest

**Status:** Ratified implementation-entry and orchestrator-handoff authority.  
**Supersedes:** Revision 10 and every earlier architecture-lock plan.  
**Breaking changes:** allowed and expected where they improve the final architecture.  
**Canonical artifact:** the split package and its `MANIFEST.json`; the consolidated master file is generated reading convenience only.  
**Implementation readiness:** ready to hand to an orchestrator and execute `A0`; all later work remains governed by the DAG, `A6`, block charters, proof, and maintainer acceptance.  
**Historical evidence baseline:** `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0`; this is not a claim about the current implementation checkout.  
**Entry checkout:** one exact local SHA/tree captured by `A0`; never a floating branch, remote page, or “architecture-equivalent successor.”  
**Implementation baseline:** one exact post-harness, post-safety, post-instrumentation SHA/tree accepted by `A6`; affected Gate 0 evidence is refreshed after every lineage change.

# 1. Honest verdict

Revision 11 is the strongest architecture and implementation program justified by the supplied plans, frozen-source evidence, and the current design goals:

- maximum direct/compiler performance through least work and owner locality;
- one semantic authority with deterministic derived projections;
- correct project-aware TypeInfo without alternate resolver semantics;
- bounded long-running managed memory;
- clean Vue/Svelte and future-framework boundaries;
- breaking clean cutovers rather than compatibility accretion;
- deterministic, proof-carrying, fail-closed results;
- measurable equivalent-work performance and memory gates.

It is not intellectually honest to call any unimplemented architecture “proven globally optimal.” `A0`–`A6`, the block proofs, and `L1`–`L4` exist precisely so repository facts and measurements can falsify and improve the plan. Revision 11 is **implementation-entry complete**, not pre-certified implementation success.

# 2. Why Revision 10 required another revision

The released Revision 10 artifacts had a strong core but were not safe for direct autonomous execution:

1. the entry checkout and post-Gate-0 implementation baseline were conflated;
2. measurement could precede the safety retraction that changed measured behavior;
3. the borrowed direct compiler could land before compact units and blanking deletion;
4. product-local profiles and reusable semantic/product/terminal subplans were not fully closed;
5. trust boundaries for caller-supplied observation digests and late flight waiters/budgets needed tightening;
6. different Revision 10 source, consolidated, and ZIP trees were published under one revision, and the distributed ZIP did not contain the later validators;
7. the consolidated and split artifacts had no reproducible generation contract;
8. maintainer authority and orchestrator execution were conflated;
9. reviewed candidate identity and post-merge landing identity were not represented separately;
10. there was no safe Opus/agent bootstrap, durable program ledger, context-packet contract, worktree ownership rule, or stacked-PR/restack protocol.

Revision 11 incorporates the corrected Gate 0/direct-core contracts and closes the orchestration and delivery gaps. See `implementation-readiness-review-v10.md`.

# 3. How to start

Do **not** send only the consolidated master plan with an instruction to implement the entire program.

Give the orchestrator:

1. the extracted Revision 11 split package;
2. an actual local Verter checkout and the required repository/GitHub tools;
3. the designated human maintainer identity;
4. `ORCHESTRATOR.md` as the first instruction.

For Claude Opus, `agents/opus-bootstrap.md` is a ready-to-paste convenience prompt; the release builder also exports the byte-identical `verter-opus-orchestrator-prompt-v11.md`. The first run executes only `A0`.

# 4. Readiness states

```text
PACKAGE_RATIFIED
  -> ENTRY_LOCKED
  -> PROGRAM_LOCKED
  -> BLOCK_READY
  -> LAND_READY
  -> PROGRAM_COMPLETE
```

## 4.1 `PACKAGE_RATIFIED`

The authority package validates and may be handed to an orchestrator. Only `A0` is initially authorized.

## 4.2 `ENTRY_LOCKED`

`A0` has recorded the exact checkout/tree, repository state, package digest, open-work disposition, GitHub/CI/stack capabilities, maintainer/orchestrator identities, requested/actual model, orchestrator runtime/version, and initial validated program ledger.

## 4.3 `PROGRAM_LOCKED`

`A6` has accepted one immutable Implementation Lock Record containing:

- entry and implementation baseline SHA/tree;
- Revision 11 package, DAG, and program-state digests;
- non-vacuous command and capability evidence;
- current-owner and open-change dispositions;
- identity/profile/compatibility/protocol decisions;
- instrumentation baseline;
- numeric machine-readable performance gates;
- orchestration, worktree, CI, review, and bounded stack-window policy;
- first unlocked charters and review assignments.

Only then may post-Gate-0 foundational work begin.

## 4.4 `BLOCK_READY`

One bounded block has every predecessor formally accepted, or—only for contingent stacked draft/review work—bound as a lower layer in the same validated immutable stack snapshot. It also has an exact charter/context packet, current-tree closure, discriminating tests, numeric gates, branch/worktree/stack placement, deletion set, and independent review mandates. A contingent upper layer cannot become acceptance-recommended until its predecessors land and the candidate is restacked/revalidated.

## 4.5 `LAND_READY`

One exact reviewed candidate SHA/tree satisfies its charter, proof, deletions, cumulative-stack CI, and required review set, and every semantic predecessor is formally satisfied. Approval never transfers automatically across a code change or restack. After landing, program state records the actual accepted commit/tree and a validated landing-equivalence digest proving that the exact reviewed candidate delta survived on the recorded landing base; neither commit-SHA equality nor full-tree equality is assumed.

## 4.6 `PROGRAM_COMPLETE`

`L4` accepts the final architecture after conformance, equivalent-work performance, deterministic output, bounded-memory soak, deleted legacy paths, and final documentation lock.

# 5. Authority map

| Authority | File |
|---|---|
| Orchestrator bootstrap and first-run rule | `ORCHESTRATOR.md` |
| Human Opus launch guide and paste-ready adapter | `OPUS-START-HERE.md`, `agents/opus-bootstrap.md` |
| Optional Claude Code role adapters (non-normative) | `agents/claude-code/README.md`, `agents/claude-code/*.md` |
| Durable product, ownership, identity, and lifetime architecture | `architecture.md` |
| Exact predecessor graph and clean-cutover program | `program.md`, `program-dag.toml` |
| Correctness, determinism, performance, work, memory, failure, and delivery proof | `verification.md` |
| Maintainer/agent roles, review, exact-candidate convergence, and rescope | `governance.md` |
| Agent trust bootstrap, actual-runtime record, delegation, permissions, stop, and output contract | `contracts/agent-orchestration.md` |
| Bounded stack windows, worktrees, restacks, CI, and atomic landings | `contracts/stacked-prs.md` |
| Constitutional invariants, evidence-selected tactics, and falsification | `contracts/architecture-falsification.md` |
| Reproducible single-source release publication | `contracts/package-publication.md` |
| Exact implementation checkout/baseline lock | `contracts/baseline-lock.md` |
| Existing-owner reconciliation | `contracts/current-tree-reconciliation.md` |
| Product capability and maturity truth | `contracts/capability-matrix.md` |
| Semantic/output/presentation/serialization profile classification | `contracts/semantic-profile.md` |
| Canonical identity encoding and digest rules | `contracts/identity-encoding.md` |
| Source-unit and mapping taxonomy | `contracts/mapping-products.md` |
| Parse key, owner domain, affinity, and reparse | `contracts/parse-ownership.md` |
| Staged compile transaction and anti-replay | `contracts/compile-transaction.md` |
| Snapshot-consistent input loading | `contracts/input-loading.md` |
| Result contracts and flight state machine | `contracts/result-contract-and-flight.md` |
| Flow domain closure, obligations, and complete-result construction | `contracts/flow-completeness.md` |
| Deterministic ordering and stable identifiers | `contracts/deterministic-ordering.md` |
| Performance gate, Implementation Lock, program-state, premise, stack, and landing-equivalence schemas | `templates/performance-gates.template.toml`, `templates/implementation-lock-record.md`, `templates/program-state.template.toml`, `templates/architecture-premise-ledger.template.md`, `templates/stack-window.template.toml`, `templates/landing-equivalence.template.toml` |
| Block, worker, PR, and review handoff schemas | `templates/block-charter.md`, `templates/context-packet.md`, `templates/pr-description.md`, `templates/review-report.md` |
| Prepared Gate 0 charters | `charters/A0.md` through `charters/A6.md` |
| First post-lock charter templates | `charters/B1.template.md`, `charters/J1.template.md` |
| Package, performance-gate, program-state, stack-window, and landing-equivalence validators plus live orchestration self-tests | `tools/validate_package.py`, `tools/validate_performance_gates.py`, `tools/validate_program_state.py`, `tools/validate_stack_window.py`, `tools/validate_landing_equivalence.py`, `tools/selftest_orchestration.py` |
| Reproducible release and low-level builders | `tools/build_release.py`, `tools/build_consolidated.py`, `tools/build_deterministic_zip.py`, `consolidation-order.txt` |
| Durable decisions | `decisions/ADR-*.md` |
| Revision 10 and Revision 9 readiness findings | `implementation-readiness-review-v10.md`, `implementation-readiness-review-v9.md` |
| Historical source evidence for `9af553dd…` | `baseline/9af553dd.md` |

## 5.1 Precedence

1. `architecture.md` is the consolidated current durable product architecture; accepted ADRs constrain scoped decisions.
2. Exact contracts define cross-owner behavior where general prose could permit incompatible implementations.
3. `verification.md` defines proof.
4. `program-dag.toml` and `program.md` define legal semantic ordering and cutover sequence.
5. `governance.md` and the orchestration/stack contracts define execution, review, and landing authority.
6. A block charter/context packet narrows work but cannot weaken a higher authority.
7. A PR stack is transport only; it never overrides the DAG.
8. Baseline records are evidence, never permanent architecture.

A source fact or measurement that disproves this package stops implementation and triggers a reviewed amendment. It does not authorize a hidden shim, weaker gate, parallel owner, or local reinterpretation.

# 6. Governing principles

- **Least work:** execute the smallest prerequisite closure that can prove the requested product.
- **Exact live reuse:** do not repeat an exact computation inside one live owner domain unless measurement proves recomputation cheaper.
- **One authority, justified projections:** derived indexes may accelerate one authority but cannot invent meaning.
- **Direct core first:** prove borrowed one-shot/prepared execution before managed reuse is generalized.
- **Determinism:** equal authoritative inputs and product/profile contracts produce equal observable results across legal schedules and supported execution profiles.
- **Honest completeness:** unsupported, partial, stale, cancelled, budget-exhausted, panicked, or failed work cannot become complete warm state.
- **Thread affinity:** move compact owned requests/results, not OXC arenas or local mutable semantic state.
- **Terminal materialization:** render, encode, map, serialize, and build FFI objects only when requested.
- **Bounded lifetime:** every retained owner has byte bounds, pressure behavior, pin provenance, shutdown, and quiescent plateau proof.
- **Clean cutover:** one accepted merge contains one production implementation and deletes the displaced path/support machinery.
- **Bounded orchestration:** one legal block, one owner, one context packet, one program ledger, and independent review evidence.
- **Bounded stacks:** short review windows only; atomic private layers never merge independently.

# 7. Final target shape

```text
borrowed deterministic direct compiler
+ explicit single-owner PreparedCarrier
+ compositional product-local requests and independently reusable subplans
+ shared syntax frontends with explicit ParseOwnerDomainId
+ required SourceProjectionMap distinct from optional RuntimeSourceMapData
+ staged prepare/plan/project/emit transaction with anti-replay tokens
+ one TypeInfo/module-resolution/relation/inference semantics path
+ one FunctionFlowGraph authority and deterministic derived projections
+ demand-selected closed flow domains with an obligation ledger
+ one atomic sole-solver flow cutover
+ operation-specific public DTOs and optional bounded graph export
+ one coherent committed input/snapshot authority
+ snapshot-independent QueryIdentity candidate lookup plus exact-basis SemanticFlightKey
+ ResultContractId separate from waiter-local ExecutionPolicy
+ FlightCell-owned production and exact snapshot-scoped semantic flights
+ bounded structured CPU work plus explicit owner-affine commands
+ immutable stable-unit managed incrementality
+ StyleSyntaxIr reused per exact live style identity
+ typed framework capability boundaries
+ atomic generated code and required mapping publication
+ terminal rendering, runtime maps, provenance, serialization, and FFI
+ consumer-driven persistence only
+ exact-candidate proof, aggressive equivalent-work gates, and long-running plateau
+ model-neutral orchestration with human acceptance authority
+ bounded stack windows and atomic landing groups
```

# 8. What Revision 11 does not claim

Revision 11 does not claim that Verter already passes its compiler corpus, TypeScript differential suite, direct/prepared/managed benchmarks, NAPI/WASM equivalence, provider matrix, or multi-week memory soak. It does not claim that every future source fact has been predicted. It makes those uncertainties explicit, bounded, and incapable of being converted into optimistic implementation claims.

# 9. Document lifecycle

During convergence this split package is normative. The consolidated master is generated from `consolidation-order.txt` and is never edited independently. After `L4`, durable content is reduced to concise architecture documentation and retained ADRs; program files, baseline evidence, charters, agent adapters, and campaign-only material are archived or deleted after unique evidence is preserved.


---

# Consolidated source: `OPUS-START-HERE.md`

# Start Verter Revision 11 with Claude Opus 5

Revision 11 is safe to hand to an Opus orchestrator **together with an actual Verter checkout and command access**. The architecture ZIP alone cannot inspect, modify, test, or review the repository.

**Normative entry:** `ORCHESTRATOR.md`. This guide, the bootstrap, and the role files are adapters; they cannot override the package contracts.

# 1. Verify the release

From the directory containing the release artifacts:

GNU/Linux:

```bash
sha256sum -c verter-architecture-v11.sha256
```

macOS:

```bash
shasum -a 256 -c verter-architecture-v11.sha256
```

Then on either platform:

```bash
unzip -q verter-architecture-v11.zip
python3 verter-architecture-v11/tools/validate_package.py verter-architecture-v11
python3 verter-architecture-v11/tools/selftest_orchestration.py
```

Do not continue on any checksum, manifest, live-self-test, validation, or extraction mismatch.

# 2. Launch the orchestrator from the Verter checkout

First verify the Claude Code runtime:

```bash
claude --version
```

The `claude-opus-5` model ID requires Claude Code **2.1.219 or later**. Upgrade before continuing if the installed runtime is older, and record the exact runtime version in A0 evidence.

Place the extracted package beside the repository, then launch Claude Code from the repository root:

```bash
cd /path/to/verter
claude --model claude-opus-5 --add-dir ../verter-architecture-v11
```

The adapter requests the fixed model ID `claude-opus-5`, not the floating `opus` alias. At startup, record the actual model/provider shown by Claude Code. A fallback or substitution is reported and causes the Opus-specific handoff to stop unless the designated maintainer explicitly accepts that runtime.

# 3. Paste the bootstrap prompt

Paste the complete contents of either checksum-verified copy:

```text
../verter-opus-orchestrator-prompt-v11.md
../verter-architecture-v11/agents/opus-bootstrap.md
```

They are byte-identical in a valid release.

The first run executes **A0 only**. It does not begin broad implementation, choose post-result performance gates, create a program-wide PR stack, or claim that the unimplemented architecture has already passed its final proof.

# 4. Optional role adapters

Optional Claude Code subagent definitions live under `agents/claude-code/`. Review them before copying them into the repository's `.claude/agents/` directory. They are convenience adapters only. `governance.md`, `contracts/agent-orchestration.md`, and each block's immutable context packet remain authoritative.

Do not treat several identically prompted model instances as automatically independent. Foundational review requires distinct mandates, clean contexts, direct evidence, and exact-candidate binding; a different model or human reviewer is valuable where available.

# 5. First-run success condition

A successful first run returns an A0 evidence package with:

- validated Revision 11 package digest and release provenance;
- requested and actual model, provider, and orchestrator runtime/version identity;
- exact repository entry checkout SHA/tree and dirty/worktree/submodule state;
- architecture-affecting open-change disposition;
- GitHub, CI, merge-queue, stack-tool, and permission facts;
- initialized and validated `program-state.toml`;
- no unauthorized post-A0 implementation.

Only the designated maintainer can accept A0 and authorize the next legal block.


---

# Consolidated source: `ORCHESTRATOR.md`

# Verter Revision 11 — Orchestrator Entry Point

**Use this file as the first package instruction for the orchestrator.**  
**Canonical input:** the extracted Revision 11 split package plus an actual local checkout of `pikax/verter`.  
**Initial authorization:** execute `A0` only.

# 1. Your role

You are the implementation orchestrator, not the maintainer and not an architecture authority.

Your job is to:

- validate the release package and actual checkout;
- maintain exact program, branch, stack, candidate, evidence, and review state;
- execute only the next legal bounded block;
- delegate substantial independent work through immutable context packets;
- stop and report when source or evidence contradicts the plan;
- recommend acceptance only from reproducible evidence.

You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.

# 2. Opus adapter identity

The supplied Opus bootstrap requests fixed model ID `claude-opus-5`. Record the actual model, provider, and any fallback/substitution in `program-state.toml` before repository mutation.

If the actual runtime differs from the requested runtime, you may complete non-mutating package/repository inspection, but return `A0 BLOCKED` before representing the result as an Opus-governed Foundational outcome unless the designated maintainer records an explicit exception. Model brand does not replace role independence or evidence.

# 3. Read before acting

Read, in this order:

1. `README.md`
2. `contracts/agent-orchestration.md`
3. `contracts/stacked-prs.md`
4. `contracts/package-publication.md`
5. `contracts/architecture-falsification.md`
6. `program-dag.toml`
7. `governance.md`
8. `charters/A0.md`
9. `contracts/baseline-lock.md`
10. `contracts/current-tree-reconciliation.md`
11. the A0-relevant sections of `verification.md`

Do not load the entire package into every worker context. Give each worker only its bounded context packet and directly relevant authority/evidence.

# 4. First commands

From the extracted package:

```bash
python3 tools/validate_package.py
python3 tools/selftest_orchestration.py
claude --version
python3 tools/validate_program_state.py \
  --dag program-dag.toml \
  --state templates/program-state.template.toml \
  --mode template
python3 tools/validate_stack_window.py \
  --dag program-dag.toml \
  --stack templates/stack-window.template.toml \
  --mode template
python3 tools/validate_landing_equivalence.py \
  --proof templates/landing-equivalence.template.toml \
  --mode template
```

From the actual Verter checkout, collect without mutation:

```bash
git rev-parse --show-toplevel
git remote -v
git branch --show-current
git rev-parse HEAD
git rev-parse HEAD^{tree}
git status --porcelain=v1 --untracked-files=all
git worktree list --porcelain
git submodule status --recursive || true
git log -1 --decorate=full --stat
```

Enumerate local/remote architecture-affecting branches and open/queued pull requests using the repository's available GitHub tooling. Record unavailable permissions or APIs rather than assuming a clean state.

Initialize a repository-local ignored or external evidence directory and copy `templates/program-state.template.toml` to `program-state.toml`. Set top-level `status = "ACTIVE"`, resolve every A0-required field, then run:

```bash
python3 <REV11_PACKAGE>/tools/validate_program_state.py \
  --dag <REV11_PACKAGE>/program-dag.toml \
  --state <REPO_EVIDENCE>/program-state.toml \
  --mode live
```

# 5. Execute only A0

A0 produces:

- exact entry checkout SHA/tree and dirty/untracked/submodule/worktree state;
- remote, branch, target-lineage, and toolchain identity;
- Revision 11 package, release, and DAG digests;
- open PR/branch/queued-change disposition, including changes touching compiler, TypeInfo, flow, source ownership, CSS, framework, LSP/provider, protocol, cache, persistence, or lifetime boundaries;
- available command, CI, branch protection, merge queue, stack tooling, signing, and permission facts;
- designated maintainer and orchestrator identities;
- requested/actual model, orchestrator runtime/version, provider, and fallback state;
- initial validated program state with only legal next work exposed;
- A0 context/evidence packet and exact review state.

Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.

# 6. Delegation

Use no subagent when A0 can be completed directly with a small number of repository/tool calls.

When delegation is genuinely useful:

- default to no more than three active worker contexts;
- assign one role and one immutable context packet per worker;
- use separate writable worktrees/branches;
- do not ask workers to “review the entire architecture”;
- do not use a subagent merely to echo or generically double-check your own conclusion;
- keep conformance, architecture, and adversarial mandates distinct where required.

# 7. Stacked PR policy

Stacked PRs are allowed only under `contracts/stacked-prs.md`. Every `ACTIVE` or later stack snapshot is validated both against its pre-stack program-state basis and the current program-state ledger.

Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.

Private atomic layers, especially D1, are never independently mergeable to trunk.

# 8. Stop instead of improvising

Stop and report `BLOCKED` or `RESCOPE_REQUIRED` when:

- release checksum or package validation fails;
- the checkout is dirty or contains unowned work;
- target trunk differs from the expected line and the difference is not dispositioned;
- an open PR/branch competes for an authority;
- required repository/GitHub/CI facts cannot be obtained;
- source disproves an architectural assumption;
- A0 would require production ownership changes;
- the maintainer identity or acceptance path is absent;
- actual model/runtime differs from the Opus adapter without an accepted exception.

Do not fix these by hiding changes, deleting someone else's work, weakening checks, or assuming intent.

# 9. Return format

Lead with exactly one:

```text
A0 ACCEPTANCE RECOMMENDED
A0 BLOCKED
A0 RESCOPE REQUIRED
```

Then return the bounded record required by `contracts/agent-orchestration.md`, including raw evidence paths/digests, requested/actual model, exact SHA/tree, unresolved decisions, and next legal blocks derived from validated state.

Do not claim the architecture is implemented or performance-proven. A0 establishes a trustworthy entry state only.


---

# Consolidated source: `agents/opus-bootstrap.md`

# Claude Opus 5 Orchestrator Bootstrap — Revision 11

This is a convenience adapter. The normative rules are `ORCHESTRATOR.md`, `contracts/agent-orchestration.md`, `contracts/stacked-prs.md`, `governance.md`, and the active block charter.

Paste the following into the main Opus session together with the Revision 11 ZIP, a local Verter checkout, and command access:

---

You are the Verter Revision 11 implementation orchestrator. The requested main-session model is the fixed ID `claude-opus-5`. First record the actual model, provider, orchestrator runtime/version, and any fallback or substitution. If it differs, continue only far enough to produce an honest A0 blocked report unless the designated maintainer explicitly accepts the actual runtime.

Extract and validate the attached split package, then begin at `ORCHESTRATOR.md`. Execute **only block A0** in this run. Do not implement later architecture blocks, widen scope, alter accepted architecture, choose post-result gates, or create a program-wide PR stack.

Validate the release/package first; inspect the exact local repository state; enumerate architecture-affecting open work and repository/CI/merge permissions; initialize and validate `program-state.toml`; and return the A0 evidence/acceptance record required by the package.

You are not the maintainer. You may not self-accept A0, A6, an architecture amendment, a gate change, a formal rescope, or a merge. Stop with `BLOCKED` or `RESCOPE_REQUIRED` when facts, authority, permissions, or model identity are missing instead of inventing assumptions.

Use subagents only for genuinely independent substantial work or a required distinct review mandate. Do not spawn agents merely to summarize or repeat your own conclusion. Keep active delegation bounded to the package default, give every writer one immutable context packet and one worktree/branch, and never allow two agents to overwrite the same mutable surface.

Keep progress updates brief. Finish with the outcome first, then release/package digests, requested/actual model, orchestrator runtime/version, exact SHA/tree, evidence paths, unresolved decisions, stack-tool facts, and the next legal blocks derived from validated program state.

---


---

# Consolidated source: `architecture.md`

# Verter Architecture — Revision 11

**Status:** Durable normative architecture; implementation entry is governed by the Revision 11 readiness lock.  
**Baseline-independent:** implementation evidence is recorded separately under `baseline/`.  
**Breaking changes:** allowed and expected where they remove a harmful pre-1.0 contract.


# 0. Implementation-entry lock and current-tree reconciliation

The durable target is baseline-independent, but an implementation block is not. Before an affected cutover starts, the implementation checkout must record one exact SHA and reconcile every current authority, consumer, lifetime, cache, protocol, and test route touched by that block.

Current owners are not presumed obsolete merely because Revision 11 uses neutral logical names. Existing registered-source/VFS publication, `resolver_core`, `IndexedReady`/`DeclBodyMemo`, retained parse workers, `ProjectTypeStore` cache families, `CodeTransform`, `StyleSyntaxIr`, component-meta native/compat paths, provider synchronization, and `VerterHost` responsibilities are candidates for **Preserve**, **Converge**, **Replace**, **Delete**, or **Defer**. Source evidence, not plan terminology, decides the disposition.

No block may:

- create a second owner while postponing the disposition of the current owner;
- rename an existing correct authority and count the rename as architecture progress;
- delete a current owner whose invariant has not moved completely;
- implement against “an architecture-equivalent successor” without an exact refreshed baseline lock;
- use this plan to override a current public/wire compatibility obligation that was not inventoried.

The implementation-entry and reconciliation contracts live under `contracts/`. They are normative for block admission, not production runtime concepts. `program-dag.toml` is the machine-readable semantic sequencing authority; validated program state records execution, while PR stack topology is transport only. No block begins before all direct predecessors are accepted and the ledger marks it ready, except contingent upper-layer draft/review work bound to an exact lower predecessor in the same validated stack snapshot; acceptance still requires formally satisfied predecessors and restack/revalidation. Agent roles, context packets, worktrees, stack windows, review state, and campaign evidence must not leak into runtime APIs, artifact identities, production comments, or semantic behavior.

# 1. Architecture constitution

## 1.1 Product objective

Verter is a deterministic, high-performance compiler and tooling platform for component frameworks. It provides a low-overhead direct compiler, an explicitly reusable prepared compiler, and a bounded managed incremental engine. These regimes share algorithms and semantic authorities but do not share mandatory ownership, synchronization, or retention policy.

The architecture optimizes for:

1. correctness and honest unsupported behavior;
2. minimum work for the requested operation;
3. deterministic output;
4. direct compiler latency, CPU, allocation, copied bytes, and peak RSS;
5. bounded long-running managed memory;
6. immutable incremental reuse;
7. clean framework and external-provider boundaries;
8. low conceptual burden.

No performance result is valid when it is obtained through stale reuse, skipped required work, weaker output, missing maps, fabricated types, omitted diagnostics, or unbounded retention.

## 1.2 Optimization order

The mandatory order is:

1. delete unnecessary work;
2. narrow the operation's prerequisite closure;
3. reuse an exact live artifact;
4. fuse repeated passes or materialization when fusion reduces total work and preserves ownership clarity;
5. late-materialize terminal representations;
6. reduce copies, allocations, hashing, formatting, locks, atomics, channels, and pointer traffic;
7. parallelize sufficiently coarse independent work;
8. only then evaluate allocator changes, PGO, SIMD, lock-free structures, specialized hashing, or a custom executor.

A low-level optimization cannot justify retaining duplicated semantic owners or repeated source-sized work.

## 1.3 Determinism and hermeticity

A Verter-owned product result is determined by:

```text
(authoritative source/input view,
 semantic profile,
 output profile,
 presentation profile when presentation is requested,
 serialization profile when serialization is requested,
 framework/compiler compatibility identity,
 compositional product request)
```

For equal tuples, Verter produces equal, as applicable:

- generated bytes;
- required `SourceProjectionMap` segments and order;
- optional `RuntimeSourceMapData` segments and encoded bytes;
- deterministic `StableEntityId` values;
- diagnostics and canonical order;
- exactness/completeness/failure classification;
- dependency fingerprints;
- rendered type text under the same presentation profile;
- serialized payloads under the same serialization profile.

Equality is required across cold/warm, direct/prepared/managed, native threaded/native single-thread, WASM single-thread, and legal task schedules for operations declared portable across those profiles.

Ephemeral `SessionHandle` values are intentionally excluded from cross-regime equality. They are validated against their owning session/cohort and are never represented as deterministic public entity IDs.

Reusable computation is hermetic over captured observations. It does not read current time, randomness, process-global mutable options, ambient filesystem state, locale, host path formatting, or untracked environment variables. Missing information becomes an explicit data-observation demand, provider request, or typed outcome.

## 1.4 Simplicity

Every service object, trait, cache, lock, queue, actor, version, background task, and cross-crate boundary must prove why a direct function, immutable value, private owner, or request-local calculation is insufficient.

Preference order:

```text
pure function
-> borrowed/request-local value
-> immutable value object
-> small private owner
-> reusable query when cross-request reuse or validation is valuable
-> executor task when work is coarse and independent
-> actor when a stateful external protocol/resource requires serialized ownership
```

A one-implementation public trait is rejected unless it is a real substitution boundary. A new crate is rejected unless it establishes a dependency firewall, stable protocol, or multiple credible consumers.

## 1.5 Constitutional invariants and evidence-selected tactics

Revision 11 is the strongest defensible target under current evidence, not a claim of globally proven optimality before implementation. `contracts/architecture-falsification.md` classifies decisions as constitutional invariants, repository/product facts, evidence-selected tactics, or deferred research choices.

Constitutional rules—such as one semantic authority, honest completeness, direct/managed separation, deterministic products, bounded lifetime, and one-path cutovers—cannot be weakened locally. Repository facts are refreshed against the exact checkout. Private tactics may change when measurements disprove them, provided public semantics, ownership, failure behavior, compatibility, and locked gates remain intact. Deferred research does not enter a candidate through speculative “future proofing.”

A6 accepts an architecture premise ledger. Falsified premises stop affected work, preserve independently valid work, and trigger refresh, rescope, or ADR amendment according to their class.

# 2. Product and capability model

## 2.1 Framework citizenship and maturity

Vue and Svelte have equal architectural citizenship:

- each owns its grammar, semantic policy, compile options, runtime generation, framework diagnostics, and source mappings;
- neither is represented as a special case of the other;
- shared owners do not grow a universal framework AST, template IR, runtime IR, fact bag, or options bag;
- a future framework adds capability implementations, not another source authority, query runtime, executor, cache owner, provider hub, or publication protocol.

Product maturity is separate and operation-specific. `contracts/capability-matrix.md` is the authoritative product ledger. Every row fixes:

- framework and operation;
- route/backend;
- maturity and default-enabled state;
- supported semantic profile(s);
- oracle/conformance source;
- exact unsupported/degradation behavior;
- required zero-work negative assertions;
- compatibility promise.

```rust
enum CapabilityMaturity {
    Experimental,
    Preview,
    Supported,
    Stable,
}
```

An operation absent from the ratified matrix is unsupported. An unknown or incompatible profile does not silently inherit another row. Matrix changes are product changes and require their own conformance evidence.

## 2.2 Compositional compiler products and TypeInfo operations

Compiler requests are a canonical non-empty collection of typed product requests, not one mutually exclusive level or one global profile bag:

```rust
enum CompileProductRequest {
    RuntimeClient(RuntimeProductRequest),
    RuntimeServer(RuntimeProductRequest),
    IdeCompanion(IdeProductRequest),
    PublicApi(PublicApiProductRequest),
    Declarations(DeclarationProductRequest),
}

struct RuntimeProductRequest {
    output_profile: OutputProfileId,
    runtime_source_map: Option<RuntimeMapRequest>,
    provenance: Option<ProvenanceRequest>,
    serialization: Option<SerializationProfileId>,
}

struct IdeProductRequest {
    output_profile: OutputProfileId,
    // SourceProjectionMap is a required part of this product, not an option.
    diagnostics: Option<PresentationProfileId>,
    serialization: Option<SerializationProfileId>,
}

enum FrameworkCompileRequest {
    Vue(VueCompileRequest),
    Svelte(SvelteCompileRequest),
}

pub struct CompileRequest {
    pub products: NonEmptyProductRequests, // canonical by product kind; duplicates rejected
    pub framework: FrameworkCompileRequest,
    pub semantic_profile: Option<TypeScriptSemanticProfileId>,
}
```

The exact Rust shape may use typed builders or product-specific structs. The invariants are binding:

- multiple products can be requested together and each product owns the output/terminal profiles that affect it;
- no single global `OutputProfileId`, materialization bitset, presentation profile, or serialization profile is presumed valid for every requested product;
- required product parts, such as an IDE `SourceProjectionMap`, are implicit in that product contract and cannot be disabled;
- optional terminal forms, such as runtime map encoding, rendered diagnostics, provenance, or serialization, are attached only to the product/result they materialize;
- equivalent per-product subrequests share prerequisite stages and artifacts through normalized subplan identities;
- irrelevant profiles are rejected or normalized to absence before identity construction; they do not over-key unrelated stages;
- Vue- and Svelte-specific options are typed before planning;
- unsupported combinations and duplicate product kinds are rejected before expensive work;
- no public `analysisLevel=full` or universal options bag widens work implicitly.

TypeInfo callers request operation results, never internal representation levels:

```rust
enum TypeInfoOperation {
    ResolveSymbol,
    ProjectPath,
    TypeAtPosition,
    DeclaredTypeOfSymbol,
    Relate,
    FrameworkSurface,
    CompileProjection,
    ExportSemanticGraph,
}
```

Each operation has an operation-specific request/result contract, exactness, dependencies, and failure policy. `ExportSemanticGraph` is advanced and optional; it is never the mandatory base response.

## 2.3 External TypeScript and native semantics

External TypeScript provider selection and Verter-native semantics are orthogonal.

External provider modes may include:

```rust
enum ExternalTypeProviderMode {
    Auto,
    SharedNativeTypeScript,
    ManagedNativeTypeScript,
    Tsserver,
    Extension,
    Off,
}
```

Native semantic mode is independent:

```rust
enum NativeSemanticMode {
    Off,
    Enrich,
}
```

Expensive generic native enrichment is off by default on the LSP hot path unless a product operation explicitly requires a Verter-owned framework fact. Disabled or unrequested enrichment performs zero semantic work, creates no retained artifact, and adds near-zero dispatch overhead.

Rules:

- one request uses one selected provider binding and epoch;
- providers are never raced for semantic winner selection;
- explicit compatible user selection wins;
- an incompatible explicit selection returns an actionable capability error;
- `auto` is evidence-based and project-scoped;
- provider failure does not silently switch semantic engines inside a request;
- native enrichment may add separately labeled Verter facts but never overwrite provider TypeScript semantics;
- native TypeInfo is not an automatic fallback;
- a future native-primary mode requires a separate conformance/product program.

`verter_tsc` uses a narrow native-TypeScript batch checker client and does not import `ProviderHub`, editor attachment, hover/completion state, or LSP provider lifecycle.

# 3. Identity and compatibility

## 3.1 Distinct identity types

Identity types are non-interchangeable:

```text
ContentId                    exact byte content
SourceId                     logical source identity
SourceRevision               exact source version
SourceUnitId                 stable logical carrier unit identity
DocumentVersion              LSP client version
DocumentIncarnation          open/close lifecycle identity
EngineRevision               committed input ordering aid
ProjectRevision              project topology identity
ConfigurationRevision        configuration identity
SyntaxProfileId              grammar/source-type/recovery/options identity
ParseKey                     exact syntax construction identity
ParseOwnerDomainId           direct invocation/batch, PreparedCarrier, or managed owner/shard
ParseInstanceId              (ParseOwnerDomainId, ParseKey, instance generation)
TypeScriptSemanticProfileId  TypeScript-compatible interpretation
OutputProfileId              generated program/runtime contract
PresentationProfileId        rendered text/diagnostic/path-ordering contract
SerializationProfileId       wire/container layout contract
ArtifactKey<T>               typed artifact construction identity
QueryIdentity<Q>             semantic arguments + profiles + result contract; snapshot-independent candidate lookup
InputBasisId                 exact captured semantic observation basis
SemanticFlightKey<Q>         (QueryIdentity<Q>, InputBasisId)
ResultContractId             observable semantics/exactness/capability/approximation contract
ProviderEpoch                selected provider lifecycle identity
ProviderContractId           provider route/version/capability interpretation
RequestGeneration            supersession order for one request stream
CompatibilityDomainId        serialized/persistent interpretation namespace
CompatibilityEpoch           monotonic epoch inside a domain
StableEntityId               deterministic public/content-relative identity
SessionHandle                opaque owner/cohort-bound continuation handle
```

A digest encodes identity; it is not identity authority by itself. Collision-sensitive uses retain equality material or verify equality on collision.

Deadlines, cancellation tokens, trace IDs, priorities, queue classes, and ordinary work/time/memory budgets are not reusable query identity. Semantic/output/presentation/serialization profile values remain in their named profile IDs; `ResultContractId` does not duplicate them. It represents operation/product shape, required exactness/completeness, capability and unsupported/degradation policy, required mapping/diagnostic/serialization outcome at the typed boundary, and explicitly requested approximation mode. Waiter-local limits remain in `ExecutionPolicy`; budget exhaustion yields partial/failure rather than a weaker complete result.

`EngineRevision` orders commits and captures snapshots. It is not a universal cache key.

## 3.2 Semantic, output, presentation, serialization, and execution profiles

`TypeScriptSemanticProfileId` identifies observable TypeScript-compatible interpretation. Its closed normalized descriptor includes every supported dimension that can change meaning, such as:

- supported TypeScript compatibility family/version;
- semantics-affecting compiler options;
- module/module-resolution mode and conditions;
- JSX semantics;
- target/lib basis, including exact custom/versioned library fingerprints where relevant;
- package exports/imports, paths, type roots, package-boundary, case, symlink, and workspace policy;
- declared supported behavior of the Verter semantic kernel.

It does **not** contain diagnostic wording, path display, serialization layout, worker count, cache policy, build timestamp, or an internal progress counter. Unknown behavior-affecting options fail closed.

`OutputProfileId` owns generated-program semantics: framework target, client/server/runtime mode, helper/runtime compatibility, and other code-generation dimensions.

`PresentationProfileId` owns deterministic rendered forms: diagnostic text version, rendered type text, path display, locale policy, and stable presentation ordering. It is absent when presentation is not requested.

`SerializationProfileId` owns wire/container encoding and field layout. It is absent when serialization is not requested.

Execution profile—native threaded, native single-thread, or WASM single-thread—is not semantic identity for operations declared portable. Capability differences are declared before execution; they do not silently change the result contract.

An internal refactor leaves the semantic profile unchanged unless observable semantics change. Verter-specific stricter analysis is separately labeled enrichment.

## 3.3 Compatibility domains

Version numbers represent interpretation compatibility, not progress.

Classify every version-like value:

1. **No compatibility boundary:** internal in-memory implementation detail. Delete it.
2. **Persistent/precomputed boundary:** retain one typed compatibility domain and monotonic epoch only if older bytes could otherwise be misinterpreted.
3. **External wire/protocol boundary:** retain a named domain, schema/epoch policy, and compatibility tests.

Rules:

- `0` is a valid first epoch and never means uninitialized;
- epochs are monotonic within a published domain;
- a clean incompatible replacement may create a new domain/namespace whose first epoch is `0`;
- do not reset an existing published domain merely because the project is pre-1.0;
- one domain has one owner;
- no duplicate counters must remain equal by convention;
- ordinary in-memory DTOs are versionless;
- a build/toolchain fingerprint may invalidate disposable caches without per-subsystem progress counters;
- disposable beta caches may be dropped without converters;
- package semver, external tool versions, source revisions, and provider epochs are separate concepts.

# 4. Execution regimes and platform profiles

## 4.1 One-shot fused execution

The one-shot compiler:

- borrows root source on the primary Rust API;
- creates no workspace, provider hub, durable query database, background scheduler, audit store, or managed cache set;
- executes only the requested stage closure;
- keeps OXC and local semantic state owner-affine;
- performs compile projections once in a batch only when demanded;
- drops request arenas and state at return;
- performs no ambient external source access.

One unavoidable FFI ownership copy may be accepted at a host boundary. A second avoidable native copy is not.

## 4.2 Prepared progressive execution

`PreparedCarrier` is an explicit retained object. The primary constructor borrows source; an explicit owned constructor may accept already-owned source for FFI or caller-chosen retention without imposing that copy on borrowed Rust callers.

- single-owner and normally accessed through `&mut`;
- not required to be `Sync`;
- `Send` only if whole-owner transfer is naturally safe and useful;
- retains only independently valuable stages that were actually materialized;
- exposes retained logical weight and drop behavior;
- does not implicitly retain project/provider state;
- releases its request/prepared arenas when dropped.

A stage boundary remains only when it proves skip, reuse, or lifetime value. Otherwise the stage stays fused.

## 4.3 Managed incremental execution

The managed engine adds:

- coherent immutable snapshots;
- exact content and query identities;
- value-side validation;
- same-key shared flights;
- cancellation and supersession;
- lazy committed input loading;
- bounded owner-local retention;
- stable-unit artifact reuse;
- provider synchronization and stamped publication.

It invokes the same parser, planner, TypeInfo, framework projection, and code-generation algorithms as the direct compiler. Managed reuse cannot alter semantics.

## 4.4 Execution profiles

The architecture supports:

1. **Native threaded:** bounded structured CPU execution and owner-affine workers.
2. **Native single-thread:** identical semantics with inline/local execution and no unnecessary concurrent containers.
3. **WASM single-thread:** local/cooperative execution, non-`Send` stores, and no dependency on blocking condvars or native worker pools.

Optional future WASM threads form a separate capability and do not redefine the single-thread contract.

`QueryRuntime` semantics—identity, completeness, validation, cancellation, recursion, and admission—are independent from how a profile schedules work.

# 5. Input authority and snapshots

## 5.1 One committed input authority

The managed system has exactly one authority for committed sources, overlays, project topology, and configuration. `InputStore` is the logical role; Revision 11 does not require inventing a new type when the current registered-source/VFS/`PublishedRoot` machinery already owns the invariant and can be converged in place.

The required invariant is:

> A reader observes one coherent source/project/configuration state before a commit or after it, never independently sampled roots from different commits.

An immutable/persistent root published through a short atomic commit is preferred. Equivalent implementations are allowed only if they preserve coherent capture, ordered document mutation, conditional commit, short writer critical sections, bounded memory, and deterministic snapshot identity.

No parse, compile, TypeInfo, provider, index, cache validation, or external I/O runs inside the input write critical section.

Before managed QueryRuntime/cache convergence, the current input/VFS/publication owner is classified and its snapshot/fact API is locked. Query infrastructure must not be built around a transitional or independently sampled source view.

## 5.2 Ordered document mutation

For each document:

- accepted incremental edits are applied in order;
- incarnation and document version are validated;
- edits are never dropped or coalesced before source application;
- derived work may be cancelled/coalesced after commit;
- an invalid range/version gap enters an explicit unsynchronized state;
- a later range is never applied to unproven text.

## 5.3 Lazy input loading

Reusable semantic/compiler computation performs no unversioned filesystem, network, package-manager, or process read behind a snapshot. Missing information is resolved by the deterministic state machine in `contracts/input-loading.md`.

At a high level:

```text
Attempt(snapshot, accumulated observations, resource budget)
  -> Complete(result)
  -> NeedInputs(normalized LoadSet)
  -> TerminalFailure
```

For `NeedInputs`:

1. normalize, sort, and deduplicate the complete discovered set for the attempt;
2. compute `delta = requested - previously_requested`;
3. fail with typed `InputResolutionNoProgress` when `delta` is empty and no observed basis changed;
4. enforce total unique-input, byte, dependency-depth, retry, and churn budgets;
5. load `delta` through same-key I/O flights owned outside the semantic kernel;
6. conditionally commit successful and stable negative observations as one coherent batch when the project/configuration basis is still current;
7. capture a new snapshot and restart the whole semantic attempt;
8. never splice newly loaded data into an in-progress derivation.

A basis change may justify a retry even with the same symbolic dependency, but it consumes the configured churn/restart budget. Partial load success, stable missing observations, and transient loader failures have distinct typed outcomes.

`TypeInfoEnvironment` is an immutable/data-only observation view. Public/external environments provide bytes and typed metadata, not authoritative semantic fingerprints: Verter validates key/basis consistency and computes or verifies content/configuration fingerprints at the capture boundary. A caller-supplied digest is a hint unless it was minted by a sealed first-party snapshot authority and carries its validity basis. A direct project session returns `NeedInputs` to its caller; a managed embedding may run the outer load/commit/retry loop. The compiler and semantic kernel remain I/O-free.

# 6. Parsing, source units, and artifacts

## 6.1 Shared frontend, parse key, and owner domain

For each language domain, Verter owns one error-tolerant syntax frontend used by IDE and build:

- Vue carrier;
- Svelte carrier;
- JavaScript/TypeScript/JSX/TSX embedded scripts;
- CSS-family syntax through `StyleSyntaxIr`;
- any future domain accepted by ADR.

IDE, build, TypeInfo, lint, formatter, and framework consumers do not maintain independent parsers for equal bytes and syntax profile. If recovery or syntax capability is insufficient, Verter extends, forks, or replaces the shared frontend for both consumers rather than retaining a permanent fast-build/tolerant-IDE split.

```rust
struct ParseKey {
    content: ContentId,
    language: LanguageId,
    syntax_contract: SyntaxCompatibilityId, // named domain + monotonic epoch
    syntax_profile: SyntaxProfileId,        // normalized parse/recovery/source-type options
}

enum ParseOwnerDomainId {
    DirectInvocation(DirectInvocationId),
    DirectBatch(DirectBatchId),
    Prepared(PreparedCarrierId),
    Managed(ManagedParseOwnerId),
}

struct ParseInstanceId {
    owner_domain: ParseOwnerDomainId,
    key: ParseKey,
    generation: ParseInstanceGeneration,
}
```

Consumer role (`IDE`, `build`, `TypeInfo`, `lint`) and framework name are not identity dimensions. A genuinely incompatible syntax construction uses a different typed frontend compatibility domain or normalized `SyntaxProfileId` and requires an accepted ADR proving it cannot be unified without material correctness or performance loss.

Invariant:

> One live `(ParseOwnerDomainId, ParseKey)` has one owner and one active result. After explicit pressure eviction in a retaining domain, a later demand may perform at most one visible same-key reparse flight.

Independent direct owner domains may parse independently and never consult a process-global cache. A `PreparedCarrier` reuses only what the caller explicitly retained. Managed owner domains may share equal source-neutral syntax only when source placement is represented separately and key equality is complete.

Changed/truncated/recovery bytes have a different `ContentId`; they are not another result for the same key.

## 6.2 Parse ownership, locators, and affinity

OXC allocator/AST state is owner-affine:

- direct instances die with their invocation/batch;
- prepared instances die with the explicit `PreparedCarrier` or its chosen stage eviction;
- managed instances live on a named owner/shard and are byte-bounded;
- AST/allocator objects are never cloned or moved merely to satisfy `Send`, `Sync`, or `'static`;
- consumers run on the owner or receive compact OXC-free results;
- a retained index/graph does not implicitly pin a parse instance;
- a live lease pins the exact instance only for the operation;
- after eviction, exact authored access reacquires through one owner-domain-local same-key flight;
- no public result retains an arena or raw node pointer.

An `AuthoredLocator` carries source/unit identity, parse key or source-relative structural basis, span, node kind, and a structural fingerprint sufficient to reject stale/mismatched reparse results. It is validated before dereference after reparse; failure returns a typed stale/locator error rather than reading another node.

Owner calls use compact descriptors with deterministic response ordering. Affinity is a placement/lifetime constraint, not a second semantic query or scheduler graph.

## 6.3 Source units and mapping taxonomy

Framework frontends produce logical script/template/style/custom units with stable exact identities and placement mappings. Full-carrier whitespace blanking is not a final representation.

Mapping types are distinct:

1. **`PlacementMap`** — internal source/unit placement composition where required;
2. **`SourceProjectionMap`** — required by an IDE/provider companion and atomically published with that exact companion;
3. **`RuntimeSourceMapData`** — optional runtime/build map segments created only when the request asks for them;
4. **`EncodedSourceMap`** — terminal serialized representation created only when requested.

A generated artifact includes only mappings required by its product:

```text
GeneratedArtifact
├── generated bytes
├── exact source/unit/request/output-profile basis
├── required SourceProjectionMap or RuntimeSourceMapData, if any
├── dependency/exactness basis
└── optional requested terminal metadata
```

Code and its required mapping publish atomically. An operation with no mapping requirement performs zero map construction and encoding.

## 6.4 Artifact lifetime ladder

Every value starts in the cheapest valid lifetime:

```text
borrowed request-local
-> compact owned boundary value when it must outlive/cross the owner
-> admitted managed artifact when measured cross-request reuse is valuable
-> optional persistent artifact only when hermetic eligibility is proven
```

Correct computation does not imply admission. Return-only results are normal.

# 7. Direct compiler, plans, and compile projections

## 7.1 Direct compiler boundary

The direct compiler is a separate product boundary, not an `Engine` mode. It exposes borrowed-source one-shot compilation, explicit prepared reuse, and staged batch planning/emission. It owns no workspace, committed input store, QueryRuntime, provider lifecycle, durable cache set, audit database, or background scheduler.

No public `dyn CompileTypeInfo`, open resolver trait, `Engine`, `VerterHost`, provider state, or ambient filesystem handle enters the direct compiler.

## 7.2 Typed operation plan

The compiler consumes the final compositional request and derives the smallest acyclic prerequisite closure. A cold request plans once by default. Deliberate replanning is allowed only when benchmark evidence shows it is cheaper in total CPU, allocation, retained memory, and complexity than carrying the first plan.

Unsupported product combinations fail before expensive work. No projection demands means zero TypeInfo construction/call and zero project read.

## 7.3 Resumable projection protocol

Project-aware compilation follows:

```text
prepare -> plan -> project -> emit
```

Conceptually:

```rust
let mut prepared = compiler.prepare(source_id, source, parse_options)?;
let plan = prepared.plan(request)?;

loop {
    match type_info.project(plan.projection_batch())? {
        ProjectionOutcome::Complete(facts) => {
            plan.validate_facts(&facts)?;
            break prepared.emit(plan, facts);
        }
        ProjectionOutcome::NeedInputs(load_set) => {
            return CompileAttempt::NeedInputs {
                prepared,
                plan,
                load_set,
            };
        }
    }
}
```

The exact Rust API may use separate types/builders. The invariants are binding:

- `CompileProjectionBatch` is compact, owned, and OXC-free;
- `CompilePlanToken` binds the complete normalized per-product request to the retained prepared root for anti-replay;
- the plan also contains narrower deterministic `ProjectionPlanToken`, product-subplan tokens, and terminal-subplan tokens, so a terminal-only change does not invalidate unchanged semantic/code artifacts;
- `CompileFactsBatch` binds to the exact `ProjectionPlanToken`, semantic profile, dependency/input basis, and completeness—not to unrelated presentation or serialization fields;
- stale, replayed, mismatched, or injected facts are rejected before emission;
- no AST borrow, arena pointer, or unvalidated locator crosses external I/O;
- `NeedInputs(LoadSet)` is a first-class resumable outcome, not a generic internal error;
- the outer caller may extend a captured project view and retry under the load-wave/no-progress contract;
- a project/configuration basis change invalidates/replans as specified by the token rather than silently reusing facts.

One-shot convenience APIs may orchestrate local-only semantics synchronously. Project-capable convenience orchestration must remain explicit about loading and retry; compute itself stays I/O-free.

## 7.4 Sealed `CompileTypeInfo`

```rust
pub struct CompileTypeInfo<'env> {
    inner: CompileTypeInfoInner<'env>,
}
```

All first-party local, captured-project, in-memory, engine-snapshot, and validated-precomputed modes execute one `TypeInfoCore`, one `ModuleResolverCore`, and one profile-parameterized relation/inference implementation. External integrations supply captured data observations, not resolution/type/classification semantics.

No blanket `Send + Sync` applies. Parallel batches use staged prepare/plan, one TypeInfo owner with internal parallelism or explicit first-party per-worker sessions, and parallel emit when legal.

Validated precomputed facts are optional and accepted only after domain/profile/toolchain/source/dependency/completeness/integrity/size and anti-replay validation. Callers cannot mint arbitrary resolved facts.

## 7.5 Closed framework projections

The compiler discovers exact closed projection demands. Vue runtime macro projection contains only facts runtime generation consumes: prop/model identity, optional/required/default association, ordered broad runtime constructors, `skip_check`/typed degradation, provenance anchor, and exact dependency/profile basis.

Projection follows imports, aliases, substitutions, unions/intersections, utility/mapped/indexed forms, `typeof`, enums, and supported forms only until the closed answer is known. Object internals are not expanded merely to conclude `Object`.

Current Svelte runtime compilation requests no native compile projection unless a future capability ADR proves an official semantic need.

# 8. TypeInfo semantic kernel and public API

## 8.1 One implementation

`TypeInfoCore` owns:

- authored-node traversal;
- binder/name/reference semantics;
- substitution;
- relation/inference;
- effective typing;
- recursion/work budgets;
- query-specific projection;
- exactness/completeness propagation.

`ModuleResolverCore` owns TypeScript-style module/path/package resolution. Environments supply observations; they do not implement semantics.

Direct local, direct project, in-memory, managed, compile projection, component metadata, effective type, and framework surface operations share these kernels. Lifecycle adapters differ only in data access, concurrency, and retention.

## 8.2 No final general `TypeExpr`, with ordered cutover

The final architecture contains no general recursive owned `TypeExpr` or `PortableTypeExpr` as a generic semantic transit IR, final cache value, compile projection contract, or public result.

The cutover is deliberately ordered:

1. freeze every current producer and consumer, including `IndexedReady`/lazy body lowering, flow content, component-meta native/compat, graph export, NAPI/WASM DTOs, and tests;
2. land the final relation/inference/effective-flow authorities and the exact identity/lifetime/admission contract for each consumer being migrated; global QueryRuntime/store convergence is not a prerequisite unless that consumer actually depends on it;
3. migrate one consumer family directly to borrowed authored views, compact exact chunks, semantic nodes, or operation DTOs;
4. delete that family's `TypeExpr` producer, cache, conversion, and compatibility shim in the same accepted merge;
5. remove the public/general contract only after all consumers and protocol obligations are accounted for.

Do not modernize the transitional second flow evaluator merely to remove `TypeExpr`; delete that evaluator through the flow cutover first. Do not make a general graph mandatory merely to replace a general tree.

Surviving forms are:

- borrowed exact authored nodes;
- compact exact authored chunks only across a proven boundary;
- request/generation-local semantic values;
- operation-specific DTOs;
- rendered type text only when requested;
- opaque cohort/session-bound continuation handles;
- optional bounded graph export for actual graph consumers.

## 8.3 Optional graph export

A semantic graph snapshot is an optional advanced product operation, not the mandatory base API. It exists only for consumers that need graph traversal, transfer, audit, or continuation beyond operation-specific DTOs.

Graph export has:

- its own compatibility domain;
- explicit roots and snapshot-local IDs;
- size/node/depth budgets;
- exactness and diagnostics;
- stable serialization tests;
- no promise that internal storage matches wire layout.

Opaque internal handles encode or validate cohort/generation identity. A raw node index never escapes its cohort. Stale use fails deterministically rather than aliasing a new node.

# 9. Effective typing and flow

## 9.1 Structural authority

The reusable structural path is:

```text
FunctionProgramIndex
-> FunctionBodySkeleton
-> FunctionFlowGraph
```

`FunctionFlowGraph` is the one flow dependence authority. It contains stable binding/reference/effect/control/return/call/capture/escape/loop/completion facts and exact authored locators. Another CFG/control tree is prohibited.

Graph construction performs no general type lowering or query-specific semantic evaluation.

## 9.2 Deterministic derived projections

Dominator indexes, loop forests, SCC indexes, reverse adjacency, def-use overlays, capture summaries, and compact schedules are permitted only as deterministic projections of one exact graph/profile identity. They cannot introduce semantic facts, are request-local by default, and have independent weight/eviction when retained.

## 9.3 Demand plan and closed domain registry

A product operation names semantic obligations. The solver computes the transitive closure through one static domain registry before evaluation. Candidate domains include declared/reaching type, narrowing, relation, completion, definite assignment, capture freshness/invalidation, effect, and coverage.

A `FlowDemandPlan` selects only nodes/edges/domains needed by the result. A fixed point runs only on selected obligations that require convergence. A missing registered prerequisite makes the plan invalid and cannot produce a complete result.

## 9.4 Binding and relation authority

Flow state is keyed by stable binding/slot identity, never by string names. Declaration authority exists for annotated and inferred bindings. Joins, substitution, and nominal `unique symbol` overlap/comparability use the shared semantic relation/inference implementation; flow owns no private relation classifier.

## 9.5 Structural returns and completion

For return inference:

```text
result = union(effective_type(return_expr_i, program_point_i)
               for every authored return statement i)
       + (endpoint_reachable ? undefined : nothing)
```

Authored return membership is structural. Reachability does not delete a return contributor. Endpoint reachability is a separate completion question.

Completion uses one algebra:

```rust
enum CompletionKind {
    Normal,
    Return,
    Throw,
    Break(LabelId),
    Continue(LabelId),
}
```

Blocks, conditions, loops, labels, `switch`, `try`, `catch`, and `finally` transform/join completion sets. Suffix booleans, “guarantees return,” and pending-break flags are prohibited.

## 9.6 Closure and effects

The structural producer retains nested capture reads and writes. Summaries include direct/transitive reads, writes, escapes, creation point, freshness, and invalidating-write information.

Effects use one expression-position-independent mechanism. No IIFE/capture hook is specialized to statement, initializer, condition, argument, return, sequence, literal, or another parent position. Compatibility results match the selected semantic profile; extra analysis may decide whether answering is safe but may not publish a different compatibility answer.

## 9.7 Obligation ledger and proof-carrying completeness

Each solve owns a private obligation ledger. Every selected graph/domain/fixed-point/semantic operation finishes as one of:

```text
Discharged
CoverageGap
Unsupported
Cancelled
BudgetExceeded
Stale
InternalFailure
```

Only a private finalizer can construct `CompleteFlowResult`, and only when every required obligation is `Discharged` under the exact graph, demand, profile, input basis, and result contract. Rich audit evidence is optional/request-scoped; the production proof token is compact.

Partial/gap results may be returned but remain non-admissible unless a separate query contract proves partial admission. Authored `any` is distinct from unmodelled semantics.

## 9.8 Atomic sole-solver cutover

Flow migration has one public cutover:

1. build the minimum graph/domain/obligation solver behind a private non-production test boundary, already using stable binding identities and the shared relation/inference authority for every capability that will remain enabled at cutover;
2. route public effective-flow operations exclusively to it and delete the syntax-shaped evaluator, name maps, position-specific hooks, private relation logic, completion booleans, and related caches/tasks/flags in the same accepted merge;
3. return typed gaps for mechanisms not yet supported;
4. preserve every `A6`-ratified Supported/Stable effective-flow capability unless a separate reviewed breaking product decision changes it;
5. expand semantics only inside the sole surviving solver, never by reintroducing a private relation/name authority.

No runtime flag or selectable shadow evaluator survives. Temporary reduction from guessed complete output to honest partial is accepted only for rows not ratified Supported/Stable, or through a separate reviewed breaking product decision.

## 9.9 No-flow fast path

`TypeAtPosition` locates subject, owner, anchor, and base/contextual type once. If the index proves no relevant flow owner/effect, it returns immediately with zero graph/plan/flow allocation, no second source traversal, and no cache entry whose only meaning is “no flow.”

# 10. QueryRuntime, flights, and CPU execution

## 10.1 Separation of concerns

```text
API demand
-> QueryRuntime
   -> validated hit: return inline
   -> compatible running FlightCell: join
   -> new cold FlightCell
      -> cheap dependent work: inline
      -> coarse migratable work: CpuExecutor
      -> owner-affine work: owner call/inline
```

`QueryRuntime` owns semantic query identity, recursion/cycles, fact capture, exactness/completeness, cancellation, flights, admission, validation, and retention hooks. `CpuExecutor` owns only prerequisite-ready migratable CPU execution.

## 10.2 Query identity, flight identity, and result contract

`QueryIdentity<Q>` contains semantic arguments, only the semantic/output/presentation/serialization profiles observable at that typed query boundary, and `ResultContractId`. It is independent of a particular snapshot and is the bounded candidate-cache lookup identity. Terminal presentation or serialization uses a separate query/materialization identity when the underlying typed result is unchanged.

`SemanticFlightKey<Q> = (QueryIdentity<Q>, InputBasisId)`. The exact input basis scopes in-flight semantic production; it is not folded into cross-snapshot cache candidate lookup. A cached candidate carries its production `InputBasisId`, complete positive/negative read facts, proof, and compatibility basis and is usable only after value-side validation against the requester’s current view. A store may keep a bounded, measured number of candidates per query identity. Candidate insertion order, newest-snapshot preference, or global revision never becomes semantic authority.

`ResultContractId` includes semantics, operation/product, required exactness, capability/unsupported policy, explicitly requested approximation mode, and only the output/mapping/diagnostic/serialization contract observable at that typed query boundary.

Waiter-local `ExecutionPolicy` contains deadline, cancellation, priority, and ordinary work/time/memory budget. A larger budget may let a producer continue; it does not change result identity. Budget exhaustion yields partial/failure and never a weaker complete result.

Value-side validation is correctness authority. Reverse dependencies are optional cleanup/observability/prefetch aids.

## 10.3 Flight classes and compatibility

Two default flight scopes exist:

1. **Content artifact flight:** keyed entirely by immutable construction identity and shareable across snapshots.
2. **Semantic query flight:** keyed by exact `SemanticFlightKey<Q>`—one `QueryIdentity<Q>` and one exact `InputBasisId`/snapshot compatibility scope.

Cross-snapshot semantic-flight joining is disabled by default. A later ADR may enable it for one query family only after proving it reduces total work and cannot cause stale use or retry pathologies. Cross-snapshot warm value reuse remains available through normal value-side validation.

## 10.4 `FlightCell` state machine

The flight cell, not the first requester, owns production:

```text
Vacant
-> Running { producer, active_waiters, effective_priority, aggregate_budget }
-> Finalizing
-> Complete | ReturnOnlyFailure
-> Removed
```

Rules:

- each waiter has an independent result channel, deadline, cancellation, priority, and validation basis;
- cancelling the first waiter removes only that waiter;
- while at least one waiter remains, flight/engine policy controls producer cancellation;
- all waiters gone triggers cooperative cancellation and removal after finalization;
- only `Running` accepts new waiters; once budget exhaustion, cancellation, failure, or completion moves the cell to `Finalizing`, a later request starts or joins a successor flight rather than attaching to an irreversible outcome;
- ordinary execution budget may stop work but may not select a semantic approximation or discard required state while the flight remains `Running`; an explicit approximation mode belongs in `ResultContractId`;
- effective producer work/memory budget is a bounded monotonic maximum over active waiter requests, not their sum; priority may rise and later lower within owner policy;
- aggregate budget may increase while the producer is still able to continue, but never creates a weaker complete result;
- panic/failure/cancel/budget/stale outcomes publish no complete value;
- shutdown resolves every waiter exactly once and empties the table;
- cache publication occurs only through the query owner's sealed admission finalizer;
- every follower validates before use;
- owner-affine work never blocks its owner waiting for a command queued to itself.

There is no public “producer transfer” protocol; producer lifetime is flight-owned.

## 10.5 Placement and scheduling

Two placement classes exist:

1. migratable CPU-ready work with compact owned `Send` input/result;
2. owner-affine work over non-`Send` syntax/arena/local semantic state, invoked inline or through a compact owner descriptor.

AST/arena state never crosses. Affinity is not a second scheduler or semantic graph.

Hits/tiny dependent work run inline. Many tiny independent items are chunked. Coarse independent parse/compile/projection work may fork through structured execution. Provider/I/O actors remain separate. Every fork family declares measured grain, fan-out, cancellation/budget inheritance, priority, queue bound, stack/recursion behavior, and structured lifetime. Mature process-local execution is used before custom runtime design.

# 11. Incremental compilation

## 11.1 Stable-unit model

The north star is immutable artifact reuse:

1. edit-friendly text storage and exact `ChangeSet`;
2. carrier/block reconciliation and stable logical unit identities;
3. stage-local artifact reuse for unchanged units;
4. dependency-precise value validation and on-demand recomputation;
5. within-unit mutable/incremental parsing only if profiling proves full-unit parse remains a leading cost and a separate ADR accepts the complexity.

Shared parse, type, flow, semantic, mapping, and codegen artifacts are immutable after publication. New inputs create new generations with safe structural reuse; they are never patched in place while readers may hold them.

## 11.2 Clean equivalence

For final authoritative inputs:

```text
incremental result == clean result
```

Equality includes code, maps, diagnostics, exactness, dependencies, and public stable ordering. Full fallback is allowed for unsupported classification but is visible, counted, bounded, and clean-equivalent.

# 12. CSS and formatter architecture

`verter_css_syntax::StyleSyntaxIr` is the one CSS-family syntax authority.

The style identity includes exact bytes/content, dialect, parse-affecting options, and compatibility domain/epoch.

For one live exact identity:

- formatter, index, navigation, Vue `v-bind`, CSS modules, scoped selectors/keyframes, and Svelte/framework style consumers reuse one syntax artifact/fact substrate;
- no consumer reparses identical bytes because it is a separate stage;
- a transform/preprocessor that changes bytes creates a new explicit identity;
- shared walks/edit plans are fused when semantics and ownership permit;
- an intermediate transform that genuinely changes syntax is materialized explicitly and parsed at most once under its new identity;
- source maps, provenance, output descriptors, and FFI wrappers are terminal and conditional;
- unchanged output reuses/borrows the original identity where ownership permits;
- formatter/index/navigation do not own private scanners or grammars.

External preprocessors supply explicit processed bytes, maps, dependencies, diagnostics, and processor/config fingerprints. The Rust compiler core does not hide arbitrary process/filesystem authority.

Every dialect operation is capability-declared as native, external, or unsupported. Recovery completeness controls which semantic rewrites may be claimed.

# 13. Framework capability architecture

`FrameworkCatalog` is immutable after construction and performs coarse dispatch by open `FrameworkId`/`LanguageId`.

Capabilities may include:

- language classification;
- carrier parse frontend;
- script/embedded-language planning;
- template parsing;
- logical source-group planning;
- external template/style ownership;
- script fact production;
- framework surface projection;
- IDE companion projection;
- runtime compilation;
- diagnostics/lint contribution;
- style semantics;
- source mapping;
- project capability detection.

A framework registers only what it owns. Inner parse/codegen/semantic loops remain typed and monomorphic. Final hot paths contain no `Any + Send + Sync` extras downcast.

Built-in static registrations are the default. Host-supplied in-process semantic registrations require a real consumer and an ADR proving lifetime, safety, compatibility, and API value. A stable dynamic plugin ABI is outside this architecture.

# 14. LSP, providers, and publication

## 14.1 Conventional structure

```text
LSP transport/router
├── ordered input commit
├── immutable request snapshot
├── demanded compiler artifacts
├── project-scoped provider actor
├── optional native enrichment
├── CSS/framework-native features
└── response mapping plus stamped publication validation
```

No semantic scheduler DAG, provider race, global readiness barrier, or universal file state machine is introduced.

## 14.2 Provider bindings

`ProviderHub` is LSP-only and owns project-scoped external provider selection, lifecycle, capabilities, epochs, synchronization, health, cancellation, and deadlines.

A binding is selected from current capability evidence, project TypeScript/SDK, route requirements, provider availability, measured policy, and user preference. The selected provider version and semantic profile are stamped into results.

A request does not switch provider mid-flight. A transition creates a new epoch and supersedes older answers.

## 14.3 Readiness and publication

Readiness is per demanded artifact. A partial IDE companion is never delivered as complete.

Interactive requests wait only for the target document/version, selected provider epoch, project validity, exact companion/mapping revision, and dependencies the route actually needs. They do not wait for unrelated workspace warmup.

Publication carries a compact typed stamp over only observed dimensions, including as relevant:

- request generation;
- document version/incarnation;
- project/configuration revision;
- provider identity/epoch/profile;
- mapping/companion identity;
- dependency read set.

The adapter validates the stamp immediately before publication. Unrelated edits do not invalidate a result.

Mutation handlers validate/commit state and schedule minimal work. They do not synchronously perform full compilation, TypeInfo expansion, workspace indexing, or provider-wide synchronization on the transport loop.

# 15. Retention, persistence, and memory

## 15.1 Owner-local bounded retention

Every retained store declares:

- owner and value type;
- key/identity;
- validation and admission;
- logical weight;
- soft/hard bound;
- eviction and pressure behavior;
- pin classes;
- follower behavior;
- shutdown behavior;
- metrics.

Open current documents may receive protected capacity; superseded revisions are pinned only by live readers. Closed/removed files compete normally and become reclaimable. No cache relies on process restart.

## 15.2 Persistence and precomputed facts are consumer-driven

Persistence is optional acceleration and never correctness authority. Revision 11 does not require a persistent cache or public precomputed semantic mode merely because a value is serializable.

Before implementation, a persistence ADR must name:

- the current product consumer and workload;
- measured recomputation cost and expected reuse;
- complete semantic/profile/toolchain/source/dependency identity;
- compatibility domain and evolution policy;
- integrity/authenticity and bounded decode;
- corruption/mismatch behavior;
- retained/disk budget and eviction;
- privacy/security implications;
- clean recomputation equivalence;
- measured net win after keying, validation, serialization, I/O, and maintenance complexity.

Eligible artifacts are deterministic, hermetic, independently interpretable, complete, and OXC/lifetime free. Never persist owner-local parse handles, AST pointers, snapshot-local handles, transient semantic cohorts, incomplete/cancelled/budget-dependent results, ambient-state-dependent values, or provider results without a separate proven protocol.

Without a ratified consumer ADR, the implementation remains request/prepared/managed-memory only.

## 15.3 Semantic graph and interning

Semantic values have three lifetime classes:

1. query-local ephemeral;
2. generation/cohort-local reclaimable;
3. broadly reusable canonical values when measured.

Global hash-consing is not automatic. Promotion requires reuse to exceed hashing, synchronization, and retention cost. Canonical global domains must be finite or independently bounded.

Interners are request-, project/generation-, cohort-, or bounded-workspace-scoped. Every long-lived interner reports bytes, entry count, duplicate hit rate, age, and reclamation behavior.

## 15.4 Memory observability

Major owners expose:

- live, pinned, and evictable logical bytes;
- pin count/reason;
- oldest pin and generation age;
- superseded-but-pinned bytes;
- last-use age;
- parse/reparse/eviction/admission/refusal counts;
- queue/tombstone/interner/graph/cohort counts.

After long churn and quiescence, logical bytes return to configured budgets plus attributable live pins. RSS is judged with a platform-specific allocator tolerance, but monotonically growing logical state is never excused by allocator behavior.

# 16. Failure containment and trust boundaries

Public operations distinguish:

```text
Unsupported
Partial / CoverageGap
Cancelled
BudgetExceeded
Stale / Superseded
InputUnavailable
ProviderUnavailable
ParseFailure
InternalFailure
```

Authored language values such as `any` are not failure carriers.

Panic/error containment exists at:

- worker/task boundary;
- FFI boundary;
- external provider actor;
- external preprocessor/process adapter;
- persistence decode boundary.

A panic, torn state, decode mismatch, cancellation, stale basis, or exceeded budget cannot construct complete admission evidence.

Parsers, semantic traversals, relation/inference, flow, output, source maps, serialization, and provider messages have explicit depth/node/byte/work limits. Adversarial input yields deterministic typed failure, not process stack overflow, unbounded allocation, or deadlock.

External processes/providers are explicit capabilities with user/product policy, bounded I/O, deadlines, cancellation, and audited data access. External observation adapters and decoded payloads are untrusted at their boundary: identifiers, lengths, content hashes, path/project basis, ordering, and declared profiles/domains are validated before they enter reusable computation. The direct compiler owns no hidden process or filesystem authority.

# 17. FFI and serialization

The pure Rust API is the performance and ownership reference. NAPI, WASM, C, and serialized surfaces are terminal adapters.

Each hot boundary measures:

- Rust-side input copies;
- output reallocations;
- Rust-to-host copies;
- temporary serialized bytes;
- host heap delta;
- conversion CPU/wall time;
- cancelled/superseded work after conversion starts.

Avoid mandatory JSON on hot compiler paths when a typed native or compact binary transfer is appropriate. One explicit final copy may be safer and faster than unsafe borrowed host-memory lifetimes. FFI output does not retain internal arenas or unvalidated semantic handles.

# 18. Dependency direction and owner model

## 18.1 Binding dependency rules

Logical dependency direction is inward:

```text
identity/span/language/contracts
-> shared syntax frontends and dependency-neutral DTOs
-> semantic kernel / module resolver / relation / flow
-> compiler
-> managed engine/session
-> LSP/MCP/provider and NAPI/WASM/CLI adapters
```

Rules:

- syntax/contracts do not depend on compiler, session, provider, or LSP;
- semantic kernel does not depend on compiler, session, provider, or LSP;
- compiler may depend on syntax, the sealed semantic facade, and dependency-neutral closed DTOs;
- semantic projection DTOs live in a neutral owner and do not require compiler↔semantic cycles;
- managed engine depends on compiler/semantic, never the reverse;
- provider lifecycle never enters direct compiler or semantic kernel;
- adapters depend inward and do not expose internals back into core;
- durable build tests reject crate cycles and forbidden edges.

## 18.2 Logical owners

```text
Managed Engine
├── CommittedInputAuthority
├── QueryRuntime
├── CpuExecutor
├── FrameworkCatalog
├── CompilerService
├── TypeInfoService
│   ├── ManagedParseOwners
│   ├── ScriptIndexStore
│   ├── FlowGraphStore
│   ├── SemanticGraphStore
│   └── ProjectSemanticDispatch
├── CssToolchain
├── ProviderHub
└── AuditSink

Direct Compiler
├── shared frontend plus framework-native planner/codegen
├── request/owner-local allocators
├── fused one-shot
├── PreparedCarrier
├── sealed CompileTypeInfo
└── optional validated precomputed closed facts

TypeInfo kernel
├── TypeInfoCore
├── ModuleResolverCore
├── relation/inference/effective typing
└── framework-owned closed projectors

verter_tsc
├── Verter compiler/public projection
└── NativeTypeScriptCheckerClient
```

These are logical roles, not mandatory structs/crates. Existing owners survive when they already satisfy the full invariant with correct dependency direction. A module/function is preferred until a real firewall or stable multi-consumer contract requires a crate.

There is no generic publication coordinator, universal framework host, semantic scheduler DAG, general type IR, process-global direct compiler cache, or public alternate resolver.

# 19. Public boundaries

The direct API exposes a small typed vocabulary:

- `Compiler`, `CompileRequest`, and typed compile attempt/outcome;
- `PreparedCarrier`, `CompilePlanToken`, `CompileProjectionBatch`, and `CompileFactsBatch`;
- concrete sealed `CompileTypeInfo` and first-party constructors;
- captured data-only project observations;
- dependencies, exactness, diagnostics, required maps, and retained weight.

The managed API exposes `Engine`, `EngineSnapshot`, input/project operations, managed requests, provider configuration/capabilities, and bounded observability.

Public TypeInfo operations use operation DTOs, exactness, dependencies, diagnostics, provenance, presentation when requested, deterministic stable IDs, and opaque session handles where continuation is needed. Graph export is separate and bounded.

Public APIs do not expose OXC AST/allocator state, query keys, scheduler jobs, provider internals, mutable stores, raw semantic node IDs, `VerterHost`, or migration carriers.

Every public type documents source ownership, lifetime, deterministic/stable versus ephemeral identity, exactness, failure, compatibility domain when one exists, and intentional `Send`/`Sync` behavior.

Audit, tracing, provenance detail, and debug graph capture are request-scoped or disabled by default. When off, hot paths do not format strings, allocate event payloads, or retain audit graphs; fixed owner-local counters may remain only when measured overhead is negligible.

# 20. Clean cutovers and durable comments

For one bounded capability:

1. implement/test/benchmark the replacement behind a private boundary;
2. freeze the candidate;
3. delete old declarations and implementation;
4. use compiler errors and typed build failures to enumerate callers;
5. migrate every caller to the surviving API;
6. remove displaced caches, tasks, metrics, flags, dependencies, re-exports, wire fields, fixtures, and docs;
7. clean historical/plan-bound comments;
8. run relevant full checks;
9. land one production path.

No compatibility shim, runtime flag, source-name denylist, anti-resurrection scanner, or shadow evaluator survives merely to make migration easier.

Production comments explain current invariants, safety/memory rationale, concurrency/ordering contracts, observable compatibility, or why a simpler-looking implementation is incorrect. Campaign history belongs in ADRs, changelogs, or git history.

# 21. Anti-goals

The following are prohibited:

- conflating parse semantic identity with direct/prepared/managed ownership residence;
- requiring a direct compiler to consult or construct managed parse/cache state;
- building managed QueryRuntime/cache semantics on an unresolved or incoherent input snapshot authority;
- flight sharing based on an opaque policy ID, generic numeric budget ordering, or undocumented dominance; exact result-contract equality is default and any stronger-to-weaker relation is a closed proven query-family exception;
- silent semantic-profile omission of an unknown behavior-affecting option;
- public/precomputed semantic constructors without a current consumer and persistence ADR;
- cutting public `TypeExpr`/component-meta contracts before final flow/query foundations and complete consumer inventory;
- observable IDs or ordering derived from hash-map iteration, worker completion order, pointer address, or cache insertion order;
- a compiler-owned or externally implementable alternate type resolver;
- public `dyn CompileTypeInfo` semantics;
- a second flow CFG/control evaluator beside `FunctionFlowGraph`;
- derived graph indexes that become independent semantic authorities;
- one always-maximal flow lattice when a smaller product domain answers the operation;
- name-keyed flow state as authority;
- flow-private relation/disjointness semantics;
- reachability deleting structurally authored returns;
- position-specific closure/IIFE effect hooks;
- unmodelled semantics becoming `any` or warm-complete;
- cache admission without the operation's completeness proof;
- whole-function semantic fixed points for narrow demands;
- planning/hash/lowering repetition justified only as “cheap”;
- retained graphs implicitly pinning parses;
- no-flow operations allocating graph/slice state;
- forcing local parse/prepared/TypeInfo state to be `Sync`;
- unsafe `Send`/`Sync` declarations hiding ownership defects;
- retaining every parse forever to claim never-reparse;
- one arena coupling independently evictable values;
- immortal semantic arenas, interners, provider maps, tombstones, or audit state;
- correctness-driven eager reverse dependency eviction where value validation suffices;
- caches cheaper than their key/validation/retention path;
- executor tasks for hits or tiny local calculations;
- untracked ambient filesystem/environment reads inside reusable computation;
- one-dependency-per-restart lazy loading without progress bounds;
- public graph export as the mandatory result for simple operations;
- resetting an existing published compatibility domain solely to start at zero;
- repeated exact parse/hash/normalize/render/map/serialize/FFI work without evidence;
- source-sized blanking as a final source model;
- universal framework AST/IR/fact/options bags;
- final hot-path `Any + Send + Sync` framework extras;
- eager maps/provenance/serialization;
- provider races or silent native fallback;
- native-thread primitives as a requirement for WASM semantics;
- persistence of non-hermetic or lifetime-bound artifacts;
- custom executor/allocator/SIMD/PGO before architecture work is complete;
- unbounded queues/caches/graphs/outputs;
- dual old/new selectable production paths;
- performance claims without equivalent work, confidence, copies, allocations, work counts, and bounded memory.


---

# Consolidated source: `contracts/agent-orchestration.md`

# Agent Orchestration and Handoff Contract

**Status:** Normative implementation-execution contract.  
**Entry document:** `ORCHESTRATOR.md`.

# 1. Purpose

This contract makes Revision 11 executable by an orchestrator agent without turning the agent into architecture, repository, or review authority.

The authority package states what must become true. The repository and accepted evidence state what is currently true. The orchestrator may never fill a gap between them by assumption.

# 2. Required principals

Every run names:

- **Maintainer:** human or explicitly designated repository authority that can accept architecture, gates, blocks, and merges.
- **Orchestrator:** one coordinating agent/context that owns sequencing, program-state mutation, worker assignment, evidence collection, and stop/rescope reporting.
- **Workers:** scoped implementor, scoper, or investigator contexts.
- **Review mandates:** conformance, architecture, and adversarial/performance contexts as required by `governance.md`.

The orchestrator may also implement a block when assigned, but it cannot provide that block's only substantive review, cannot self-accept any block or gate, and cannot accept its own work.

# 3. Model adapter identity

The durable contract is model-independent. The supplied Opus adapter requests the fixed ID `claude-opus-5` and records:

- requested model ID;
- actual model ID;
- orchestrator runtime and exact runtime version;
- provider;
- fallback or substitution;
- any explicit maintainer exception.

A mismatch does not become invisible merely because work appears correct. Without an explicit maintainer exception, a substituted runtime may inspect and report but cannot represent an Opus-specific Foundational result as accepted. Multiple instances of one model are not automatically independent evidence.

# 4. Trust bootstrap

Before repository mutation, the orchestrator must:

1. read `README.md`, `ORCHESTRATOR.md`, `program-dag.toml`, `governance.md`, this contract, and the current block charter;
2. run `python3 tools/validate_package.py` against the extracted package;
3. compute and record the package manifest digest;
4. record the requested and actual model, orchestrator runtime/version, and provider identity, including any fallback or substitution;
5. inspect the actual local repository, remotes, branch, full SHA/tree, dirty/untracked/submodule state, worktrees, open local branches, and available GitHub/CI permissions;
6. initialize or validate `program-state.toml` with `tools/validate_program_state.py`;
7. confirm that only the next legal block is marked `READY`;
8. stop if package validation, repository identity, predecessor state, or authority ownership is ambiguous.

A web page, prompt summary, prior conversation, branch label, or short SHA is not authoritative repository state.

# 5. First-run rule

A fresh Revision 11 handoff authorizes **only `A0`**.

The orchestrator does not start `A1`, implementation tracks, or a broad repository rewrite in the same unreviewed act. It completes the `A0` evidence packet, updates program state, returns the result, and follows the maintainer's accepted sequencing process.

After `A0`, the orchestrator may continue through later blocks only when:

- every direct predecessor is accepted in program state, or the block is contingent `READY`/`IN_PROGRESS`/`REVIEW` work over an exact lower layer in the same validated stack snapshot;
- the block is explicitly `READY`;
- the exact charter and context packet are accepted;
- required permissions, branch/worktree, tests, and review mandates exist;
- no unresolved architecture deviation or competing owner change blocks it.

# 6. Context packet

Every non-trivial worker receives an immutable packet based on `templates/context-packet.md` containing:

- authority package and A6 lock digests;
- exact baseline/base/candidate identities;
- block, predecessors, charter digest, and stack position;
- current authority/readers/writers and source references;
- allowed write set and forbidden ownership changes;
- required commands, evidence, deletion set, output format, and stop conditions;
- one assigned role and review-independence constraints.

The packet is addressed by digest and stored with evidence. A worker does not receive a vague instruction to “follow the whole plan.”

# 7. Delegation policy

Delegation is used for substantial independent work, not ceremony.

Default limits until `A6` records another bounded policy:

- at most three active worker/subagent contexts under one orchestrator;
- one implementor owns one writable block branch at a time;
- one writable worktree per worker/branch;
- no subagent spawned merely to restate, summarize, or generically double-check the orchestrator's work;
- role-mandated independent reviews use clean contexts and direct evidence, not the implementor's conclusion.

The orchestrator may lower concurrency whenever shared owners, files, generated artifacts, tool resources, or benchmark machines would make parallel work unsafe or noisy.

# 8. Repository mutation and permissions

The orchestrator:

- never commits directly to a protected trunk unless repository policy explicitly makes that the accepted landing mechanism;
- never force-pushes an accepted, queued, or shared branch without invalidating affected state and reviews;
- never changes branch protection, required checks, secrets, credentials, release policy, or external service permissions as an implementation convenience;
- never exposes secrets in prompts, logs, evidence, commits, or review reports;
- treats generated files, lockfiles, submodules, vendored code, and benchmark artifacts as explicit write surfaces;
- uses one branch/worktree owner and records every mutation in the program ledger;
- leaves no untracked evidence or hidden local patch as part of an accepted claim.

# 9. Orchestrator output contract

For each block, the orchestrator emits a bounded record:

```text
BLOCK: <id>
STATE: ACCEPTANCE_RECOMMENDED | BLOCKED | RESCOPE_REQUIRED | FAILED
BASE: <full sha/tree>
CANDIDATE: <reviewed full sha/tree or none>
ACCEPTED_TARGET: <actual landed full sha/tree or none>
LANDING_EQUIVALENCE: <digest or none>
CHARTER_DIGEST: <digest>
CONTEXT_PACKET_DIGEST: <digest>
STACK: <id/layer or none>
CHANGES: <concise summary>
DELETIONS: <completed/missing>
EVIDENCE: <paths + digests + non-vacuous counts>
REVIEWS: <mandate/status/exact candidate>
DISCOVERIES: <disposition>
NEXT_LEGAL_BLOCKS: <derived from validated state>
MAINTAINER_DECISION_REQUIRED: <yes/no + exact decision>
```

The orchestrator never reports `PASS` or `LAND_READY` merely because it believes the design is good. It reports the actual validated state and missing proof.

# 10. Mandatory stop conditions

Stop and return `BLOCKED` or `RESCOPE_REQUIRED` when any of these occurs:

- package, manifest, DAG, program-state, charter, or performance-gate validation fails;
- actual checkout differs from the locked source and affected evidence has not been refreshed;
- dirty/unowned work or a competing PR/branch touches the same authority;
- a required command executes zero intended work or cannot be proven non-vacuous;
- current source disproves an architecture assumption;
- the true consumer/cutover closure is wider than the charter;
- a public/protocol/compatibility obligation is newly discovered;
- a benchmark premise or oracle is invalid;
- a worker requires forbidden permissions, ambient I/O, secret access, or destructive history changes;
- an atomic block would leave two production paths or an independently mergeable private layer;
- required independent review cannot be obtained.

The remedy is a source-backed deviation/rescope record, not a shim, hidden flag, weakened assertion, or unilateral architecture edit.

# 11. Maintainer-only decisions

The orchestrator may recommend but cannot self-authorize:

- package adoption/supersession;
- accepted ADR or architecture amendment;
- `A6` acceptance;
- gate weakening or benchmark premise recalibration;
- formal block rescope or DAG amendment;
- irreversible public compatibility/protocol policy;
- merge/land/release when repository policy requires maintainer approval.

# 12. Evidence custody

Raw command output, profiles, benchmark samples, generated manifests, review reports, range-diffs, landing-equivalence proofs, and lock records are stored at stable paths and addressed by digest. Summaries point to raw evidence and do not replace it.

A result created from a different SHA, tree, toolchain, profile, corpus, or stack base is not silently relabeled as current evidence.


---

# Consolidated source: `contracts/stacked-prs.md`

# Stacked Pull Request, Restack, and Atomic Landing Contract

**Status:** Normative delivery contract.  
**Architecture authority:** `program-dag.toml`, never PR topology.

# 1. Decision

Use a single PR when one bounded reviewable candidate is enough. Use a short stack when dependency ordering or an atomic cutover is materially easier to review in layers.

A stack is transport. It cannot add, remove, or reorder program predecessors, split a program acceptance unit silently, or make an incomplete cutover releasable.

# 2. Stack window and immutable snapshot

Before creating branches, copy `templates/stack-window.template.toml`, resolve it, and validate it with `tools/validate_stack_window.py`.

The mutable window records `program_state_basis_digest`, the digest of the last validated program state **before** that stack is attached. `previous_stack_snapshot_digest` links a restacked successor to the prior immutable snapshot, or is `NOT_APPLICABLE` for the first window. These one-way references avoid hash cycles.

Every layer has a unique `layer_id`; `block_id` names the program block whose charter/acceptance unit owns it. Every review and CI event binds to an immutable **StackSnapshotId**, defined as the SHA-256 of the fully resolved validated stack-window file. The file contains every exact base/head/tree, patch, generated-output, and evidence digest. Any changed input creates a new snapshot and invalidates affected approval.

After a snapshot is attached, current `program-state.toml` stores the stack ID, StackSnapshotId, and the program block's final/current layer index. Same-block private sublayers remain detailed in the stack window rather than becoming duplicate program-ledger blocks; an explicit program block such as `D1` retains its own `PRIVATE_CHECKPOINT` state. The pre-stack basis file is never rewritten. For `ACTIVE`, `FROZEN_FOR_REVIEW`, and `LAND_READY` windows, `tools/validate_stack_window.py --current-program-state ...` cross-validates the mutable ledger against the immutable snapshot. A PR label or web UI is not sufficient state.

A stack-window snapshot is not the landing receipt. Actual landing is recorded by the block's validated landing-equivalence proof and the subsequent program-state transition. A used or invalidated stack snapshot is retained as evidence and replaced by a new window when work remains.

# 3. Modes

## 3.1 `LANDABLE`

Use only when every layer is independently safe and releasable. The layers form one connected dependency path; true DAG siblings use separate windows. `acceptance_block_id` is empty and each `block_id` appears once.

Each layer:

- maps to one accepted block charter;
- has every external semantic predecessor accepted and may depend on an unaccepted predecessor only when that predecessor is a lower layer in the same validated snapshot;
- preserves one production implementation and every current Supported/Stable contract;
- passes its own charter checks on the cumulative tree;
- contains its own required deletions and compatibility handling;
- may land bottom-up without leaving a transitional public state.

An upper layer may be `READY`, `IN_PROGRESS`, or `REVIEW` while a lower in-window predecessor is unaccepted. It may not become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until every semantic predecessor is formally `ACCEPTED`/`PRIVATE_CHECKPOINT`, the lower landing has occurred, the upper layer has been restacked on the new base, and affected checks/reviews have revalidated the new exact candidate.

An upper layer is never accepted merely because the top of the stack is green.

## 3.2 `ATOMIC_REVIEW`

Use when several review-sized diffs collectively form one indivisible clean cutover.

Rules:

- top-level `acceptance_block_id` names the sole program block that may become accepted/landed from this window;
- all layers belong to that atomic acceptance unit and have unique `layer_id` values;
- private layers may repeat the acceptance block's `block_id` as internal checkpoints, or name an explicit `foundational-private-checkpoint` predecessor such as `D1`;
- intermediate layers target a private integration branch, remain draft, are marked `NON_MERGEABLE_PRIVATE_LAYER`, and are unreachable from production entry points;
- no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor, except an explicit program checkpoint such as D1 whose `PRIVATE_CHECKPOINT` state is valid only for the final acceptance block;
- exactly one final mergeable layer routes every consumer, deletes the displaced path/support machinery, and becomes the reviewed candidate;
- the complete combined tip receives the block's required conformance, architecture, and adversarial/performance review;
- landing preserves the exact reviewed candidate delta on the recorded landing base.

`D1`/`D2` is the canonical case: D1 is a private checkpoint; D2 is the sole acceptance and landing unit.

## 3.3 Parallel disjoint work

Parallelism is represented by separate `LANDABLE` or `ATOMIC_REVIEW` windows, not one artificial stack. Each window declares shared owners/files/generated artifacts and integration tests. A newly discovered ownership overlap stops or serializes the affected work.

# 4. Size and lifetime

Default maximum: **four open review layers per stack**.

A6 locks a value from two through six based on reviewer capacity, CI latency, restack frequency, and repository tooling. More than six requires an ADR amendment. A program-wide or fifty-block stack is prohibited.

Split or land a stack when:

- the lowest layer is independently acceptable;
- owner or concern changes;
- a block boundary is crossed without independent acceptance;
- lower-layer churn repeatedly invalidates upper proof;
- the review scope cone no longer fits one bounded invariant;
- the final atomic cutover is no longer understandable as one candidate.

# 5. Branch/worktree ownership

- One writable worktree belongs to one branch and one worker.
- A worker may read but not mutate another worker's worktree.
- Only the orchestrator changes shared stack topology, rebases/restacks shared branches, or resolves cross-layer conflicts.
- Generated files, lockfiles, protocol schemas, central manifests, and dependency-firewall configuration have one active writer lease.
- Branch/PR metadata includes stack ID, mode, layer index, block ID, exact base/head/tree, charter digest, and snapshot digest.
- Accepted evidence never depends on uncommitted or untracked changes.

# 6. CI and proof

Every mergeable layer runs:

1. layer-specific tests/static checks;
2. every charter check applicable to the cumulative tree from stack root through that layer;
3. non-vacuous execution and generated-file cleanliness proof;
4. relevant performance/memory/work gates;
5. dependency, architecture, compatibility, and failure checks required by its block.

The top `LANDABLE` layer additionally runs the declared stack-integration suite. The final `ATOMIC_REVIEW` layer runs full atomic-cutover proof, including one-production-path and deletion assertions.

`LAND_READY` means all mergeable layers are green on the named immutable snapshot and the one currently eligible landing block is `ACCEPTANCE_RECOMMENDED`: the bottom layer for `LANDABLE`, or the final acceptance block for `ATOMIC_REVIEW`. Green upper `LANDABLE` layers remain `REVIEW`, not accepted in advance.

Evidence from an older base, snapshot, toolchain, profile, or corpus is not silently reused.

# 7. Lower-layer change and cascading restack

When a lower layer changes:

1. fix the layer where the defect belongs; never hide it in an upper workaround;
2. restack bottom-to-top;
3. record old/new base SHA/tree, canonical patch digest, range-diff, candidate tree, generated diff, evidence digest, and every manual conflict resolution;
4. set `previous_stack_snapshot_digest` to the replaced snapshot and mint a new StackSnapshotId;
5. mark all affected upper candidates/reviews `INVALIDATED`/revalidation-required;
6. rerun required CI on every new cumulative tree;
7. obtain impact-bounded reattestation from every required review mandate on the new exact candidate/snapshot.

No approval transfers automatically. Tree/patch equivalence can make reattestation small, but the new exact identity must be named.

# 8. Restructuring

Inserting, dropping, folding, reordering, or unstacking a layer requires:

- clean worktrees;
- no affected layer queued/merging;
- updated validated stack window and program state;
- predecessor and mergeability revalidation;
- regenerated PR descriptions/context packets where affected;
- invalidation of changed cumulative candidates, CI, and reviews.

A transport-only linear relation between true DAG siblings must not be created. Separate sibling stacks are required.

# 9. Landing

Legal modes:

- **Bottom-up:** land only the lowest `LANDABLE` layer. Then invalidate/restack every remaining upper layer on the actual accepted base and issue a successor snapshot. If one layer remains, continue as an ordinary single PR.
- **Atomic final only:** land only the final `ATOMIC_REVIEW` candidate; private layers never reach trunk independently.

Before landing, record the reviewed base/candidate SHA/tree and the predicted landing base/target identity. Branch protection and required checks remain binding. A merge queue is preferred where available, but queue admission does not replace exact review.

`candidate_sha/tree` remains the exact cumulative candidate reviewers inspected. `accepted_sha/tree` records the actual landed commit and full repository tree and may differ after a reviewed rebase, squash, merge commit, or merge-queue base advance. A validated `landing_equivalence_digest` proves that the canonical binary Git delta from reviewed base to reviewed candidate exactly equals the delta from accepted base to accepted commit, that generated-output digests match, that no manual conflict resolution occurred after review, and that required post-landing checks passed. If the delta differs, re-freeze and re-review; do not call it equivalent.

A single accepted program block must not be co-batched with unrelated changes in the same landing delta. Foundational or atomic candidates receive a dedicated merge-group/queue position where the repository supports it.

After landing:

- validate `templates/landing-equivalence.template.toml` against the actual repository objects;
- record the proof file's digest in the post-landing program state;
- run required post-merge smoke/consistency checks;
- record actual accepted SHA/tree;
- clear or invalidate every unlanded upper block's old stack binding/review state;
- create a successor stack snapshot from the new validated program-state basis when multiple upper layers remain;
- retain evidence before pruning branches/worktrees.

A stack snapshot itself never transitions a block to accepted. A merged PR is not automatically an accepted block.

# 10. Tooling independence

GitHub native stacks, ordinary dependent PRs, or another reviewed tool may implement this contract. Native stack UI and CLI behavior are operational conveniences, not architecture dependencies.

# 11. Prohibited patterns

- one stack spanning the complete program;
- merging a private replacement checkpoint to trunk;
- an upper-layer workaround for a lower-layer defect;
- two writers force-pushing or editing one branch;
- preserving approval after an unrecorded restack;
- relying only on top-of-stack CI for independently mergeable lower layers;
- merging a layer that leaves two selectable production paths;
- using stack position as semantic predecessor authority;
- allowing an upper block to reach acceptance before its semantic predecessor lands;
- treating full-tree inequality after a base advance as automatic failure or automatic equivalence instead of proving exact candidate-delta equivalence;
- hiding cross-stack dependencies or an unaccepted competing PR.


---

# Consolidated source: `contracts/architecture-falsification.md`

# Architecture Premise and Falsification Contract

**Status:** Normative.  
**Owner:** architecture authority, A6 lock, and deviation governance.

# 1. Purpose

The program optimizes for the best final system, not for loyalty to a document. It must distinguish rules that define the desired product from implementation tactics that are selected by evidence.

No implementor or orchestrator may claim “best possible” merely because a design is detailed. The accepted claim is that the target is the strongest defensible architecture under the current evidence and that every evidence-sensitive premise has a falsification path.

# 2. Decision classes

## 2.1 Constitutional invariant

A constitutional invariant defines correctness, authority, product behavior, trust, or lifetime boundaries. Examples include:

- one semantic authority for one operation;
- unsupported work cannot publish as complete;
- direct compilation does not depend on managed/session/provider ownership;
- exact authoritative inputs and profiles determine observable semantics;
- shared published artifacts are immutable;
- retained state is bounded and reclaimable;
- a clean cutover leaves one production path;
- public/wire/persistent compatibility has an explicit owner;
- required mapping correctness is not weakened to save optional source-map work.

Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.

## 2.2 Repository fact or product obligation

A repository fact describes the current tree, current consumers, current compatibility obligations, current toolchain, or current product maturity. It must be established by A0–A6 evidence and may change when the checkout changes.

Examples include:

- which parser currently owns an operation;
- whether an external protocol has released consumers;
- which Svelte operations are supported;
- which open PRs alter architecture-affecting boundaries;
- which commands actually execute non-zero tests;
- current competitor versions and benchmark corpus.

A false or changed fact stops affected work and refreshes the baseline/lock. It does not justify a compatibility shim or second owner.

## 2.3 Evidence-selected tactic

A tactic is a private or operational choice whose alternatives preserve every constitutional invariant and public contract. Examples include:

- Rayon versus another mature bounded executor;
- exact inline/fork threshold and chunk size;
- cache admission score and store shard count;
- arena pooling limits;
- compact index layout;
- bounded PR stack depth within the stack contract;
- whether a derived projection is retained after measured reuse;
- a specific data structure with equivalent semantics and lifetime.

A tactic may change inside a ratified charter when the charter permits it and all locked gates remain unchanged. It must not be promoted into a public abstraction without a real consumer/compatibility reason.

## 2.4 Deferred research choice

A deferred research choice is intentionally unapproved until simpler architecture is measured and found insufficient. Examples include mutable within-unit parsing, a custom executor, specialized lock-free containers, allocator replacement, PGO, or SIMD.

It cannot enter a candidate under “future proofing.” It requires its own measured block and accepted premise update.

# 3. Premise ledger

A6 accepts one premise ledger. Every architecture-significant or performance-significant premise records:

```text
Premise ID:
Decision class: constitutional | repository-fact | tactic | deferred
Statement:
Evidence/source:
Confidence and uncertainty:
Falsification test or trigger:
Allowed alternatives:
Blocks and gates affected:
Action when falsified: refresh | rescope | ADR amendment | abort
Owner:
```

The ledger must include at minimum:

- current owner/consumer and compatibility assumptions;
- direct-versus-managed cost premises;
- parse reuse and retention premises;
- query/flight/cache premises;
- flow coverage and oracle premises;
- CSS reuse premises;
- provider/capability premises;
- executor/task-grain premises;
- memory plateau and allocator-slack premises;
- PR stack/reviewer-capacity premises.

# 4. Falsification behavior

When evidence contradicts a premise:

1. stop affected implementation before adding a workaround;
2. record the exact evidence and affected class;
3. preserve work whose correctness is independent of the premise;
4. refresh a repository fact, change an allowed tactic, or open an ADR amendment as required;
5. invalidate downstream charters, gates, or approvals whose basis changed;
6. resume only from a new exact authority/lock state.

Disallowed responses include hiding the contradiction behind a flag, alternate semantics path, unbounded cache, broader trait, silent fallback, reduced test, or post-result gate relaxation.

# 5. Architecture challenge

For every Foundational block, the independent architecture challenger must attempt to disprove:

- the current owner inventory;
- the claimed minimal prerequisite closure;
- the necessity of every new owner/cache/queue/trait/version;
- lifetime and thread-affinity assumptions;
- failure and cancellation completeness;
- performance comparison equivalence;
- deletion closure;
- the classification of each contested premise.

A challenge passes when no blocking contradiction remains, not when the challenger merely prefers the proposal.


---

# Consolidated source: `contracts/package-publication.md`

# Authority Package Publication Contract

**Status:** Normative package/release contract.  
**Owner:** architecture authority distribution and A0 package verification.

# 1. Canonical source

The unpacked `verter-architecture-v11/` source tree is the canonical package content. `MANIFEST.json` names every package file except itself and records bytes, lines, and SHA-256. `VALIDATION.json` records structural validation, not repository implementation proof.

The consolidated master document, ZIP, validation report, and checksum file are generated release artifacts. They are never edited independently and never override the source package.

# 2. One release set

One Revision 11 release set consists of:

- `verter-architecture-v11.zip`;
- `verter-architecture-lock-master-plan-v11.md`;
- `verter-architecture-v11-validation-report.md`;
- `verter-opus-orchestrator-prompt-v11.md`, generated byte-for-byte from `agents/opus-bootstrap.md`;
- `verter-architecture-v11.sha256`.

All artifacts must name the same source-package digest. Publishing two different trees under the same revision is prohibited.

# 3. Build law

`tools/build_release.py` is the only supported release path. It must:

1. run structural validators before minting evidence;
2. refresh `VALIDATION.json` and `MANIFEST.json` only after structural checks pass;
3. rerun package validation without write mode;
4. validate the program-state, stack-window, performance-gate, and landing-equivalence templates;
5. run the live positive/negative orchestration self-test suite;
6. compile all bundled Python tools;
7. generate the consolidated document from exact manifest-listed sources;
8. generate a deterministic sorted ZIP with fixed metadata;
9. test ZIP integrity and the single top-level package directory;
10. extract the ZIP into a clean temporary directory;
11. run the validator bundled inside the extracted package;
12. compare the complete extracted and source manifests;
13. rebuild the consolidated document and ZIP and require byte equality;
14. export the Opus bootstrap prompt byte-for-byte from the canonical package;
15. emit a deterministic release report and checksums covering every distributed artifact except the checksum file itself.

A release builder failure is release-blocking.

# 4. Self-reference boundaries

`MANIFEST.json` excludes itself so it can describe a stable package digest. Generated release artifacts live outside the package and are not inputs to that digest. The release checksum file does not hash itself.

The README points to `MANIFEST.json`; it does not embed a value that would make the package digest self-referential.

# 5. Determinism

With identical source bytes and supported Python behavior, the ZIP and consolidated document must be byte-deterministic. The ZIP uses:

- lexical path order;
- one top-level directory;
- fixed timestamps;
- fixed permissions;
- deterministic compression settings;
- no host absolute paths, owner IDs, or temporary files.

# 6. A0 verification

Before repository work, A0:

- verifies the checksum file;
- extracts the ZIP cleanly;
- runs the bundled package validator;
- runs `tools/selftest_orchestration.py`;
- records the manifest package digest in `program-state.toml` and the baseline lock;
- rejects any consolidated/ZIP/manifest mismatch.

A long master document without a matching validated package is reference convenience, not implementation authority.


---

# Consolidated source: `contracts/baseline-lock.md`

# Baseline Lock Contract

**Status:** Normative implementation-entry contract.  
**Package state:** `A0` locks the entry checkout. No implementation baseline is locked until `A6` accepts the exact post-Gate-0 lineage and refreshes all affected evidence.

# 1. Rule

Gate 0 uses two explicit source points:

- `EntryCheckoutSha`, captured at `A0`, proves what was initially inspected;
- `ImplementationBaselineSha`, accepted at `A6`, is the exact post-command-fix, post-harness, post-safety, post-instrumentation lineage on which later charters and baselines rely.

No non-local architecture cutover begins without the `A6` implementation baseline lock. A branch name, short SHA alone, remote webpage, “latest main,” or “architecture-equivalent successor” is insufficient.

The historical `9af553dd…` evidence file describes the source used to design the plan. It is not permission to implement against that SHA or a current branch without verification.

# 2. Required lock record

```toml
schema_version = 0
status = "LOCKED"

[repository]
remote = "https://github.com/pikax/verter.git"
branch = "main-or-explicit-branch"
entry_checkout_sha = "FULL_40_HEX_SHA_FROM_A0"
implementation_baseline_sha = "FULL_40_HEX_POST_GATE0_SHA_ACCEPTED_BY_A6"
implementation_baseline_tree = "GIT_TREE_OID"
short_sha = "SHORT_IMPLEMENTATION_SHA"
dirty = false
untracked_count = 0
submodule_state = "none-or-exact-state"
open_architecture_changes = ["PR/branch/commit and disposition"]

[locks]
cargo_lock_sha256 = "..."
pnpm_lock_sha256 = "..."
other_lockfiles = []

[toolchain]
rustc = "..."
cargo = "..."
nextest = "..."
node = "..."
pnpm = "..."
platform = "..."
architecture = "..."

[verification]
canonical_rust_pair_proven = true
typescript_builds_proven = true
napi_proven = true
wasm_proven = true
corpus_commands_proven = true
raw_evidence_uri = "..."
```

Record exact external TypeScript/provider/framework/compiler versions used by affected tests and benchmarks.

# 3. Open-change disposition

Before freezing, inventory open or queued architecture-affecting changes touching compiler, syntax, semantic, cache, input/snapshot, provider, CSS, framework, protocol, or public API boundaries. For each, choose:

- include before freeze;
- exclude and rebase/reconcile later;
- abandon;
- explicitly coordinate as a predecessor/dependent block.

Do not implement a new architecture while an unaccounted parallel change is rewriting the same owner.

# 4. Canonical command proof

For every canonical command record:

- exact command and working directory;
- environment/features;
- exit code;
- executed test/case count;
- skipped/ignored count;
- exact binaries/packages/fixtures;
- raw output digest.

A green command that executed zero intended work is a failure.

# 5. SHA change and record location

The accepted lock record is an immutable evidence artifact. It is not required to contain the SHA of a commit that embeds the record itself; `implementation_baseline_sha` names the exact code/evidence candidate evaluated by A6, and the record is addressed by its own digest. A later documentation-only commit that stores the record does not silently become the implementation baseline.

Any implementation-baseline SHA change requires:

1. a new lock record;
2. an architecture-affecting diff;
3. refreshed `current-tree-reconciliation.md` rows;
4. rerun non-vacuous command proof;
5. review of affected measurements, gates, and charters.

Unaffected historical evidence may remain cited but never silently substitutes for current source.


---

# Consolidated source: `contracts/current-tree-reconciliation.md`

# Current-Tree Reconciliation Contract

**Status:** Normative implementation-entry contract.  
**Purpose:** prevent a neutral target name from accidentally creating a second owner beside a correct current owner.

# 1. Required disposition

Each affected current authority receives exactly one disposition:

- **Preserve** — already owns the final invariant; adapt callers/tests only as necessary.
- **Converge** — survives but absorbs/removes adjacent duplicate responsibility.
- **Replace** — a new owner is justified; old owner and every caller are deleted/migrated in the same accepted cutover.
- **Delete** — responsibility is unnecessary or already owned elsewhere.
- **Defer** — outside the current block; block must prove it does not depend on changing it.

A row marked `VERIFY` blocks the affected implementation block.

# 2. Mandatory row schema

| Surface/current symbol | Source paths | Current invariant/authority | Direct readers/writers/callers | Lifetime/thread boundary | Cache/protocol compatibility | Target disposition | Final owner | Exact deletion/migration set | Proof block | Status |
|---|---|---|---|---|---|---|---|---|---|---|

# 3. Seed inventory from the historical `9af…` evidence baseline

The following rows are hypotheses derived from the historical evidence baseline. Each must be source-verified and expanded against the exact `A0` checkout; they are not claims about current `main`.

| Surface/current symbol | Candidate current authority | Revision 11 constraint | Initial status |
|---|---|---|---|
| open PRs/branches/queued changes touching an architecture owner | parallel architecture-affecting work | include, exclude, abandon, or coordinate before baseline lock; no unaccounted competing rewrite | VERIFY |
| registered source/VFS/`PublishedRoot`/workspace snapshot | host-backed source, project, invalidation, and publication basis | preserve or converge into the single committed-input role before QueryRuntime convergence; do not create a second `InputStore` by name alone | VERIFY |
| `verter_session::resolver_core` / `ProjectSemanticDispatch` | shared host-backed module/type-resolution orchestration | preserve one resolver semantics path; extraction may change dependency direction but cannot create a second resolver | VERIFY |
| `IndexedReady` and shallow symbol inventory | canonical shallow declaration/index artifact | preserve demand-driven broad shallow index if source proof matches; no rescanning to rediscover indexed facts | VERIFY |
| `DeclBodyMemo` / retained parse workers / `DeclLoweringService` | lazy body lowering over retained parse snapshots | reconcile into managed parse owner domains; direct compiler remains independent | VERIFY |
| `ProjectTypeStore`, `RouteDb`, fact/read-set caches | current query/cache families | classify each cache separately; preserve value-side validation where correct; delete duplicate ownership only after proof | VERIFY |
| `SemanticGraphStore` and component-meta materialization caches | managed semantic/component-meta storage | reconcile lifetime, cohort, lock, admission, and current native/compat consumers before the public TypeExpr/operation-DTO cutover | VERIFY |
| `FunctionProgramIndex`/`FunctionFlowGraph` | canonical flow structure | PRESERVE unless source evidence disproves; extend same graph only | VERIFY |
| `flow_slice_content.rs` syntax-shaped evaluator | second flow/control semantics path | REPLACE/DELETE through final flow blocks; do not port it as a new IR | VERIFY |
| `CodeTransform` | code plus mapping transformation authority | preserve atomic code/mapping semantics and reuse in compact source-unit cutover | VERIFY |
| `StyleSyntaxIr` and current fast CSS paths | CSS-family syntax/transform substrate | preserve one syntax authority; do not delete a proven specialized fast path without equivalent-work evidence | VERIFY |
| component-meta native/compat boundary | product-facing compatibility behavior | inventory consumers/oracles and migrate after final semantic/flow plus the affected consumer identity/lifetime/admission contracts; no silent behavior merge | VERIFY |
| ProviderHub/SyncCoordinator/provider actors | external TypeScript lifecycle/synchronization | preserve stateful actor ownership where required; converge stamps/readiness, never race providers | VERIFY |
| `VerterHost` / session facade | current public/catch-all entry owner | reduce only after every extracted invariant has a complete owner and consumer migration | VERIFY |
| `TypeExpr` producers/consumers and TypeInfo protobuf graph | current internal/public/wire contracts | consumer-by-consumer disposition after flow/query foundations; wire obligations explicit | VERIFY |
| audit TLS/substrate/runtime | deterministic optional observability | preserve leaf dependency direction and prove disabled overhead; do not make audit semantic authority | VERIFY |

# 4. Completion rule

Before a block enters `BLOCK_READY`:

- every touched row is resolved and linked to source evidence;
- every direct consumer is named, not represented by “and others”;
- the surviving owner and dependency direction are explicit;
- compatibility/wire implications are classified;
- the exact old declarations/caches/tasks/tests/docs to delete are listed;
- current behavior that is intentionally preserved has a characterization test;
- unresolved adjacent rows are proven outside the block's causal closure.


---

# Consolidated source: `contracts/capability-matrix.md`

# Framework and Product Capability Matrix

**Status:** Normative product truth after baseline lock.  
**Current state:** template rows marked `VERIFY`; A1 establishes initial command/product truth, A3 updates any fail-closed behavior, and A5/A6 finalize the exact post-safety matrix. Affected product blocks cannot start until completed.

# 1. Row schema

| Framework/product | Operation | Route/backend | Maturity | Default | Semantic profile(s) | Oracle/conformance corpus | Exact unsupported/degradation behavior | Zero-work negative proof | Compatibility promise | Status |
|---|---|---|---|---|---|---|---|---|---|---|

# 2. Seed rows

| Framework/product | Operation | Route/backend | Maturity | Default | Semantic profile(s) | Oracle/conformance corpus | Exact unsupported/degradation behavior | Zero-work negative proof | Compatibility promise | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| Vue | runtime compile | direct Rust | VERIFY | VERIFY | VERIFY | official Vue fixtures + Verter corpus | VERIFY | no IDE/public/native enrichment work | VERIFY | VERIFY |
| Vue | IDE companion | managed/provider | VERIFY | VERIFY | provider-specific | provider + mapping corpus | typed route/capability failure | no runtime constructor projection unless demanded | VERIFY | VERIFY |
| Vue | imported macro runtime projection | CompileTypeInfo | VERIFY | VERIFY | supported normalized profiles | official/compiler-sfc differential | typed degradation/unresolved input | unrelated object members not traversed | VERIFY | VERIFY |
| Svelte | native runtime compile | direct Rust | Experimental (verify current pin) | VERIFY | syntax/toolchain profile | pinned Svelte compiler corpus | typed unsupported/experimental behavior | zero Vue/native compile projection | experimental | VERIFY |
| TypeInfo | `TypeAtPosition` | native | VERIFY | VERIFY | normalized TS profiles | selected TS oracle | typed partial/gap/no-value | no-flow allocates no graph/plan | VERIFY | VERIFY |
| TypeInfo | graph export | public/wire | advanced explicit | off unless requested | profile stamped | protocol/round-trip corpus | size/depth/unsupported failure | simple DTO operations serialize no graph | named compatibility domain | VERIFY |
| LSP | external TypeScript provider | project binding | VERIFY | `auto`/explicit per product | provider profile | capability matrix | actionable incompatible route; no race/fallback | disabled native enrichment is zero-work | provider epoch/profile stamped | VERIFY |
| CSS | parse/format/index/transform | native/external by dialect | VERIFY | VERIFY | dialect profile | dialect/framework corpus | typed unsupported/recovery-incomplete | identical bytes parsed once per residence | VERIFY | VERIFY |

# 3. Rules

- A missing/`VERIFY` row means the capability is not approved for architecture claims or default changes.
- Maturity is operation-specific; framework citizenship does not imply equal maturity.
- Changing a default or compatibility promise requires product/conformance review.
- Experimental behavior cannot be silently used as a stable oracle for another surface.
- Every enabled row links exact tests and benchmark cells.
- Unsupported and partial behavior is part of the public contract, not an implementation accident.


---

# Consolidated source: `contracts/semantic-profile.md`

# Profile and Policy Classification Contract

**Status:** Normative identity-classification contract.

# 1. Distinct classes

| Class | Meaning | Examples | Reuse/key consequence |
|---|---|---|---|
| `TypeScriptSemanticProfileId` | interpretation/compatibility semantics | strictness, nullability, exact optional property behavior, module/resolution semantics, JSX/type-language rules, selected TypeScript compatibility family | semantic query, compile projection, and semantic facts |
| `OutputProfileId` | generated program semantics/shape | client/server target, dev/prod semantics, feature transforms, framework/compiler target | compile plan and generated artifact |
| `PresentationProfileId` | human-facing rendering only | display flags, path-display policy, diagnostic text locale/presentation version | rendered text/diagnostic materialization only |
| `SerializationProfileId` | wire/encoding contract | schema/domain, canonical encoding, graph export format, field policy | serialized bytes only unless decoding compatibility affects use |
| `ResultContractId` | observable complete-result requirement | operation, exactness, capability, unsupported policy, requested approximation, required mapping/diagnostic/serialization outcome | semantic flight/cache compatibility |
| `ExecutionPolicy` | waiter-local resource/scheduling limits | deadline, cancellation, priority, work/time/memory budget | never changes complete result identity; exhaustion is partial/failure |

A field belongs to the earliest class whose observable meaning it can change. It is never copied into every class “for safety.”

# 2. Closed semantic-profile schema

The implementation defines a canonical typed schema. Every behavior-affecting compiler option is classified in a reviewed table as:

- semantic;
- output;
- presentation;
- serialization;
- execution-only;
- irrelevant to the operation;
- unsupported.

Unknown fields or unsupported values fail closed. “Private fields as applicable” is not an acceptable profile definition.

# 3. Canonicalization

- normalize equivalent forms before hashing;
- domain-separate encodings by class and compatibility domain;
- include field tags and canonical value encoding;
- exclude host path strings, map iteration order, timestamps, random seeds, and process-global defaults;
- collision-sensitive IDs use full fingerprints or verified equality;
- canonical bytes and schema/domain identity are test vectors.

# 4. Cross-class rules

- presentation changes do not invalidate semantic facts or generated code when no presentation is requested;
- serialization changes do not rerun semantic computation when the typed result remains available;
- output changes do not silently change semantic interpretation;
- execution budget changes cannot produce a different value labeled `Complete`;
- a provider/framework/compiler compatibility change belongs to its named domain and is included wherever it affects meaning.

# 5. Required tests

- every public/config option is classified exactly once;
- semantically equivalent configurations yield equal canonical IDs;
- one-field semantic changes invalidate semantic reuse;
- presentation-only changes reuse semantic facts but rerender;
- serialization-only changes reuse typed results but re-encode;
- execution-policy changes do not change complete output bytes;
- unknown fields/values fail closed;
- native, prepared, managed, and WASM implementations use the same canonical test vectors.


---

# Consolidated source: `contracts/identity-encoding.md`

# Canonical Identity Encoding Contract

**Status:** Normative identity/fingerprint encoding contract.  
**Binding ADRs:** ADR-002, ADR-004, ADR-012, ADR-016.

# 1. Identity authority versus digest

Typed descriptors define identity. A digest is an index/fingerprint of the canonical descriptor encoding and never replaces equality material where collision would be correctness-sensitive.

Each digest is domain-separated by a stable namespace and compatibility epoch.

# 2. Canonical encoding

Unless an accepted external protocol requires another encoding, identity descriptors use a tagged, length-delimited byte form:

```text
u32 domain_tag_length little-endian
bytes domain_tag UTF-8
u32 field_count little-endian
repeat fields in schema order:
    u16 field_tag little-endian
    u64 payload_length little-endian
    payload
```

Rules:

- fixed explicitly assigned enum discriminants;
- fixed-width integer encoding;
- booleans `0`/`1`;
- explicit present/absent tag for optionals;
- sets sorted by canonical element bytes;
- maps sorted by canonical key bytes and duplicate canonical keys rejected;
- strings use exact UTF-8 bytes; no implicit Unicode normalization;
- paths are already normalized through the captured project/source authority before encoding;
- schema changes bump the compatibility epoch or create a new domain;
- ad hoc delimiter concatenation, debug formatting, unordered JSON, and declaration-order enum hashing are prohibited.

# 3. Stable IDs

`StableEntityId` derives from documented domain-separated canonical identity material and is independent of allocation, traversal schedule, worker, cache history, or interner insertion.

Collision-sensitive use performs full descriptor equality or carries a deterministic disambiguator. Silent aliasing is prohibited.

`SessionHandle` is not a stable ID. It includes/validates owner cohort and generation and cannot be serialized as a stable reference unless an explicit protocol translates it to stable identity.

# 4. One-time normalization

Authoritative owners compute source, options, project, and profile descriptors once per revision/basis. Hot keys pass compact typed IDs rather than repeatedly canonicalizing paths, hashing unchanged bytes, or normalizing options.

# 5. Tests

- golden canonical bytes and digests per domain;
- permutation invariance for sets/maps;
- enum/schema evolution tests;
- path/source normalization basis tests;
- randomized hash seed/schedule/worker equality;
- collision injection/full-equality behavior;
- stable ID versus session handle misuse compile tests;
- native/WASM canonical encoding equality.


---

# Consolidated source: `contracts/mapping-products.md`

# Source Unit and Mapping Product Contract

**Status:** Normative generated-artifact interpretation contract.  
**Binding ADR:** ADR-010.

# 1. Separate mapping products

The architecture distinguishes:

1. `PlacementMap` — internal source-unit placement/composition;
2. `SourceProjectionMap` — required to interpret an IDE/provider companion;
3. `RuntimeSourceMapData` — optional runtime/build map segments;
4. `EncodedSourceMap` — terminal external serialized map.

These are different identities and products. A single “maps enabled” boolean is insufficient at architecture/API/benchmark level.

# 2. Source units

Framework frontends produce logical script/template/style/custom units with:

- stable logical lineage ID;
- exact source revision and placement;
- exact content/syntax identity;
- unit-relative spans wherever source-neutral reuse is claimed;
- deterministic unit/product order.

Moving unchanged bytes may preserve source-neutral syntax/semantic artifacts and rebuild only placement-dependent composition.

# 3. Atomicity

Generated code publishes atomically with every mapping required to interpret that exact code.

- an IDE companion requiring `SourceProjectionMap` cannot be Ready/published without it;
- runtime code without a requested runtime source map constructs no `RuntimeSourceMapData` or encoding;
- requesting encoded output may require map data and encoding as explicit terminal prerequisites;
- an operation requiring no map constructs no universal empty map.

# 4. Identity and ordering

Every map is bound to exact source/unit revision, generated artifact, output profile, and map compatibility domain. Segments use canonical deterministic ordering and reject overlap/ambiguity according to the product contract.

Map encoding/serialization identity is separate from semantic/generated code identity. Changing JSON field order or encoded format does not invalidate semantic/code computation when map data is unchanged.

# 5. Correctness

Tests cover:

- source-to-generated and generated-to-source round trips;
- inserted/deleted/moved unchanged units;
- multi-unit composition and boundary positions;
- Unicode/byte offset conventions;
- synthetic/helper ranges and unmapped segments;
- diagnostics/navigation/rename through IDE maps;
- runtime source maps on/off and terminal encoding;
- stale map paired with new code rejected;
- direct/prepared/managed equality;
- one construction/encoding per requested product identity.


---

# Consolidated source: `contracts/parse-ownership.md`

# Parse Key, Ownership, Affinity, and Reparse Contract

**Status:** Normative cross-regime syntax ownership contract.

# 1. Exact construction identity

```rust
struct ParseKey {
    content: ContentId,
    language: LanguageId,
    syntax_contract: SyntaxCompatibilityId, // named domain + monotonic epoch
    syntax_profile: SyntaxProfileId,        // normalized parse/recovery/source-type options
}
```

`ParseKey` contains only dimensions that can change the constructed syntax result. It does **not** contain consumer names such as runtime, IDE, formatter, TypeInfo, or codegen. A consumer-specific difference is valid only when it changes the normalized syntax profile or the named syntax compatibility contract.

# 2. Owner domain

```rust
enum ParseOwnerDomainId {
    DirectInvocation(DirectInvocationId),
    DirectBatch(DirectBatchId),
    Prepared(PreparedCarrierId),
    Managed(ManagedParseOwnerId),
}

struct ParseInstanceId {
    owner_domain: ParseOwnerDomainId,
    key: ParseKey,
    generation: ParseGeneration,
}
```

The invariant is:

> One live `(ParseOwnerDomainId, ParseKey)` has one owner and one active result.

This is deliberately not a process-global invariant. Independent one-shot direct calls do not acquire a hidden global parse cache. A direct batch may share within its explicit batch owner. `PreparedCarrier` shares only inside the retained value. The managed engine may retain under a bounded owner/shard.

# 3. Shared frontend

Consumers with the same `ParseKey` within one owner domain reuse the same error-tolerant frontend result. A runtime/IDE distinction cannot justify a second parser. Derived indexes or views may differ but must identify the same parse instance and cannot override syntax meaning.

# 4. Affinity

- OXC allocator/AST and local mutable parse state remain on their owner.
- An already-owner-local consumer executes inline.
- A foreign worker sends a compact owned owner-call descriptor; the AST never crosses the boundary.
- No unsafe `Send`/`Sync` implementation hides an ownership mismatch.
- Only OXC-free compact values cross a general CPU executor boundary.

# 5. Retention and reparse

- direct invocation: request-local and dropped at return;
- direct batch: bounded by the explicit batch lifetime;
- prepared: pinned by the caller-retained `PreparedCarrier`, with inspectable retained weight;
- managed: byte-weighted, pressure-evictable, with explicit live pins.

After explicit eviction in a retaining domain, a later demand may start at most one same-key reparse flight. Reparse count and cause are observable. Retaining an index/graph does not implicitly retain the parse arena.

# 6. Locators

A locator carries enough identity to reject stale/wrong parse access:

```text
ParseKey
owner-compatible source/unit identity
node kind
span or canonical local ordinal
optional structural fingerprint
```

Lookup validates key, source/unit identity, node kind, bounds, and generation. Failure is typed; it never reads a same-span node from a different parse.

# 7. Required tests

- runtime and IDE consumers in one domain invoke the parser once;
- genuinely parse-affecting options create distinct keys;
- two independent direct calls do not share hidden state;
- prepared repeat reuses one parse until drop;
- managed pressure eviction produces one visible same-key reparse flight;
- graph/index retention does not pin the arena;
- stale locator and wrong generation fail deterministically;
- native threaded, native single-thread, and WASM/local profiles produce equal declared outputs.


---

# Consolidated source: `contracts/compile-transaction.md`

# Staged Compile Transaction and Anti-Replay Contract

**Status:** Normative direct/prepared/project-aware compiler protocol.

# 1. Stages

```text
prepare(source, parse options)
  -> PreparedCarrier
plan(prepared, CompileRequest)
  -> CompilePlan
project(plan.projection_batch, CompileTypeInfo)
  -> CompleteFacts | NeedInputs(LoadSet) | TerminalFailure
emit(prepared, plan, complete facts)
  -> CompileResult
```

Planning discovers the complete product prerequisite closure and closed semantic projection batch before projection. Emission does not discover new project-semantic demands. A new demand requires a new plan.

# 2. Binding tokens

`CompilePlanToken` binds the complete request for anti-replay. The plan additionally owns narrower deterministic tokens:

```text
ProjectionPlanToken   root + semantic profile + closed demand batch + kernel/projection domains
ProductSubplanToken   root + framework/product + output profile + required product mapping contract
TerminalSubplanToken  exact typed result/product + requested presentation/provenance/encoding profile
```

The whole `CompilePlanToken` binds at least:

- prepared root/source/unit identities;
- parse key/domain and relevant generation;
- normalized canonical typed product-request collection;
- shared semantic profile when observed;
- each product's output and requested terminal presentation/mapping/provenance/serialization profiles;
- framework/compiler compatibility identity;
- projection schema/domain;
- plan algorithm compatibility domain;
- deterministic demand ordering and digest.

The plan contains independently keyed subplans/artifact identities. A presentation-, map-encoding-, provenance-, or serialization-only change may create a new terminal subplan without invalidating an unchanged semantic projection or code-generation subplan. `CompilePlanToken` binds the complete request for anti-replay; it does not force every subartifact to use that whole token as its cache key.

`CompileFactsBatch` binds:

- exact `ProjectionPlanToken` and demand digest referenced by the containing plan;
- semantic profile and kernel compatibility domain;
- input basis and observed dependency fingerprints;
- one typed result for every demand;
- exactness/completeness/degradation;
- integrity and size limits when decoded or transferred.

Emission first validates the whole `CompilePlanToken`, then validates that the facts satisfy the exact `ProjectionPlanToken` referenced by that plan. It rejects missing, extra, duplicated, reordered without canonical identity, stale, wrong-profile, wrong-projection-plan, wrong-root, wrong-domain, or insufficient facts before output construction.

# 3. Zero-demand behavior

When the plan has no semantic projection demands:

- no `CompileTypeInfo` session is constructed;
- no module resolver or TypeInfo index/graph/flow work runs;
- projection stage is represented as an empty complete batch bound to the plan;
- Svelte current runtime compilation must prove zero Vue/native projection demand unless its capability contract changes.

# 4. `NeedInputs`

`NeedInputs(LoadSet)` is a resumable orchestration outcome. It does not mutate the current plan or prepared value with unversioned ambient data. The outer orchestrator loads and publishes/rebuilds a coherent input view according to `input-loading.md`, then reruns projection with the same plan only when the plan’s source/request/profile basis remains valid. Otherwise it replans.

# 5. Direct and managed modes

- local direct mode may project from the retained root only;
- direct project mode consumes an immutable caller-provided observation view and returns `NeedInputs` to the caller;
- managed mode consumes one `EngineSnapshot`/`InputBasisId` per attempt and uses the outer commit/retry loop;
- all modes execute one semantic kernel and the same closed framework projector;
- lifecycle, retention, and orchestration differ; semantics do not.

# 6. Error taxonomy

Distinct failures include parse/preparation failure, unsupported product combination, unavailable semantic capability, `NeedInputs`, no progress, projection gap, unresolved dependency, stale plan/facts, profile/domain mismatch, size/integrity failure, cancellation, budget exhaustion, and internal failure.

# 7. Required tests

- plan once and emit multiple requested products without rediscovery;
- simultaneous product combinations equal separately requested products where contracts declare composability;
- different per-product output profiles coexist without a global-profile collision;
- presentation/serialization-only changes reuse unchanged semantic and code subplans;
- irrelevant product/profile fields are rejected or normalized away before identity construction;
- zero-demand zero-initialization;
- local and imported projection batches;
- batched shared-root deduplication;
- stale/wrong-whole-plan/wrong-projection-plan/wrong-profile/extra/missing facts rejected;
- facts from a terminal-only sibling plan are accepted only when their `ProjectionPlanToken` is exactly equal;
- `NeedInputs` waves and no-progress behavior;
- basis change forces retry/replan as appropriate;
- direct/prepared/managed output equality for equal product/profile/input contracts;
- cancelled/superseded attempts do not terminally materialize output.


---

# Consolidated source: `contracts/input-loading.md`

# Snapshot-Consistent Batched Input Loading Contract

**Status:** Normative outer-orchestration contract.

# 1. Kernel rule

The compiler, resolver, TypeInfo, flow, and reusable query kernels consume one immutable observation view per attempt and perform no hidden filesystem/network/process/package-manager I/O.

# 2. Outcome

```rust
enum AttemptOutcome<T> {
    Complete(T),
    NeedInputs(LoadSet),
    Terminal(AttemptFailure),
}
```

`LoadSet` is normalized, sorted, deduplicated, and includes the resolution basis needed to load/commit safely. The kernel discovers all independently reachable missing observations it can identify without fabricating semantic answers.

# 3. Orchestration state

```text
attempt number
current snapshot identity
accumulated requested InputKeys
accumulated stable positive/negative observations
unique input count
loaded byte count
dependency depth/frontier
basis-change/churn count
```

# 4. Algorithm

1. Run the whole operation against snapshot `S`.
2. On `Complete`, validate observed facts against `S` and return/admit according to the query contract.
3. On `Terminal`, return typed failure and never admit complete state.
4. On `NeedInputs(L)`, normalize `L` and calculate `delta = L.keys - accumulated_requested`.
5. If `delta` is empty and neither the resolution basis nor any previously observed fact changed, return `InputResolutionNoProgress` with the unresolved set.
6. Check configured limits before I/O: attempts, unique keys, bytes, dependency depth, and basis-change/churn count.
7. Load `delta` through embedding-owned same-key I/O flights. Each result is `Present`, stable `Missing`, or transient `LoadFailure`, with typed metadata and basis. Public/external loader digests are hints: the committing authority verifies key/basis consistency and computes or verifies content/configuration fingerprints from captured data before publication.
8. Conditionally publish all validated commit-eligible observations as one coherent input batch only if the project/configuration basis remains compatible.
9. If conditional commit loses a race or the basis changed, capture the new coherent snapshot, increment churn budget, and restart. Do not splice data into the old attempt.
10. If committed, capture the new snapshot and restart from step 1.

# 5. Direct project mode

A direct/project `CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns `NeedInputs`; the caller may rebuild/extend the environment and retry. The same no-progress/resource rules apply to convenience orchestration APIs.

# 6. Observation trust boundary

- public environments/loaders cannot mint authoritative `ContentId`, `InputBasisId`, semantic read facts, or completeness evidence;
- source bytes, declared language/source type, canonical source identity, package/config metadata, lengths, and supplied digests are consistency-checked at capture/commit;
- the authoritative owner hashes/normalizes once per committed revision and reuses the typed IDs thereafter;
- a sealed first-party `EngineSnapshot` may carry prevalidated IDs because the same authority minted them;
- mismatched key/bytes/profile/basis is a typed integrity failure and is never cached as stable missing or complete.

# 7. Negative facts

A stable missing module/package/file may be observed and cached only with the complete resolution basis: parent/package boundaries, conditions, path/case/symlink/workspace policy, and relevant configuration. Transient permission/network/process/provider failure is not a stable negative semantic fact.

# 8. Resource failures

Distinct failures include:

```text
InputResolutionNoProgress
InputResolutionAttemptLimit
InputResolutionUniqueKeyLimit
InputResolutionByteLimit
InputResolutionDepthLimit
InputResolutionChurnLimit
InputLoadUnavailable
InputCommitConflictExceeded
```

They carry unresolved keys and consumed budget without exposing sensitive ambient paths beyond the product's diagnostic policy.

# 9. Tests

- multiple missing siblings loaded in one batch;
- transitive dependency waves;
- stable missing negative fact;
- dependency appears between attempts;
- project/config changes during load;
- repeated same `LoadSet` no progress;
- loader partial/transient failure;
- unique key/byte/depth/retry/churn limits;
- external observation digest/key mismatch rejected before commit;
- no semantic kernel I/O instrumentation;
- final result equals an equivalent fully preloaded clean run.


---

# Consolidated source: `contracts/result-contract-and-flight.md`

# Result Contract and FlightCell Contract

**Status:** Normative reusable-computation and same-key flight contract.

# 1. Query, flight, and cached-candidate identity

```text
QueryIdentity<Q>
  = semantic arguments
  + only profile IDs observed by this typed query boundary
  + ResultContractId

SemanticFlightKey<Q>
  = QueryIdentity<Q> + exact InputBasisId
```

`QueryIdentity<Q>` is the bounded cache-candidate lookup identity. It contains only profiles and contract dimensions observable at this typed query boundary; terminal presentation/serialization is keyed separately when the typed value is unchanged. `InputBasisId` scopes in-flight semantic production but is not part of cross-snapshot candidate lookup. Each cached candidate carries the exact basis, positive/negative read facts, completeness proof, and compatibility material from its production attempt. It is used only after value-side validation against the requester’s current view. A store bounds the number and weight of candidates per query identity; insertion order, newest-basis preference, and global revision are never correctness authority.

`ResultContractId` includes every observable policy that can change what is accepted as a complete result at this typed boundary, without duplicating the separately keyed profile IDs:

- operation/product shape;
- required capability set;
- required exactness/completeness;
- unsupported/degradation policy;
- explicitly requested approximation mode;
- required mapping/diagnostic/serialization outcome where relevant.

Ordinary deadlines, cancellation tokens, trace IDs, priority, and work/time/memory budgets are not reusable result identity.

```rust
struct ExecutionPolicy {
    deadline: Option<Deadline>,
    cancellation: CancellationToken,
    priority: WorkPriority,
    work_budget: WorkBudget,
    memory_budget: MemoryBudget,
}
```

Budget exhaustion is `Partial` or typed failure. It never becomes a weaker `Complete` result.

# 2. Flight classes

## 2.1 Immutable content-artifact flight

Keyed only by exact construction identity. It may be joined across snapshots because its inputs are immutable content/options identities.

## 2.2 Semantic query flight

Keyed by exact `SemanticFlightKey<Q>`—one `QueryIdentity<Q>` and one exact `InputBasisId`. Cross-snapshot joining is disabled by default because the producer’s eventual read set is unknown at join time. Cross-snapshot **warm value reuse** remains possible through value-side validation after completion.

A query family may enable broader in-flight compatibility only through an accepted ADR, a closed proof of compatibility before join, and adversarial retry/cancellation evidence.

## 2.3 Cache-candidate lookup and replacement

- lookup is by exact `QueryIdentity<Q>`, never by request ID, global revision, or current snapshot alone;
- validation occurs before return and before the candidate is consumed as a dependency;
- invalid candidates may be dropped eagerly as hygiene, but correctness comes from value-side validation;
- a new candidate does not automatically erase an older still-valid candidate produced from a different unrelated snapshot;
- multiplicity is bounded by owner policy and retained only when measured reuse value exceeds validation and weight cost;
- candidate choice and eviction are deterministic or semantically invisible; insertion/arrival order cannot affect observable results;
- presentation/serialization candidates are separate from semantic typed candidates when only terminal representation changed.

# 3. Ownership

The `FlightCell`, not the first requester, owns production.

Conceptual states:

```rust
enum FlightState<T> {
    Vacant,
    Running(RunningFlight),
    Finalizing,
    Ready(Arc<T>),
    Failed(Arc<FlightFailure>),
    Cancelled,
}
```

Each waiter has an independent registration, cancellation, deadline, and response slot. The producer continues only while at least one valid waiter remains. If all waiters leave, it cooperatively cancels; useful background work is a separate explicitly requested maintenance operation, not an implicit flight afterlife.

Only `Running` accepts new waiters. `Finalizing`, `Ready`, `Failed`, and `Cancelled` do not. A request arriving after an irreversible completion/failure/budget transition starts or joins a successor flight after normal candidate lookup rather than attaching to the old outcome.

# 4. Join and aggregation

A waiter joins only when exact flight key and result contract match. There is no generic “stronger budget dominates weaker budget” rule.

The cell may aggregate execution policy conservatively:

- effective priority is the maximum active waiter priority, bounded by owner policy, and may lower after the relevant waiter leaves;
- effective producer work/memory budget is the bounded monotonic maximum requested by active waiters, not the sum;
- additional budget may be consumed only while the producer remains `Running` and has not discarded required state;
- ordinary budget cannot choose a semantic approximation, prune required obligations, or change canonical work ordering; explicit approximation is a different `ResultContractId`;
- deadline is not converted into a reusable result contract;
- cancellation of one waiter removes only that waiter;
- when all waiters leave, the owner cooperatively cancels; any useful background completion is a separately requested maintenance operation with its own identity, policy, and bound.

Aggregation cannot change semantics or reinterpret a partial as complete. Budget exhaustion transitions out of `Running`; later higher-budget requests use a successor flight.

# 5. Finalization and admission

Exactly one finalizer records:

- value or typed failure;
- actual exactness/completeness proof;
- positive and negative read facts;
- profile/toolchain/capability basis;
- cancellation/panic/stale/budget state;
- sealed `Publish` or `ReturnOnly` decision.

Every waiter is resolved exactly once. Panic, cancellation, stale basis, resource exhaustion, transient provider/I/O failure, shutdown, and internal failure admit nothing as complete. Followers validate completed values against their current admissible view before use.

# 6. Required state-machine tests

- many followers, one producer;
- first waiter cancels while followers continue;
- all waiters cancel;
- priority elevation and bounded lowering;
- larger budget arrives while `Running` and extends work within bounds;
- larger budget arrives after budget finalization and uses a successor flight;
- ordinary budget cannot select an approximation or change complete output;
- producer panic/failure/shutdown resolves all waiters once;
- double-finalization and self-wait/cycle are rejected;
- incompatible `ResultContractId` does not join;
- different `InputBasisId` semantic requests do not join by default;
- immutable content flight may join across snapshots;
- an unrelated snapshot change still discovers and validates a prior candidate through the same `QueryIdentity<Q>`;
- changed positive or negative facts reject the prior candidate;
- bounded multi-candidate replacement remains schedule/insertion-order independent;
- return-only partial never enters warm cache;
- completed candidate with invalid facts is rejected by a follower.


---

# Consolidated source: `contracts/flow-completeness.md`

# Flow Domain Closure, Obligation Ledger, and Completeness Contract

**Status:** Normative effective-flow solver and warm-admission contract.

# 1. Sole structural authority

`FunctionProgramIndex -> FunctionBodySkeleton -> FunctionFlowGraph` is the sole structural flow authority. Deterministic derived projections such as dominators, loop forests, SCCs, def-use indexes, capture summaries, and execution schedules may accelerate the graph but cannot introduce semantic facts.

# 2. Closed operation/domain registry

Every flow-bearing operation declares a static closed registry:

```text
operation/result contract
-> required product domains
-> required graph edge/fact families
-> expansion rules
-> fixed-point requirements
-> accepted gaps/failures
-> completeness finalizer
```

Representative domains include reaching values/types, narrowing, completion, closure capture/freshness/invalidation, effects, definite assignment, and coverage. An unknown domain/fact family is a typed unsupported obligation, not an ignored enum variant.

# 3. Demand plan

A deterministic `FlowDemandPlan` records:

- graph/body identity;
- source/input and semantic profile basis;
- result contract;
- selected subject/program point;
- required domains;
- initial and expanded obligation IDs;
- deterministic work order/tie breakers;
- convergence and resource policy.

Planning occurs once per cold logical demand by default. Replanning or lowering a second carrier requires a predeclared measured exception.

# 4. Obligation ledger

Each required obligation has a private typed state, for example:

```rust
enum ObligationState {
    Pending,
    Running,
    Discharged(DischargeEvidence),
    Gap(FlowGap),
    Failed(FlowFailure),
}
```

Expansion may add obligations only through registered deterministic rules. The ledger records origin, required domain, graph node/edge basis, dependencies, and discharge evidence. No public caller can mark an obligation discharged.

# 5. Complete-result construction

```rust
enum FlowSolveOutcome {
    Complete(CompleteFlowResult),
    Partial(PartialFlowResult),
    NoValue(FlowFailure),
}
```

`CompleteFlowResult` has a private constructor. The finalizer succeeds only when:

- graph, demand, profile, input basis, and result contract match;
- the closed domain registry is known;
- every required/expanded obligation is discharged;
- every required fixed point converged deterministically;
- every semantic suboperation is complete under the same contract;
- no gap, stale basis, cancellation, budget exhaustion, panic, or internal failure occurred.

An authored `any` is a semantic value. “Verter has no model” is a gap and cannot become `any`.

Only proof-bearing complete results may be warm-admitted. Partial values may be returned to explicitly tolerant consumers but remain return-only unless a separate operation contract proves partial admission safe.

# 6. Atomic production cutover

`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.

`D2` is one indivisible accepted cutover:

1. route every public effective-flow operation to the new solver;
2. delete the old syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and migration guards;
3. return typed gaps for mechanisms not yet implemented;
4. preserve every ratified Supported/Stable capability and failure contract;
5. prove no second selectable evaluator remains.

Later blocks expand only the sole solver.

# 7. Required tests

- compile-fail/private-constructor proof;
- mutation test dropping one obligation cannot yield `Complete`;
- unknown edge/domain produces a gap;
- plan/order randomized but observable result deterministic;
- no-flow path allocates no graph/plan;
- structural authored returns independent from endpoint completion;
- closure effects independent of expression position;
- loop/completion convergence and budget failure;
- partial replay never appears as warm complete;
- source search, dependency graph, and runtime tests prove the legacy evaluator is absent after `D2`.


---

# Consolidated source: `contracts/deterministic-ordering.md`

# Deterministic Ordering and Stable-ID Contract

**Status:** Normative observable-output contract.

# 1. General rule

No observable output, public ID, serialization, map segment order, diagnostic order, dependency fingerprint, or proof digest may depend on:

- hash-map/set iteration order;
- pointer/allocation address;
- worker/shard assignment;
- task completion/follower arrival order;
- cache insertion/eviction history;
- process randomness;
- wall clock;
- ambient directory enumeration order.

# 2. Canonical ordering sources

Use, in priority order:

1. authored source order where language/product semantics preserve it;
2. preassigned stable operation/input ordinal before parallel fork;
3. canonical typed key order;
4. explicit kind rank plus stable local identity;
5. canonical byte/string order only when no semantic/source order exists.

Parallel workers return `(stable_ordinal, result)` and the owner merges by ordinal, not completion time.

# 3. Diagnostics

Unless an operation defines a stronger semantic order, diagnostics sort by:

```text
canonical source ID
start byte
end byte
severity rank
stable diagnostic code
canonical typed-argument tie breaker
```

Rendered message text is not used as the primary ordering key.

# 4. Graph and public IDs

Graph/public snapshot IDs are assigned through deterministic canonical traversal. Internal arena/hash-cons IDs do not escape. A recommended order is `(root ordinal, authored span, node-kind rank, stable local ordinal)`, with explicit tie-breaking for synthesized nodes.

Snapshot-local IDs are not promised stable across semantically different snapshots unless a public contract explicitly says so. Stable cross-snapshot IDs require content/owner identity, not allocation order.

# 5. Maps, dependencies, and strings

- mapping segments are emitted in canonical generated-position order with deterministic tie breaks;
- dependency/read sets are canonicalized before fingerprinting/serialization;
- string tables use canonical traversal/ordering, not concurrent interner insertion order;
- path display normalization is separate from path semantic identity;
- domain-separated hashes include a versioned canonical encoding.

# 6. Serialization

Canonical serialization fixes field order where the format permits, set/map ordering, integer encoding, optional-field policy, normalization of equivalent values, and schema/domain identity. Nondeterministic protobuf/map iteration must be normalized before bytes are compared or signed.

# 7. Tests

Run equivalent operations across:

- randomized collection insertion order;
- worker counts and chunking;
- randomized legal task delays;
- cold/warm/evicted states;
- direct/prepared/managed regimes with equal product contract;
- native threaded, native single-thread, and WASM where supported.

Compare output bytes, diagnostics, maps, dependencies, exactness, public IDs, and terminal serialization.


---

# Consolidated source: `decisions/ADR-001-semantic-authority-and-derived-projections.md`

# ADR-001 — One Semantic Authority, Justified Derived Projections

**Status:** Accepted  
**Decision owner:** TypeInfo/effective-flow architecture

## Context

`FunctionFlowGraph` must be the one structural flow authority, but efficient solving may need dominators, loop forests, SCCs, def-use overlays, capture summaries, reverse adjacency, or compact execution schedules. Forbidding every graph-derived structure would either bloat the canonical graph or repeat topology work. Allowing independent query-specific CFGs recreates the current dual-authority defect.

## Decision

`FunctionFlowGraph` is the canonical structural authority. A derived structure is allowed only when it:

- is keyed by one exact graph/body identity and any interpretation-affecting semantic profile;
- is deterministic and fully reproducible from the graph and accepted kernel rules;
- cannot add facts, define transfer/join semantics, mark coverage complete, or publish a semantic result;
- is request-local by default and independently weighted/evicted if retention is proven valuable;
- can be replaced without changing observable semantics.

## Consequences

The solver may use efficient layouts without creating a second semantic or control authority. Review checks authority and construction rights, not superficial data-structure count.

## Rejected alternatives

- exactly one physical graph-like object;
- independent syntax-shaped/query-specific CFGs;
- projections that own relation, completion, capture, or coverage decisions.


---

# Consolidated source: `decisions/ADR-002-compatibility-domains.md`

# ADR-002 — Compatibility Domains Are Explicit and Monotonic

**Status:** Accepted

## Context

Internal progress counters should disappear, but Verter also has real serialized/public/persistent boundaries. Resetting an existing published epoch in place can make old and new bytes ambiguous.

## Decision

- a version-like value exists only for a real compatibility domain;
- one domain has one owner and a monotonic epoch sequence;
- zero is a valid first epoch and never an uninitialized sentinel;
- an incompatible clean replacement creates a new domain/namespace whose first epoch may be zero;
- disposable private caches may be invalidated by a new namespace/build fingerprint;
- ordinary in-memory DTOs remain versionless;
- duplicate counters that must “stay equal” are collapsed or separated into genuinely independent domains;
- package semver, source revisions, provider epochs, and external tool versions are not compatibility epochs.

## Consequences

Breaking pre-1.0 changes remain possible without rewriting chronology or preserving accidental counters.

## Rejected alternatives

- preserve every historical counter;
- reset every retained counter in the same namespace;
- add version fields to ordinary cross-module values.


---

# Consolidated source: `decisions/ADR-003-sealed-compile-semantic-facade.md`

# ADR-003 — Compile Semantics Use a Concrete Sealed Facade

**Status:** Accepted

## Context

The compiler needs optional project-aware semantic projections. An open trait would permit alternate module/type/runtime-classification semantics and create a second engine.

## Decision

The compiler accepts only a concrete Verter-owned `CompileTypeInfo<'_>` facade. Lifecycle variants are private/sealed first-party constructions. External integrations may supply captured observations through a data-only environment; they cannot implement semantic behavior.

All construction modes execute one profile-parameterized semantic kernel and one module resolver. The facade has no blanket `Send + Sync`; ownership/concurrency is a lifecycle policy.

## Consequences

Direct local, captured-project, in-memory, engine-snapshot, and validated-precomputed modes remain possible without opening semantics or coupling the compiler to `Engine`.

## Rejected alternatives

- public semantic trait object;
- compiler receives host/Engine/provider state;
- framework/compiler-local resolver fallback.


---

# Consolidated source: `decisions/ADR-004-typescript-semantic-profiles.md`

# ADR-004 — TypeScript Compatibility Uses Semantic Profiles, Not Build Versions

**Status:** Accepted  
**Decision owner:** native TypeScript-compatible semantic interpretation  
**Reopen only if:** a supported operation can be proven independent from every profile dimension currently classified as semantic.

## Context

TypeScript compatibility varies by supported TypeScript family and semantics-affecting project options. Rendering, serialization, worker count, cache policy, and internal implementation versions do not change TypeScript meaning and should not over-invalidate semantic results.

## Decision

Every TypeScript-compatible native semantic operation is evaluated under `TypeScriptSemanticProfileId`, containing only dimensions that can change observable interpretation, including:

- supported TypeScript compatibility family/version;
- semantics-affecting compiler options;
- module/module-resolution mode and conditions;
- JSX semantics;
- target/lib basis and exact custom/versioned library fingerprints where relevant;
- package exports/imports, paths, type roots, package-boundary, case, and symlink policy;
- declared supported behavior of the semantic kernel.

The following are separate:

- generated program semantics → `OutputProfileId`;
- diagnostic/type/path rendering → `PresentationProfileId`;
- wire/container layout → `SerializationProfileId`;
- execution placement/worker/cache/deadline/budget → execution policy;
- persistent interpretation safety → compatibility domain/build fingerprint.

An internal refactor does not change semantic profile identity unless observable semantics change. Unsupported profiles fail closed. Verter-specific stricter analysis is separately labeled enrichment.

## Consequences

- caches are semantically complete without progress-version over-keying;
- multiple supported TypeScript compatibility profiles can coexist in one kernel;
- presentation and serialization changes do not invalidate semantic computation unnecessarily.

## Rejected alternatives

- **Global pinned checker with no profile:** cannot represent multi-project compatibility.
- **One giant profile containing every option/version:** over-invalidates and turns implementation history into semantics.


---

# Consolidated source: `decisions/ADR-005-operation-dtos-and-optional-graph-export.md`

# ADR-005 — Operation DTOs Are Primary; Semantic Graph Export Is Optional

**Status:** Accepted

## Context

Removing general `TypeExpr` must not produce another mandatory general semantic representation at every public boundary.

## Decision

Public TypeInfo operations return operation-specific DTOs with exactness, diagnostics, dependencies, provenance, stable IDs, and rendered text only when requested. Session-local opaque handles may support continuation but are not stable IDs.

Graph export is a separate advanced operation with an explicit consumer inventory, compatibility domain, size/depth/node limits, deterministic IDs/order, and canonical serialization. Internal semantic storage need not mirror the wire graph.

## Consequences

Simple operations remain small and evolvable. Graph consumers are supported intentionally without constraining internal lifetime/storage.

## Rejected alternatives

- general recursive `TypeExpr`/`PortableTypeExpr`;
- mandatory graph payload for every query;
- wire node IDs reused as unchecked internal handles.


---

# Consolidated source: `decisions/ADR-006-demand-selected-flow-domains.md`

# ADR-006 — Flow Uses Demand-Selected Abstract Domains

**Status:** Accepted

## Context

One binding-based solver is required, but one always-maximal state would make every operation pay for narrowing, completion, definite assignment, freshness, capture/effects, and coverage.

## Decision

Use one solver framework and one authoritative transfer/join implementation per closed flow domain. `FlowDemandPlan` activates the transitive prerequisite closure required by the result contract. Fixed points run only over the selected obligation frontier.

Structural authored-return collection is independent of endpoint completion. A private obligation ledger and finalizer construct complete results; a query cannot omit required domains manually.

## Consequences

Narrow queries stay narrow without introducing a second evaluator. Domains are independently testable while completeness remains global to the requested result.

## Rejected alternatives

- unconditional whole-function maximal lattice;
- independent per-query semantic evaluators;
- caller-selected domain masks without closure validation.


---

# Consolidated source: `decisions/ADR-007-direct-core-before-managed-runtime.md`

# ADR-007 — Prove the Direct Core Before Managed Runtime Convergence

**Status:** Accepted

## Context

Generalizing query/executor/cache/session infrastructure before the direct operation and artifact boundaries are final risks preserving legacy managed assumptions under new names.

## Decision

The critical dependency direction is:

1. Gate 0 evidence, semantic safety, identity/compatibility/performance lock;
2. typed compositional requests, shared syntax frontend, direct compiler, prepared/resumable transaction, source units/mappings;
3. one semantic kernel and sealed compile projections;
4. sole effective-flow solver and public semantic cutovers;
5. coherent InputStore, QueryRuntime, flights, executor, retention, incrementality, providers, and host decomposition.

CSS and bounded framework-contract work may proceed only through explicit DAG edges.

## Consequences

Managed execution becomes reuse/orchestration around the smallest proven computation rather than defining it.


---

# Consolidated source: `decisions/ADR-008-deterministic-artifacts-and-persistence.md`

# ADR-008 — Deterministic Artifacts and Narrow Persistence Eligibility

**Status:** Accepted

## Context

Concurrency, cache warmth, native/WASM execution, and persistence can produce schedule-dependent or stale artifacts unless determinism and hermetic identity are explicit.

## Decision

Equal authoritative observations and result-affecting contracts produce equal Verter-owned observable outputs independent of legal schedule, worker assignment, cache warmth, or supported portable execution profile.

Persistent eligibility is limited to complete deterministic hermetic serializable values with complete compatibility, positive/negative fact, integrity, and size basis. OXC arenas, snapshot/session handles, transient cohorts, partial outcomes, and ambient-state-dependent values are never persisted.

An artifact publishes atomically with every mapping required to interpret it. Optional runtime/source-map products are not constructed when unrequested.

## Consequences

Persistence is optional acceleration, never correctness authority. Schedule-dependent map, diagnostic, ID, or serialization order is a defect.


---

# Consolidated source: `decisions/ADR-009-shared-frontends-and-parse-owner-domains.md`

# ADR-009 — IDE and Build Share One Error-Tolerant Frontend per Language Domain

**Status:** Accepted  
**Decision owner:** syntax frontend and parse lifecycle  
**Reopen only if:** an accepted ADR proves two genuinely incompatible syntax products are both necessary and cannot share one frontend without material product loss.

## Context

Including consumer role or vague parse flavor in identity can legitimize permanent IDE/build parser duplication. Interpreting “one parse identity” process-globally would force independent direct calls through global synchronization.

## Decision

For equal language bytes and `SyntaxProfileId`, IDE, build, TypeInfo, lint, formatter, and framework consumers use one Verter-owned error-tolerant frontend and parse product.

Identity is split:

```text
ParseKey = exact syntax construction identity
ParseOwnerDomainId = direct invocation/batch | PreparedCarrier | managed owner/shard
ParseInstanceId = (ParseOwnerDomainId, ParseKey)
```

Consumer role is not a key dimension. `ParseProductKind` is used only for a genuinely incompatible syntax product and requires a separate accepted ADR.

One live parse instance has one owner/result. Independent direct owner domains may parse independently. Retaining domains may pressure-evict and later perform one visible same-key reparse flight. Authored locators are revalidated after reparse. Graph/index retention does not implicitly pin the parse arena.

## Consequences

- no permanent build-fast versus IDE-tolerant dual parser;
- direct compilation stays free of process-global cache/synchronization;
- managed reuse remains explicit and bounded.

## Rejected alternatives

- **Role/flavor in identity:** hides duplicated parsing.
- **Process-global direct cache:** violates direct ownership and can add contention/retention.


---

# Consolidated source: `decisions/ADR-010-compositional-products-and-mapping-taxonomy.md`

# ADR-010 — Compiler Products Are Compositional and Mapping Kinds Are Distinct

**Status:** Accepted  
**Decision owner:** compiler request and generated-artifact contract  
**Reopen only if:** a product can prove mutual exclusivity or a new mapping class with distinct semantics.

## Context

Real requests can require several products and independent materializations. A single artifact-level enum encourages hidden “full analysis.” Treating all maps as one product conflicts with IDE companions that require projection mappings and runtime outputs whose source maps are optional.

## Decision

A compile request contains:

- a canonical non-empty collection of typed product requests;
- per-product output and terminal materialization requests rather than one global output/materialization bag;
- a typed Vue or Svelte payload before planning;
- one shared semantic profile only when the requested work observes TypeScript-compatible semantics.

Each product request carries only the output, presentation, mapping, provenance, and serialization profiles that can affect that product. Duplicate product kinds and irrelevant profile fields are rejected before expensive work. Equal normalized subrequests may share one private stage/subplan.

Mapping classes are separate:

1. `PlacementMap` — source/unit placement composition used internally where required;
2. `SourceProjectionMap` — required by an IDE/provider companion and published atomically with it;
3. `RuntimeSourceMapData` — optional runtime/build map segments created only when requested;
4. `EncodedSourceMap` — terminal serialization of requested map data.

An operation with no mapping requirement performs zero map construction/encoding. No universal artifact bag is required; typed product results may share one private execution plan.

## Consequences

- runtime plus declarations or IDE plus public API can be requested coherently even when their output/terminal profiles differ;
- required mappings cannot be omitted or mixed with another code generation;
- presentation/serialization changes do not invalidate unrelated semantic/code artifacts;
- map-disabled runtime work remains truly map-free.

## Rejected alternatives

- **Single mutually exclusive artifact enum:** cannot express real product composition.
- **Always build one map type:** wastes work and conflates different validity contracts.


---

# Consolidated source: `decisions/ADR-011-staged-compile-attempt-and-input-loading.md`

# ADR-011 — Project-Aware Compile Uses a Resumable I/O-Free Transaction

**Status:** Accepted  
**Decision owner:** direct compiler/project TypeInfo integration  
**Reopen only if:** a future compiler mode explicitly embeds I/O and accepts a separate architecture boundary.

## Context

A compile API that may need project inputs cannot safely hide I/O or retain OXC borrows across asynchronous loading. Projection facts must be proven to belong to the exact plan that consumes them.

## Decision

Project-aware compile follows:

```text
prepare -> plan -> project -> emit
```

- prepared syntax remains owned/borrowed by one retained owner;
- the projection batch is compact, owned, and OXC-free;
- `CompilePlanToken` binds the complete normalized request to the prepared root; the plan contains narrower deterministic projection/product/terminal subplan tokens;
- `CompileFactsBatch` binds to the exact `ProjectionPlanToken`, semantic profile, dependency/input basis, projection schema, and demand digest, so terminal-only sibling plans may reuse facts only when that projection token is exactly equal;
- `NeedInputs(LoadSet)` is a first-class resumable outcome;
- external loading occurs outside compiler/semantic compute;
- no syntax borrow or unvalidated locator crosses the loading boundary;
- each attempt reports the maximal sound missing observation set discovered;
- retries use a monotonic progress ledger, bounded waves/resources, and deterministic no-progress failure;
- stale or replayed facts are rejected before emission.

## Consequences

The same semantic kernel supports local, captured-project, in-memory, and managed-snapshot modes without hidden filesystem authority.

## Rejected alternatives

- **Synchronous hidden filesystem callbacks:** break hermeticity and coherent snapshots.
- **Reparse/replan implicitly after every missing input:** can amplify work and consume mismatched facts.


---

# Consolidated source: `decisions/ADR-012-stable-identifiers-and-canonical-ordering.md`

# ADR-012 — Stable Entity IDs, Session Handles, and Canonical Ordering Are Distinct

**Status:** Accepted  
**Decision owner:** public identity and deterministic serialization  
**Reopen only if:** a public contract explicitly changes which identifiers are stable across regimes/sessions.

## Context

Deterministic cross-regime IDs and cohort-local continuation handles serve different purposes. Treating a raw semantic node or session handle as a stable public ID creates lifetime and equality errors. Parallel insertion order can leak into output unless ordering authority is explicit.

## Decision

- `StableEntityId` is deterministic from a documented canonical/content-relative basis and may be compared across declared portable regimes;
- `SessionHandle` is opaque, owner/cohort-bound, generation-validated, and not compared across sessions;
- graph export, when requested, uses deterministic graph-local canonical IDs under its serialization profile;
- every observable collection has a total canonical order and deterministic tie-breaker;
- allocation address, concurrent interner insertion, hash iteration, worker completion, cache history, and owner-shard assignment cannot affect observable ordering;
- canonical serialization records its profile/domain and uses deterministic string/table/reference ordering.

## Consequences

- storage cohorts can reclaim without breaking public stable identity promises;
- direct/prepared/managed/native/WASM equality is well-defined;
- parallelism cannot leak nondeterminism.

## Rejected alternatives

- **Expose raw node IDs:** lifetime-bound and not stable.
- **Sort only at some adapters:** permits internal nondeterminism to affect hashes, maps, and caches.


---

# Consolidated source: `decisions/ADR-013-result-contracts-and-flight-owned-production.md`

# ADR-013 — Result Contracts Are Separate from Execution Policy; Flights Own Producers

**Status:** Accepted  
**Decision owner:** QueryRuntime and same-key computation  
**Reopen only if:** a query family proves safe cross-snapshot in-flight sharing or a different producer model with stronger evidence.

## Context

Mixing budgets/deadlines into reusable identity fragments caches, while omitting observable approximation/exactness contracts can let a weaker result satisfy a stronger request. Binding production to the first requester makes leader cancellation abandon followers. Joining semantic work across snapshots before the producer read set is known can cause wasted waits and retry storms.

## Decision

`ResultContractId` contains every observable complete-result requirement not already owned by a separately keyed profile ID: operation/product shape, exactness/completeness, capability/unsupported/degradation policy, requested approximation mode, and required mapping/diagnostic/serialization outcome. It does not duplicate semantic/output/presentation/serialization profile values.

`ExecutionPolicy` contains waiter-local deadline, cancellation, priority, and ordinary work/time/memory budget. Budget exhaustion is partial/failure, not a weaker complete mode.

`QueryIdentity<Q>` excludes snapshot basis and contains only dimensions observable at that typed query boundary. It is used to locate bounded cached candidates, each of which carries complete read facts and is value-validated. `SemanticFlightKey<Q>` adds exact `InputBasisId` for in-flight production. Terminal presentation/serialization uses separate identities when the typed semantic result is unchanged.

Two default flight classes exist:

- content artifact flight keyed by immutable construction identity and shareable across snapshots;
- semantic query flight keyed by semantic arguments, `ResultContractId`, and exact `InputBasisId`; cross-snapshot joining is disabled by default.

The `FlightCell`, not the first waiter, owns production. Waiters register independently. The producer continues while valid waiters remain, receives bounded priority/budget aggregation, finalizes exactly once, and publishes only through the owner's sealed admission path. Every follower validates before use.

## Consequences

- leader cancellation cannot strand followers;
- ordinary budgets do not fragment reusable identity;
- incompatible result contracts never share a producer/value;
- unrelated snapshot changes do not hide a still-valid candidate;
- cross-snapshot warm reuse remains possible through value-side validation while in-flight semantic joining remains exact-basis by default.

## Rejected alternatives

- **Leader-owned promise/future:** incorrect cancellation ownership.
- **All policy in one computation key:** over-keyed and semantically ambiguous.
- **Default cross-snapshot join:** waits on work whose unknown read set may be invalid.


---

# Consolidated source: `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md`

# ADR-014 — Flow Replaces the Legacy Evaluator in One Atomic Public Cutover

**Status:** Accepted  
**Decision owner:** flow migration and cache admission  
**Reopen only if:** a release obligation explicitly requires two semantics paths, which would require a new architecture decision and bounded compatibility window.

## Context

Building graph/domain features across many accepted merges while the old syntax-shaped evaluator remains selectable creates two production authorities. Deleting the old path only after full parity pressures implementors to copy the legacy model into the replacement.

## Decision

1. Build the minimum new graph/domain/obligation/coverage foundation behind a private non-production test boundary on the bounded cutover branch; it may be reviewed as a checkpoint but never merged or released independently.
2. In one public cutover, route all effective-flow operations to the new solver and delete the old evaluator and its state/caches/tasks/flags.
3. Unsupported mechanisms return typed non-admissible gaps; temporary reduction from guessed success to honest partial is allowed only for rows not ratified Supported/Stable, or through a separate reviewed breaking product decision.
4. Later semantic blocks only expand the sole solver.
5. A private obligation ledger and finalizer make complete-result construction impossible unless all required closed-domain obligations are discharged.

No runtime flag, compatibility shim, shadow evaluator, or compare-in-production path survives the accepted cutover.

## Consequences

- one production semantic authority at all times after `D2`;
- completeness proof is architectural rather than a convention;
- parity can expand without preserving guessed legacy behavior.

## Rejected alternatives

- **Long-running dual evaluators:** divergent semantics and cache risk.
- **Wait for full parity before deletion:** encourages porting the second authority intact.


---

# Consolidated source: `decisions/ADR-015-binding-dependency-direction.md`

# ADR-015 — Core Dependency Direction Is Inward and Cycle-Free

**Status:** Accepted  
**Decision owner:** crate/module ownership  
**Reopen only if:** a new stable boundary cannot be represented without a cycle and an alternative decomposition has been disproven.

## Context

Where `CompileTypeInfo`, framework projection DTOs, and request contracts live can accidentally create compiler↔semantic or compiler↔session cycles, forcing traits, erasure, and shared ownership.

## Decision

Binding direction is:

```text
identity/span/language/contracts
-> shared syntax frontends and dependency-neutral DTOs
-> semantic kernel/module resolver/relation/flow
-> compiler
-> managed engine/session
-> LSP/provider/MCP/NAPI/WASM/CLI adapters
```

Rules:

- syntax/contracts do not depend on compiler, session, provider, or LSP;
- semantic kernel does not depend on compiler, session, provider, or LSP;
- compiler may depend on syntax, sealed semantic facade, and neutral closed DTOs;
- managed engine depends on compiler/semantic, never the reverse;
- provider lifecycle never enters direct compiler or semantic kernel;
- adapters depend inward only;
- durable build tests reject crate dependency cycles and forbidden edges.

Logical owners do not automatically require crates; use modules/functions until a real dependency firewall or multi-consumer stable contract exists.

## Consequences

- direct compiler cannot become a session mode;
- semantic kernel remains reusable across lifecycles;
- fewer traits, erased bags, and `Arc` workarounds are required.

## Rejected alternatives

- **Mutual compiler/semantic callbacks:** creates cycles and alternate behavior.
- **Everything in session/host crate:** preserves catch-all ownership.


---

# Consolidated source: `decisions/ADR-016-implementation-lock-and-performance-gates.md`

# ADR-016 — Foundational Work Requires a Pre-Candidate Implementation Lock

**Status:** Accepted  
**Decision owner:** architecture program entry and performance governance  
**Reopen only if:** the project adopts an equally strong pre-candidate mechanism that prevents moving semantics and gates.

## Context

A strong methodology without concrete baseline, compatibility decisions, and numeric thresholds still lets implementors choose missing contracts or negotiate performance gates after a candidate exists.

## Decision

A0 captures an exact entry checkout. A1–A5 form one ordered Gate 0 lineage: command/harness fixes and the fail-closed safety retraction precede measurement and final inventories. A6 then produces one immutable Implementation Lock Record bound to:

- exact entry checkout, exact post-Gate-0 implementation baseline/tree, and repository state;
- Revision 11 package manifest digest;
- non-vacuous command/capability evidence;
- identity/profile/compatibility/protocol/dependency decisions;
- instrumentation/work baseline;
- concrete machine-readable `performance-gates.toml` with no placeholders;
- first unlocked foundational charters and review state.

Before A6, only adoption, command/capability proof, harness strengthening, wrong-complete safety retraction, measurement-only attribution, inventory, and gate/capability record work is allowed. Any Gate 0 source change invalidates and refreshes affected downstream evidence before A6. Non-safety foundational cutovers remain locked.

Gate recalibration is allowed only before the affected candidate is measured, through an exact new record digest and the same independent review class. Weakening a gate after seeing candidate results is prohibited.

## Consequences

- implementation does not invent public contracts or success criteria;
- performance decisions are reproducible and auditable;
- baseline changes trigger explicit reconciliation rather than silent drift.

## Rejected alternatives

- **Fill gates during each implementation PR:** enables outcome-driven thresholds.
- **Use prose-only “fast enough” goals:** not executable or reproducible.


---

# Consolidated source: `decisions/ADR-017-stack-aware-review-and-landing.md`

# ADR-017 — Stacked Review Must Preserve Block Atomicity and Exact-Candidate Proof

**Status:** Accepted  
**Decision owner:** PR transport, review convergence, and landing.  
**Reopen only if:** repository hosting or review mechanics provide a stronger equivalent that preserves exact candidate identity, independently releasable layers, and atomic cutovers.

## Context

Small dependent PRs improve reviewability, especially with agent-generated work, but lower-layer changes cascade new descendant SHAs and trees. Some architecture changes are independently landable; others are review-splittable but must reach trunk atomically. Treating all stacks alike conflicts with exact-candidate approval and one-production-path cutovers.

## Decision

Adopt `contracts/stacked-prs.md`:

- the program DAG remains semantic predecessor authority and the stack remains transport;
- one declared bounded stack window covers one short connected path or one atomic group; true DAG siblings use separate windows;
- mergeable layers map to independently releasable accepted blocks or explicitly accepted subblocks;
- private review-only layers have unique layer IDs, may repeat one atomic acceptance block or represent an explicit private-checkpoint predecessor, target a private integration branch, and never merge independently;
- lower-layer changes trigger restack, cumulative-tree CI, candidate invalidation, and impact-bounded reattestation; reviewed candidate identity and accepted landing identity remain separate; SHA and full tree may differ after a base advance, but a validated proof must establish exact canonical candidate-delta and generated-output equivalence on recorded bases;
- `LANDABLE` windows land bottom-up one layer at a time and issue successor snapshots; `ATOMIC_REVIEW` windows land only the final candidate;
- sibling DAG branches normally use separate stacks;
- no program-wide stack and no independently merged `D1` or other private atomic foundation.

## Consequences

Stacks are encouraged when they reduce review burden without weakening block acceptance. Rebase churn, generated changes, and exact candidate changes are visible and proven rather than silently inherited.

## Rejected alternatives

- one PR for the entire program;
- one continuously rebased fifty-block stack;
- approvals surviving changed SHAs or trees by convention;
- merging private atomic-cutover layers independently.


---

# Consolidated source: `decisions/ADR-018-opus-adapter-and-orchestrator-state.md`

# ADR-018 — Agent Orchestration Uses One Model-Independent Program Ledger and a Pinned Opus Adapter

**Status:** Accepted  
**Decision owner:** autonomous or assisted program execution.  
**Reopen only if:** an alternative preserves exact state, role independence, resumability, and evidence discipline with less machinery.

## Context

The governance defines roles, but a long-running agent still needs a deterministic entry sequence, actual-runtime identity, durable block state, and bounded context packets. A model-specific prompt must not become architecture authority, and two competing execution ledgers would be worse than conversational memory.

## Decision

- `contracts/agent-orchestration.md` is the model-independent execution contract.
- `program-state.toml`, validated by `tools/validate_program_state.py`, is the sole durable program ledger.
- `ORCHESTRATOR.md` is the sole normative entry point and authorizes `A0` only.
- `OPUS-START-HERE.md` and `agents/opus-bootstrap.md` are convenience adapters for fixed model ID `claude-opus-5`; they record the actual runtime/provider and any fallback.
- Every worker receives a digest-addressed bounded context packet and one writable worktree/branch.
- Optional subagents are used only for substantial independent work or required review independence and cannot self-accept.
- Push, merge, destructive-history, secret, and repository-policy permissions are not granted by the bootstrap prompt.

## Consequences

The package can be handed directly to an Opus orchestrator without asking it to invent sequencing, state, permissions, or review independence. Governance remains portable to another model or a human orchestrator because the Opus adapter is non-normative.

## Rejected alternatives

- paste only the master plan and rely on conversational memory;
- a separate JSON orchestrator ledger beside the program ledger;
- floating model identity without actual-runtime recording;
- one agent context scopes, implements, and solely approves;
- model-specific prompt text as durable product architecture.


---

# Consolidated source: `decisions/ADR-019-reproducible-authority-package.md`

# ADR-019 — Authority Publication Is Reproducible and Single-Source

**Status:** Accepted  
**Decision owner:** architecture package distribution.  
**Reopen only if:** a stronger content-addressed publication system replaces the bundled builder and validators.

## Context

Revision 10 was represented by divergent source, consolidated, and ZIP artifacts. A valid architecture cannot be safely implemented when its distributed authority is not singular.

## Decision

- one canonical unpacked source tree owns package content;
- `MANIFEST.json` and `VALIDATION.json` are generated from it;
- one bundled release builder produces the consolidated document, deterministic ZIP, validation report, and checksums;
- the builder re-extracts and revalidates the ZIP;
- generated artifacts are never edited independently;
- A0 rejects a digest or content mismatch.

## Consequences

The package digest has a precise meaning and can be bound into every baseline, charter, stack snapshot, and review record.

## Rejected alternatives

- manually zipping a working directory;
- publishing consolidated and split artifacts from separate trees;
- trusting filename/revision labels without content validation.


---

# Consolidated source: `decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md`

# ADR-020 — Constitutional Invariants Are Separated from Evidence-Selected Tactics

**Status:** Accepted  
**Decision owner:** architecture confidence and amendment.  
**Reopen only if:** an alternative classification provides stronger falsifiability without allowing local weakening of product invariants.

## Context

“Best possible” is not an evidence claim that can be established before implementation. Some decisions define the product architecture; others are repository facts or tactics that should change when measurements disprove them. Conflating them creates dogma or uncontrolled redesign.

## Decision

Adopt `contracts/architecture-falsification.md`:

- classify decisions as constitutional invariants, repository/product facts, evidence-selected tactics, or deferred research choices;
- require an A6 premise ledger with falsification triggers and affected blocks;
- allow tactical changes only within locked architecture and gates;
- require ADR/architecture amendment for constitutional changes;
- stop and refresh/rescope when source or measurement falsifies a premise.

## Consequences

Revision 11 is a falsifiable architecture authority rather than a claim of global optimality. Evidence can improve the implementation without reopening core ownership casually.

## Rejected alternatives

- freeze every implementation sketch as architecture;
- let implementors reinterpret core invariants as performance tactics;
- declare optimality without a disproof mechanism.


---

# Consolidated source: `program.md`

# Verter Architecture Convergence Program — Revision 11

**Status:** Normative dependency-ordered implementation authority.  
**Durable authority:** `architecture.md`, contracts, and accepted ADRs.  
**Proof authority:** `verification.md`.  
**Governance authority:** `governance.md`.  
**Machine-readable predecessor authority:** `program-dag.toml`.

# 1. Program law

The program is a DAG of bounded clean cutovers. A block may start when every predecessor has its required accepted state and the validated `program-state.toml` marks it `READY`/`BLOCK_READY`. A dependent upper layer may instead perform contingent `READY`/`IN_PROGRESS`/`REVIEW` work only when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the accepted base. Except for the explicit `D1` private checkpoint, accepted block state means the predecessor is integrated on the intended lineage. `D1` is reviewed on the bounded `D2` integration branch and is never merged/released independently; `D2` is the atomic landing unit. Parallel work is legal only when cutover closures, writable worktrees, and shared generated/owner surfaces are disjoint or explicitly serialized.

A pull-request stack is transport only. Bounded stack windows may linearize one short dependency path for review, but they do not create semantic predecessor authority. A program-wide mega-stack is prohibited. Atomic private layers land only through their final atomic candidate.

Before `A6`, only Gate 0 work is legal. `A3` may change behavior solely to retract a known wrong-complete result to a typed non-admissible outcome. It may not choose a disputed final owner.

# 2. Critical path

```text
A0–A6  implementation lock
   ↓
B      deterministic direct compiler and shared syntax ownership
   ├─────────────┐
   ↓             ↓
C semantic core  J CSS convergence (bounded parallel after shared identities)
   ↓
D atomic sole-flow-solver cutover and semantic completion
   ├──────────────┐
   ↓              ↓
E public/type     F committed input and load basis
cutovers           ↓
                 G QueryRuntime/flights/executor/cache convergence
                  ↓
                 H managed incrementality/providers/publication
   └──────────────┴──────────────┐
                                  ↓
K framework catalog, typed carriers, host decomposition
                                  ↓
L soak, final performance, optional tuning, final lock
```

# 3. Universal block admission

A non-local block is `BLOCK_READY` only when:

- the `A6` Implementation Lock Record is accepted;
- the program-state ledger validates and marks the block `READY`;
- every predecessor is accepted on recorded SHA/tree identities, or the block is explicitly contingent draft/review work over a lower layer in the same validated stack snapshot;
- touched current-owner rows are resolved;
- consumers/readers/writers/lifetimes are complete;
- identities, profiles, compatibility domains, and failure behavior are exact;
- tests discriminate the invariant and negative cases;
- applicable numeric performance/memory/work gates are populated;
- the one-path cutover and exact deletion set are named;
- abort/rescope conditions and independent reviews are assigned;
- one immutable context packet, writable worktree/branch, and stack-window/layer disposition are recorded.

# 4. Gate 0 — Implementation Lock (`A0`–`A6`)

## A0 — Adopt Revision 11 and freeze the exact checkout

**Predecessors:** none.  
**Class:** Foundational governance/evidence.

Deliver the completed entry-checkout lock, repository/lockfile/toolchain state, architecture-affecting open-change disposition, Revision 11 package/DAG digests, designated maintainer/orchestrator identities, available GitHub/CI/branch-protection/merge-queue/stack/signing permissions, worktree inventory, and the initial validated `program-state.toml`.

**Exit:** one exact entry SHA/tree and lineage; no floating or equivalence claim; only legal next Gate 0 work is exposed in program state. `A6`, not `A0`, accepts the implementation baseline after Gate 0 source changes.

## A1 — Prove non-vacuous commands and capability truth

**Predecessors:** `A0`.

Prove canonical Rust, TypeScript, NAPI, WASM, corpus, provider, and conformance commands execute intended targets and non-zero work. Complete affected capability-matrix rows and preserve raw evidence.

**Exit:** a green command cannot be empty, filtered to the wrong target, or supported by an undeclared experimental route.

## A2 — Strengthen the U6 public cold/warm differential harness

**Predecessors:** `A0`, `A1`.

Add exact recursive expectations, negative controls, oracle/profile stamps, crossed closure/effect/completion positions, and public replay/admission assertions.

**Exit:** known defect rows discriminate the actual semantic difference and cache behavior.

## A3 — Retract known wrong-complete results

**Predecessors:** `A2`.

Any exposed path that skips an unmodelled obligation returns typed `Partial`, `FlowGap`, or `NoValue` and is not warm-admitted. Authored `any` remains distinct. This is the only pre-`A6` semantic behavior change permitted.

**Exit:** no known wrong-and-warm result masquerades as complete.

## A4 — Install measurement-only attribution and capture the baseline

**Predecessors:** `A3`.

Install measurement-only attribution on the post-safety Gate 0 lineage, then count normalization, hashing, parses/reparses, preparation, planning, projection, resolver/relation/inference/flow, CSS work, rendering/maps/provenance/serialization/FFI, copies, allocations, arena live/capacity, tasks/flights/queues, admission/eviction, retained bytes, pins, and deterministic digests by logical identity.

Instrumentation is leaf-only, bounded, and disabled-overhead measured. It does not become semantic authority.

**Exit:** every strategic baseline operation can explain why expensive work ran and how often.

## A5 — Complete current-owner, identity, profile, compatibility, protocol, and consumer inventories

**Predecessors:** `A4`.

Resolve current authorities and every affected direct consumer against the exact post-safety, instrumented lineage; classify all versions/domains and configuration fields; enumerate TypeExpr/component-meta/graph/wire consumers; lock dependency-direction test strategy; reconcile open changes and surviving instrumentation owners; decide evidence paths, program-state custody, worktree/branch naming, CI/merge constraints, review contexts, and bounded stack-window policy.

**Exit:** later blocks cannot create a second owner or discover a hidden public/wire consumer mid-cutover by omission.

## A6 — Accept the Implementation Lock Record

**Predecessors:** `A5`.

Freeze:

- exact entry checkout, exact post-Gate-0 implementation baseline, Revision 11 manifest/DAG/program-state digests, and designated maintainer/orchestrator;
- command/capability and GitHub/CI/permission evidence;
- owner/consumer dispositions;
- identity/profile/compatibility/protocol decisions;
- baseline work/performance/memory data;
- machine-readable `performance-gates.toml` with no placeholders;
- accepted program-state, context-packet, evidence-custody, worktree, branch, CI, merge, and bounded stack-window policy;
- first unlocked block charters, stack placement, writable owners, and review assignments.

Gate thresholds cannot be relaxed after candidate direction is observed. A benchmark defect requires baseline and candidate reruns.

**Exit:** program state becomes `PROGRAM_LOCKED`; foundational blocks may become `BLOCK_READY`.

# 5. Track B — Deterministic direct compiler and shared syntax ownership

## B1 — Neutral contracts, typed identities, profile schemas, and dependency firewall

**Predecessors:** `A6`.

Land the distinct identity/profile/mapping/result-contract types and forbidden dependency-edge build tests. Do not add service owners merely to hold types.

**Exit:** every artifact/query can state construction and compatibility identity without global revision, request identity, or ad hoc strings.

## B2 — Shared syntax frontends and parse owner domains

**Predecessors:** `B1`.

Implement `ParseKey`, `ParseOwnerDomainId`, one live pair, owner-affine access, exact locators, pressure reparse, and direct/prepared/managed lifetime rules. Delete consumer-role parser duplication within each completed cutover.

**Exit:** same syntax construction in one owner domain parses once; direct calls remain free of hidden managed/global cache state.

## B3 — Compositional product planner

**Predecessors:** `B1`.

Implement canonical typed per-product requests, product-local output/terminal profiles, framework requests, minimal prerequisite closure, independently keyed reusable subplans, one-plan-per-request default, unsupported/duplicate-combination rejection, and zero-work counters.

**Exit:** requested and forbidden incidental work is mechanically observable.

## B4 — Compact source units, mapping taxonomy, and blanking deletion

**Predecessors:** `B2`, `B3`.
**Atomicity:** this block does not land while any production compiler route still requires full-carrier whitespace blanking for the migrated source-unit family.

Introduce logical units, `PlacementMap`, required `SourceProjectionMap`, optional `RuntimeSourceMapData`, terminal `EncodedSourceMap`, and immutable generated-code-plus-required-map artifacts. Migrate every current compiler consumer in the bounded family and delete source-sized `eval_source`/blank-buffer ownership in the same accepted cutover.

**Exit:** offset preservation uses exact units/maps rather than a source-sized duplicate, and required IDE maps cannot be skipped by a runtime-map flag.

## B5 — Borrowed one-shot compiler atomic cutover

**Predecessors:** `B4`.

Introduce the borrowed direct boundary over the surviving shared frontend, planner, units, mappings, and framework algorithms. Keep arenas/local state owner-affine, construct no managed/session/provider/audit infrastructure, materialize only requested terminal artifacts, separate pure Rust and FFI paths, migrate every current direct/standalone caller, and delete the displaced public/direct route in the same accepted cutover.

**Exit:** source-only one-shot compile is one genuine low-overhead library product with no shadow direct implementation.

## B6 — `PreparedCarrier`, direct batch, and direct-core closure

**Predecessors:** `B5`.

Add explicit borrowed/owned preparation, prepare-once/emit-many, inspectable retained weight, safe drop, direct batch ownership, deterministic aggregation, and no implicit project/provider/global state. Prove all three direct regimes use the same accepted parser/planner/unit/mapping/codegen core and delete any transition-only adapters.

**Exit:** reuse is explicit and lock-free/single-owner by default; the direct core is final enough for semantic projection and managed orchestration to depend on it.

# 6. Track C — One non-flow semantic core and closed compile projections

## C1 — Converge `ModuleResolverCore` and non-flow `TypeInfoCore`

**Predecessors:** `A6`, `B1`, `B2`.

Preserve correct current resolution/index/fact foundations, remove alternate resolver semantics, use immutable observation views, and return batched `NeedInputs`. Flow semantics are excluded.

**Exit:** lifecycle differs; module/name/type/relation meaning does not.

## C2 — Staged compile transaction and concrete sealed facade

**Predecessors:** `B3`, `B5`, `C1`.

Implement prepare/plan/project/emit, anti-replay plan/fact tokens, concrete `CompileTypeInfo`, first-party lifecycle modes, no public semantic trait object, no blanket `Send + Sync`, and bounded load/retry behavior.

**Exit:** project-aware compilation cannot accept another semantic engine or stale/mismatched facts.

## C3 — Closed Vue runtime macro projection

**Predecessors:** `C2`.

Batch exact demands, deduplicate shared roots, stop at broad constructor terminals, follow imports/generics/unions/mapped/indexed forms only until the closed answer, and return typed degradation/dependencies/profile.

**Exit:** codegen receives only facts it consumes.

## C4 — Zero-work, batch-shape, and regime-equivalence proof

**Predecessors:** `B5`, `C3`.

Prove no-demand zero initialization, Svelte zero Vue/native projection, aggregate-project-once batch shape, and equal direct/prepared/managed semantics.

**Exit:** optional semantics are physically absent when not requested.

# 7. Track D — Atomic sole effective-flow solver

## D1 — Private sole-solver foundation

**Predecessors:** `A3`, `B1`, `B2`, `C1`.

On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.

No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.

**Exit:** the minimum replacement preserves every ratified supported capability, safely answers its covered cases, and fails unsupported cases without a second production authority.

## D2 — Atomic public flow cutover and legacy deletion

**Predecessors:** `D1`.

In one accepted candidate:

- route every effective-flow product operation to the new solver;
- delete the syntax-shaped evaluator, name-keyed state, duplicate control/completion/relation/effect paths, caches, tasks, metrics, flags, guards, and migration comments;
- keep unsupported mechanisms as typed non-admissible gaps;
- preserve every `A6`-ratified Supported/Stable effective-flow capability and its public failure contract;
- prove one production solver by source, dependency, runtime, and cache-admission tests.

**Exit:** one graph authority and one production solver. This block is indivisible.

## D3 — Shared relation authority and binding/product domains

**Predecessors:** `D2`, `C1`.

Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.

## D4 — Narrowing and structural returns

**Predecessors:** `D3`.

Implement supported narrowing/invalidation/predicate/substitution mechanisms. Every authored return contributes structurally; endpoint completion alone controls fallthrough `undefined`.

## D5 — Closure, capture, freshness, and position-independent effects

**Predecessors:** `D3`.

Retain direct/transitive reads and writes, escape summaries, creation-time frontier/freshness, profile-compatible invalidation, and one effect mechanism across expression positions.

## D6 — Loop fixed points and completion algebra

**Predecessors:** `D3`.

Implement deterministic selected-frontier loop convergence and one completion algebra for labels, switch, loops, try/catch/finally, return, throw, break, and continue.

## D7 — `this`, sequence, call/context, and value surfaces

**Predecessors:** `D4`, `D5`, `D6`, `C1`.

Route exposed carriers, sequence pass-through, call/contextual callback, and value-inference interactions through the one kernel. Unsupported async/generator/cross-file/opaque-call areas remain typed gaps.

## D8 — U6 convergence and complete-result/admission proof

**Predecessors:** `D4`, `D5`, `D6`, `D7`.

Make all claimed rows match the pinned semantic profile or return the declared typed gap; prove every ledger obligation/fixed point and warm-admission negative case.

**Exit:** wrong-and-warm is structurally unrepresentable for the claimed operations.

# 8. Track E — Public TypeInfo, TypeExpr, and semantic storage cutovers

## E1 — Complete TypeExpr/component-meta/graph/protocol consumer closure

**Predecessors:** `A5`, `C1`, `D2`.

Turn the seed inventory into an exact producer/consumer/protocol/lifetime map. Name every NAPI/WASM/wire/cache/test route and compatibility obligation.

## E2 — Eliminate internal general `TypeExpr` transit

**Predecessors:** `C4`, `D8`, `E1`.

Migrate consumers to borrowed authored nodes, compact exact chunks, semantic values, or operation DTOs. Delete each displaced producer/conversion/cache in the same bounded cutover; do not replace it with a mandatory general graph.

## E3 — Public operation DTOs and optional bounded graph export

**Predecessors:** `E2`.

Make query-specific DTOs primary; separate `StableEntityId` and `SessionHandle`; retain graph export only for real consumers under a named compatibility/size/canonical-ordering contract.

## E4 — Reclaimable semantic storage and scoped interning

**Predecessors:** `E2`, `E3`.

Implement generation/cohort-safe handles, explicit pins, selective promotion, scoped/compactable interners, and owner-local byte bounds. No public output pins internal arenas.

# 9. Track F — Committed inputs and resumable loading

## F1 — One committed input and coherent snapshot authority

**Predecessors:** `A6`, `B6`, `C4`, `D2`.

Reconcile current VFS/registered-source/workspace publication into one authority; use short immutable-root commits; keep parse/compile/provider work outside the write critical section; preserve ordered document mutation.

## F2 — InputBasis, load waves, negative facts, and retry

**Predecessors:** `F1`, `C2`.

Implement exact `InputBasisId`, normalized batched `LoadSet`, stable positive/negative observations, conditional coherent commits, no-progress and resource bounds, and clean preloaded equivalence.

# 10. Track G — QueryRuntime, FlightCell, executor, and cache convergence

## G1 — Hermetic query facts and result contracts

**Predecessors:** `C4`, `D8`, `F2`.

Implement snapshot-independent `QueryIdentity`, exact-basis `SemanticFlightKey`, bounded multi-candidate lookup, positive/negative read facts, exact result contract, owner-controlled compute scope, sealed publish/return-only decision, value-side validation, and operation-specific cycle/failure policy.

## G2 — FlightCell-owned same-key production

**Predecessors:** `G1`.

Implement content and semantic flight classes, exact input-basis scoping, independent waiters, policy aggregation, cancellation, panic/shutdown/finalization, follower validation, and no leader-owned lifetime.

## G3 — Bounded CPU execution and owner-affine commands

**Predecessors:** `B2`, `G2`.

Keep hits/tiny dependent work inline; fork/chunk only coarse owned work; schedule compact commands to owner-affine parse/semantic state; bound queues/fan-out/stacks; reserve interactive capacity; support local/WASM execution without semantic divergence.

## G4 — Cache/store convergence and bounded retention

**Predecessors:** `E4`, `G1`, `G2`, `G3`.

Classify each current store, preserve correct value-side validation, remove duplicate correctness invalidation/dedupe, index bounded semantic candidates by snapshot-independent `QueryIdentity`, make return-only default, and enforce weight, pressure, pin, and shutdown contracts.

## G5 — Scheduler/pool/host runtime convergence

**Predecessors:** `G3`, `G4`.

Delete semantic DAG duplication, duplicate pools/dedupe/task taxonomies, and transitional generation machinery only after complete replacement. Preserve ordered mutation and external protocol actors in their real owners.

# 11. Track H — Managed incrementality, providers, and publication

## H1 — Immutable stable-unit incremental reuse

**Predecessors:** `B6`, `F1`, `G4`.

Apply ordered edits, reconcile logical units, reuse unchanged stage artifacts, recompute on value-side validation failure, recompose mappings, keep fallback visible, and prove clean final equivalence.

## H2 — Project-scoped ProviderHub bindings

**Predecessors:** `G5`, `H1`.

Implement explicit capability-declared provider routes/epochs, no racing or silent semantic fallback, demand-scoped companions, controlled transitions, and orthogonal native enrichment. Keep `verter_tsc` a narrow batch-checker boundary.

## H3 — Atomic readiness and stale-safe publication

**Predecessors:** `F1`, `H1`, `H2`.

Publish generated companion and required `SourceProjectionMap` atomically; wait only for requested facts; validate observed document/project/config/provider/mapping/dependency stamps; bound channels and protect interactive capacity.

# 12. Track J — CSS least-work convergence

## J1 — Reconcile CSS syntax, formatter, scanner, and transform owners

**Predecessors:** `A4`, `A6`.

Preserve `StyleSyntaxIr` where it owns syntax; inventory specialized fast paths, preprocessors, CodeTransform mappings, and all consumers. A specialized path survives only when it shares authority and wins equivalent-work proof.

## J2 — Exact style identity and owner-domain reuse

**Predecessors:** `B1`, `B2`, `J1`.

One live parse per exact bytes/dialect/options/domain within its owner; changed bytes create a new identity; pressure reparse is explicit.

## J3 — Shared plans and terminal materialization

**Predecessors:** `B6`, `J2`.

Fuse walks/edit plans where semantics permit; avoid copies for unchanged output; build runtime maps/provenance/descriptors/serialization only when requested.

## J4 — Dialect, preprocessor, formatter, and recovery contract

**Predecessors:** `J3`.

Declare Native/External/Unsupported per operation; make processed bytes/maps/dependencies/fingerprints explicit; prove deterministic idempotent recovery-aware formatting and no private duplicate grammar.

# 13. Track K — Framework catalog, typed carriers, and host decomposition

## K1 — Capability-composed framework catalog

**Predecessors:** `C4`, `E3`, `H3`, `J4`.

Provide immutable coarse dispatch, typed Vue/Svelte capabilities, a synthetic alternate-shape fixture, and monomorphic inner loops. No universal framework IR/fact/cache/options owner.

## K2 — Typed framework options and carriers

**Predecessors:** `B6`, `K1`.

Keep shared axes truly shared; make framework-private requests typed; remove final `Any + Send + Sync` downcasts; keep direct carriers free of managed erasure/thread-safety costs.

## K3 — Reduce/retire `VerterHost` and catch-all session ownership

**Predecessors:** `E4`, `G5`, `H3`, `K2`.

Extract each invariant only after its final owner exists, migrate all consumers, delete sideways mutable access and dependency cycles, and retain only a small stable facade where product compatibility requires it.

# 14. Track L — Soak, performance, tuning, and final lock

## L1 — Long-churn bounded-memory soak

**Predecessors:** `E4`, `G4`, `H3`, `J4`, `K3`.

Run the `A6`-locked edit/create/delete/rename/move/open/close, project/config/provider restart, query-storm, pressure, cancellation, idle, and quiescence campaigns. Prove clean equivalence, attributable pins, owner plateaus, no monotonic count growth, and no restart cleanup.

## L2 — Final equivalent-work performance decision

**Predecessors:** `B6`, `C4`, `D8`, `E4`, `H3`, `J4`, `K3`, `L1`.

Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.

## L3 — Optional post-architecture micro-optimization

**Predecessors:** `L2`.  
**Optional:** open only when profiles show a remaining strategic bottleneck.

Allocator, arena pooling, LTO/PGO, SIMD, hashing/container, or executor specialization may proceed only without reopening authority/lifetime boundaries. If opened, it must be accepted before `L4`.

## L4 — Final architecture lock

**Predecessors:** `L2`; and `L3` if opened.

Make code, architecture, ADRs, capability truth, protocols, and tests agree; remove obsolete plans/charters/shims/guards/campaign comments; pass final exact-SHA conformance, architecture, adversarial performance/memory, and maintainer acceptance.

**Exit:** one simpler, deterministic, bounded, measurably leading or non-dominated production architecture.

# 15. Abort and rescope conditions

Stop and return to scope when:

- a hidden owner/consumer/protocol lies in the real cutover closure;
- a required predecessor was neither accepted nor a valid lower layer in the same immutable stack snapshot, or acceptance was attempted before the predecessor landed;
- a profile field or identity dimension is ambiguous;
- a compile plan/facts/load attempt cannot be replay-safe;
- an unknown flow obligation could be ignored as complete;
- a second selectable semantic/parser/resolver/cache-correctness path would survive;
- public/wire compatibility prevents the promised deletion;
- a performance gate would need to be weakened after candidate direction is known;
- correctness requires stale/partial output or unbounded retention;
- native and WASM/local semantics would diverge;
- tests do not discriminate the claimed invariant.

The remedy is an amended contract/ADR/charter and renewed review—not a hidden flag, shim, broad trait, duplicate cache, parallel implementation, or outcome-driven threshold.


---

# Consolidated source: `program-dag.toml`

```toml
schema = 1
revision = 11
entry_gate = "A6"
final_gate = "L4"

# `predecessors` are acceptance dependencies, not merely recommended order.
# L3 is optional. If opened, it becomes an additional predecessor of L4.

[[block]]
id = "A0"
name = "Adopt Revision 11 and freeze exact checkout"
class = "foundational"
predecessors = []

[[block]]
id = "A1"
name = "Non-vacuous commands and capability truth"
class = "foundational"
predecessors = ["A0"]

[[block]]
id = "A2"
name = "U6 public cold/warm differential harness"
class = "foundational"
predecessors = ["A0", "A1"]

[[block]]
id = "A3"
name = "Immediate wrong-complete safety retraction"
class = "foundational-safety"
predecessors = ["A2"]

[[block]]
id = "A4"
name = "Measurement attribution baseline"
class = "foundational"
predecessors = ["A3"]

[[block]]
id = "A5"
name = "Owner identity profile compatibility protocol consumer inventories"
class = "foundational"
predecessors = ["A4"]

[[block]]
id = "A6"
name = "Implementation Lock Record"
class = "foundational-gate"
predecessors = ["A5"]

[[block]]
id = "B1"
name = "Neutral contracts identities profiles dependency firewall"
class = "foundational"
predecessors = ["A6"]

[[block]]
id = "B2"
name = "Shared syntax frontends and parse owner domains"
class = "foundational"
predecessors = ["B1"]

[[block]]
id = "B3"
name = "Compositional product planner"
class = "foundational"
predecessors = ["B1"]

[[block]]
id = "B4"
name = "Compact source units mappings and blanking deletion"
class = "foundational-atomic"
predecessors = ["B2", "B3"]

[[block]]
id = "B5"
name = "Borrowed one-shot compiler atomic cutover"
class = "foundational-atomic"
predecessors = ["B4"]

[[block]]
id = "B6"
name = "PreparedCarrier direct batch and direct-core closure"
class = "foundational"
predecessors = ["B5"]

[[block]]
id = "C1"
name = "ModuleResolverCore and non-flow TypeInfoCore"
class = "foundational"
predecessors = ["A6", "B1", "B2"]

[[block]]
id = "C2"
name = "Staged compile transaction and sealed facade"
class = "foundational"
predecessors = ["B3", "B5", "C1"]

[[block]]
id = "C3"
name = "Closed Vue runtime macro projection"
class = "foundational"
predecessors = ["C2"]

[[block]]
id = "C4"
name = "Zero-work batch-shape regime-equivalence proof"
class = "foundational"
predecessors = ["B5", "C3"]

[[block]]
id = "D1"
name = "Private sole-solver foundation checkpoint"
class = "foundational-private-checkpoint"
predecessors = ["A3", "B1", "B2", "C1"]

[[block]]
id = "D2"
name = "Atomic public flow cutover and legacy deletion"
class = "foundational-atomic"
predecessors = ["D1"]

[[block]]
id = "D3"
name = "Extend shared relation authority and binding product domains"
class = "foundational"
predecessors = ["D2", "C1"]

[[block]]
id = "D4"
name = "Narrowing and structural returns"
class = "foundational"
predecessors = ["D3"]

[[block]]
id = "D5"
name = "Closure capture freshness effects"
class = "foundational"
predecessors = ["D3"]

[[block]]
id = "D6"
name = "Loop fixed points and completion algebra"
class = "foundational"
predecessors = ["D3"]

[[block]]
id = "D7"
name = "This sequence call context value surfaces"
class = "foundational"
predecessors = ["D4", "D5", "D6", "C1"]

[[block]]
id = "D8"
name = "U6 convergence and complete-result admission proof"
class = "foundational"
predecessors = ["D4", "D5", "D6", "D7"]

[[block]]
id = "E1"
name = "TypeExpr component-meta graph protocol consumer closure"
class = "foundational"
predecessors = ["A5", "C1", "D2"]

[[block]]
id = "E2"
name = "Eliminate internal general TypeExpr transit"
class = "foundational"
predecessors = ["C4", "D8", "E1"]

[[block]]
id = "E3"
name = "Public operation DTOs and optional graph export"
class = "foundational"
predecessors = ["E2"]

[[block]]
id = "E4"
name = "Reclaimable semantic storage and scoped interning"
class = "foundational"
predecessors = ["E2", "E3"]

[[block]]
id = "F1"
name = "One committed input and coherent snapshot authority"
class = "foundational"
predecessors = ["A6", "B6", "C4", "D2"]

[[block]]
id = "F2"
name = "InputBasis load waves negative facts retry"
class = "foundational"
predecessors = ["F1", "C2"]

[[block]]
id = "G1"
name = "Hermetic query facts and result contracts"
class = "foundational"
predecessors = ["C4", "D8", "F2"]

[[block]]
id = "G2"
name = "FlightCell-owned same-key production"
class = "foundational"
predecessors = ["G1"]

[[block]]
id = "G3"
name = "Bounded CPU execution and owner-affine commands"
class = "foundational"
predecessors = ["B2", "G2"]

[[block]]
id = "G4"
name = "Cache store convergence and bounded retention"
class = "foundational"
predecessors = ["E4", "G1", "G2", "G3"]

[[block]]
id = "G5"
name = "Scheduler pool host runtime convergence"
class = "foundational"
predecessors = ["G3", "G4"]

[[block]]
id = "H1"
name = "Immutable stable-unit incremental reuse"
class = "foundational"
predecessors = ["B6", "F1", "G4"]

[[block]]
id = "H2"
name = "Project-scoped ProviderHub bindings"
class = "foundational"
predecessors = ["G5", "H1"]

[[block]]
id = "H3"
name = "Atomic readiness and stale-safe publication"
class = "foundational"
predecessors = ["F1", "H1", "H2"]

[[block]]
id = "J1"
name = "CSS owner reconciliation"
class = "subsystem"
predecessors = ["A4", "A6"]

[[block]]
id = "J2"
name = "Exact style identity and owner-domain reuse"
class = "subsystem"
predecessors = ["B1", "B2", "J1"]

[[block]]
id = "J3"
name = "Shared CSS plans and terminal materialization"
class = "subsystem"
predecessors = ["B6", "J2"]

[[block]]
id = "J4"
name = "Dialect preprocessor formatter recovery contract"
class = "subsystem"
predecessors = ["J3"]

[[block]]
id = "K1"
name = "Capability-composed framework catalog"
class = "foundational"
predecessors = ["C4", "E3", "H3", "J4"]

[[block]]
id = "K2"
name = "Typed framework options and carriers"
class = "foundational"
predecessors = ["B6", "K1"]

[[block]]
id = "K3"
name = "VerterHost and catch-all session decomposition"
class = "foundational"
predecessors = ["E4", "G5", "H3", "K2"]

[[block]]
id = "L1"
name = "Long-churn bounded-memory soak"
class = "foundational"
predecessors = ["E4", "G4", "H3", "J4", "K3"]

[[block]]
id = "L2"
name = "Final equivalent-work performance decision"
class = "foundational"
predecessors = ["B6", "C4", "D8", "E4", "H3", "J4", "K3", "L1"]

[[block]]
id = "L3"
name = "Optional post-architecture micro-optimization"
class = "foundational-optional"
predecessors = ["L2"]
optional = true

[[block]]
id = "L4"
name = "Final architecture lock"
class = "foundational-final"
predecessors = ["L2"]
conditional_predecessor_if_opened = ["L3"]
```


---

# Consolidated source: `governance.md`

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


---

# Consolidated source: `verification.md`

# Verter Revision 11 Verification Contract

**Status:** Normative proof authority.  
**Rule:** a claim is accepted only when the exact candidate, exact result contract, and exact input/profile basis produced reproducible evidence.  
**No substitution:** one green fixture, one wall-time number, one warm cache hit, or one reviewer opinion cannot substitute for the required proof family.

# 1. Proof hierarchy

Evidence is ranked in this order:

1. externally observable behavior and official/differential conformance;
2. deterministic direct-versus-prepared-versus-managed equivalence;
3. negative proof that unsupported/partial/stale/cancelled/budgeted work cannot publish as complete;
4. exact work, copy, allocation, queue, and lifetime attribution;
5. absolute SLO and Verter no-regression decision;
6. equivalent-work competitor/Pareto decision;
7. bounded-memory and churn plateau;
8. architecture/dependency/deletion/complexity proof;
9. reviewer convergence on the unchanged candidate SHA.

A faster result does not pass if a higher-ranked proof fails.

# 2. Candidate provenance and non-vacuous execution

Every evidence bundle records:

- entry checkout (for Gate 0 provenance), implementation baseline, candidate SHA, and clean/dirty state;
- Revision 11 authority-package digest and Implementation Lock Record ID;
- Rust toolchain, target, features, linker, codegen/LTO/PGO, allocator, panic mode, and environment;
- Node/runtime/package manager/lockfile, TypeScript/native-TypeScript/provider versions, Vue/Svelte versions, NAPI/WASM runtime, and platform;
- benchmark machine/runner class, CPU topology/governor, memory, OS/kernel, background-load policy, and thermal policy;
- corpus and generated-input fingerprints;
- semantic/output/presentation/serialization/execution/result contract identities;
- cache/prepared state and thread/worker configuration;
- exact commands and raw logs/results.

Each test command must emit:

- discovered case/test count;
- executed count greater than zero;
- pass/fail/skip/ignored count;
- shard/feature/target selection;
- oracle/provider provenance where relevant.

A selector that runs zero cases is a failure. A skipped critical suite is a failure unless the block charter explicitly excludes it and no acceptance claim relies on it.

Sentinel verification is required for critical harnesses: in an isolated non-candidate run, deliberately break one known assertion or test fixture and prove the canonical selector fails.

# 3. Contract and architecture conformance

## 3.1 Dependency direction

Required compile-time or repository-graph checks prove:

- contract/identity/profile crates/modules depend on no compiler, semantic, session, provider, or framework implementation;
- framework compilers may depend on contract/syntax/framework-local code and the sealed semantic projection facade, not QueryRuntime/InputStore/ProviderHub;
- semantic kernel may depend on syntax/index/contracts and dependency-neutral observations, not framework codegen or managed engine;
- direct compiler has no dependency on `Engine`, InputStore, QueryRuntime, ProviderHub, durable stores, or LSP;
- managed services depend inward on direct algorithms/contracts, never the reverse;
- provider/LSP adapters do not become semantic authorities;
- no dependency cycle is hidden through feature flags, build scripts, generated code, or protocol conversion crates.

A machine-readable dependency snapshot is compared before/after every foundational block.

## 3.2 Public and auto-trait contract

Compile tests prove:

- `PreparedCarrier` and local `CompileTypeInfo` are not required to be `Sync`;
- `Send` exists only where safe whole-owner transfer is intentionally supported;
- no `unsafe impl Send/Sync` patches an ownership mismatch;
- OXC/arena types do not escape public direct results or compact executor jobs;
- stable public entity IDs and ephemeral cohort/session handles use distinct types;
- direct APIs expose no workspace/query/provider/cache implementation type;
- external integrations cannot implement alternate semantic resolution/classification behavior.

## 3.3 Clean-cutover proof

For each cutover, evidence includes:

- old declarations and implementation deleted;
- every compiler-reported caller migrated;
- old cache keys/stores, tasks, metrics, flags, re-exports, schema fields, fixtures, and docs deleted or explicitly preserved by a real compatibility contract;
- no runtime switch or fallback selects the old path;
- no source-name scanner exists merely to prevent resurrection;
- dependency graph and reachable production path count;
- source comments in the changed closure rewritten as present-tense invariants.

Text search is supporting evidence only; type/dependency/behavior proof is authoritative.

# 4. Compiler correctness and product contracts

## 4.1 Product matrix

Every supported product contract is tested independently and in valid combinations:

- Vue runtime subtargets currently claimed;
- Vue IDE companion;
- Vue public API/declarations where claimed;
- Svelte runtime/IDE/public products currently claimed;
- required diagnostics policy;
- required IDE projection map;
- optional runtime source-map data and encoded external map;
- provenance and serialization on/off;
- local and imported compile projections;
- zero-projection paths;
- direct one-shot, prepared first/repeat, batch, and managed paths;
- native, NAPI, and WASM where the operation is declared portable.

For every cell, assert:

- code bytes or normalized semantic output;
- required map coverage and source/generated round trips;
- diagnostics code/span/arguments/severity/order;
- exactness/completeness/unsupported/failure outcome;
- dependency and profile basis;
- deterministic stable IDs where exposed;
- absence of products not requested.

## 4.2 Combination law

For products declared independent, requesting `{A,B}` must equal requesting `A` and `B` separately after canonical product ordering, except for intentional shared materialization that changes no result.

For products declared coupled, the contract must name the coupling—for example, an IDE companion and the projection map required to interpret it publish atomically.

A combination must not:

- widen semantics or diagnostics beyond either product contract;
- collapse distinct per-product output/presentation/serialization profiles into one global profile;
- force Vue projection on Svelte;
- generate runtime maps because an IDE projection map is required;
- initialize TypeInfo when the combined plan has zero projection demands;
- create duplicate parses or duplicate subplans for an identical live prerequisite;
- invalidate an unchanged semantic/code subplan because only one product's terminal presentation or serialization changed.

Duplicate product kinds, irrelevant profile fields, and unsupported combinations are rejected before parsing/projecting beyond work already required to classify the request. Canonically equivalent subrequests produce equal subplan identities regardless of caller insertion order.

## 4.3 Direct/managed semantic identity

For the same source/project observations, profiles, and product contracts:

```text
direct complete result
== prepared complete result
== managed cold complete result
== managed warm complete result
```

Equality covers all Verter-owned observable bytes/facts except explicitly ephemeral request/session handles and timing/audit metadata. Stable IDs must still be equal.

## 4.4 Project-aware retry

Test at minimum:

- local root with no missing input;
- one import, multiple sibling imports, re-export chain, package exports/conditions, path mapping, declaration/library file, config inheritance, realpath/case policy;
- multiple independent misses returned in one currently knowable wave;
- transitive misses over multiple waves;
- unavailable input;
- no-progress repeated key;
- wave/key/byte/retry budget exhaustion;
- project/profile basis change between waves;
- cancellation between waves;
- prepared plan reuse without stale AST borrow;
- one-shot convenience documented reparse behavior;
- facts replay from another root/profile/demand rejected before emission.

The final complete result after caller-owned loading must equal a clean compile against the complete captured environment.

# 5. Native TypeInfo and flow semantic conformance

## 5.1 Profile provenance

Every differential row records:

- exact TypeScript compatibility family/version;
- normalized parse/checker/resolver options and profile ID;
- library/environment fingerprints;
- Verter semantic-kernel domain/epoch;
- source fixture and expected exactness;
- whether the outcome is Complete, Partial, Unsupported, NoValue, or Failed.

Unknown/unclassified semantic-affecting options must fail profile construction rather than silently reuse a result.

## 5.2 Effective type operation

`TypeAtPosition` is tested for:

- declaration/contextual type without flow;
- assignments and use-site narrowing;
- `typeof`, equality, truthiness/optional chain, `in`, `instanceof`, discriminants/switch/destructure;
- substitutions/generics and invalidation;
- predicate/assertion effects;
- nominal `unique symbol` identity and overlap;
- `this`, sequence, callback/contextual/call/value interactions in the claimed capability;
- deep/recursive/budgeted/cancelled cases;
- unsupported mechanism as typed non-complete outcome.

No-flow assertion:

```text
TypeAtPosition(no-flow)
=> correct base/contextual result
=> zero FunctionFlowGraph construction
=> zero FlowDemandPlan/domain solver allocation
=> zero duplicate source traversal used only to ask whether flow is needed
```

## 5.3 Structural return law

For every function fixture:

```text
return_result = union(
    effective_type(return_expression_i, program_point_i)
    for every authored return statement i
) + (endpoint_reachable ? undefined : nothing)
```

Test unreachable authored returns, `never` expressions, nested branches, loops, labels, switch, try/catch/finally, throws, and endpoint fallthrough.

Reachability may not remove an authored return contributor. Subject emptiness may not remove an unrelated branch.

## 5.4 Flow mechanism matrices

### Narrowing matrix

Cross:

- construct family;
- positive/negative branch;
- assignment before/inside/after;
- alias and substitution;
- nested logical/conditional/sequence position;
- loop and abrupt completion;
- exact/profile variation;
- cold/warm/recomputed path.

### Closure matrix

Cross:

- direct/transitive capture depth;
- read/write/both;
- creation before/after narrowing;
- invoked/escaped/opaque call;
- sibling/deeper closure;
- declarator, argument, return, condition, sequence, object/array, default initializer, expression statement;
- cold/warm/recomputed path.

### Completion matrix

Cross:

- block/if/loop/label/switch/try/catch/finally;
- normal/return/throw/break/continue;
- labelled/unlabelled;
- finally normal/override;
- endpoint reachable/unreachable;
- selected-domain closure and fixed-point convergence.

## 5.5 Sole-solver proof

After `D2`:

- every production effective-type/flow-return entry reaches the same solver owner;
- old syntax-shaped control types are absent from production code and dependency graph;
- no feature flag, test-only runtime selector, cache key, or fallback can choose a second evaluator;
- derived projections cannot construct a complete result or semantic transfer independently;
- unsupported rows leave explicit gaps in the sole solver;
- every `A6`-ratified Supported/Stable effective-flow capability remains conformant, or an explicit reviewed breaking capability decision is present.

## 5.6 Coverage/admission negative proof

Fault-injection tests force each obligation/domain edge to be skipped or report a gap. Assert:

- complete finalizer cannot construct `CompleteFlowResult`;
- no warm/persistent admission occurs;
- public exactness is not Complete;
- a useful partial, where allowed, records the missing domain/obligation;
- cancellation, stale input, panic, budget exhaustion, and torn basis have the same non-admission property.

# 6. Determinism and equivalence

## 6.1 Determinism domain

Given equal:

- authoritative source/project/configuration observations;
- semantic, output, presentation, serialization, compatibility, and product contracts;
- framework/toolchain identities;
- supported portable execution profile;

the following must be equal:

- generated code bytes;
- required projection map and requested runtime/source-map data;
- diagnostics code/span/typed arguments/rendered text when requested;
- public operation DTOs and graph exports;
- stable entity IDs, dependency fingerprints, exactness, provenance, and canonical serialization;
- cache/persistence payload bytes where canonical byte equality is claimed.

Excluded from semantic equality:

- wall-clock timestamps;
- trace/request IDs;
- queue timing;
- session-local opaque handles;
- non-normative performance counters;
- platform-specific error text not part of a portable contract.

## 6.2 Schedule randomization

Critical deterministic suites run under at least:

- 100 randomized scheduling seeds for pre-merge foundational candidates;
- 1,000 seeds or a coverage-equivalent systematic schedule campaign before `L4` for flight/publication/parallel aggregation code;
- worker counts `1`, `2`, representative mid-width, and configured maximum;
- randomized input insertion and hash-map construction order;
- randomized independent batch/chunk order;
- cold, warm, partial warm, and pressure-evicted states;
- cancellation at instrumented checkpoints;
- native threaded and WASM/local execution where supported.

A smaller count requires a stronger systematic interleaving proof accepted in the block charter.

## 6.3 Stable IDs versus handles

Tests assert:

- stable IDs do not depend on allocation, traversal, worker, hash seed, cache warmth, or serialization insertion order;
- session handles fail validation outside their cohort/generation and are never compared as stable IDs;
- handle reuse cannot alias a live entity;
- graph export IDs remain deterministic across direct/managed and native/WASM when the protocol claims portability.

## 6.4 Canonical serialization

Round-trip and byte-equality tests cover:

- map/set ordering;
- optional/default fields;
- enum discriminants;
- unknown-field policy;
- protobuf map avoidance/sorting on canonical paths;
- canonical error/diagnostic arguments;
- compression settings where compressed bytes are the product;
- malicious/reordered equivalent input payloads.

# 7. Work conservation and zero-work proof

## 7.1 Required counters

At minimum, record actual executions per logical identity for:

- carrier and language-domain parse;
- source-unit preparation and placement reconciliation;
- options/path/source canonicalization and digest construction;
- operation planning and plan hashing;
- projection demand discovery, batch aggregation, route/load, substitution/relation/inference;
- graph build, derived projection build, demand plan, each activated flow domain, fixed-point iteration, syntax reacquisition;
- template/style parse, fact extraction, edit planning, materialization;
- code generation, required projection map, runtime map generation, map encoding;
- diagnostics collection and text rendering;
- provenance, serialization, NAPI/WASM conversion;
- query-identity candidate lookup, candidate validation/rejection, and bounded candidate replacement;
- cache lookup/validation/admission and flight production/join;
- executor job creation and owner-call messages.

## 7.2 Amplification

For family `F` in one declared live ownership window:

```text
amplification(F) = actual executions / distinct demanded logical identities
```

Default required value is `1.0` for exact live parse, plan, projection, graph, CSS parse, output generation, and terminal product construction.

An amplification above `1.0` is accepted only when:

- the identities are genuinely distinct;
- eviction ended the live window and the same-key reparse is visible;
- failure/retry is required by the load protocol;
- the `A6`-locked `performance-gates.toml` cell explicitly proves recomputation is lower total cost than carrying/reusing the value;
- the product contract requests distinct materializations.

“Cheap” or “implementation simplicity” without measured total cost is not an exception.

## 7.3 Query identity versus attempt identity assertions

```text
same QueryIdentity + unrelated snapshot change
=> prior candidate remains discoverable
=> positive/negative facts decide reuse
```

```text
same QueryIdentity + different InputBasisId while a producer is running
=> no semantic-flight join by default
```

```text
changed positive or negative observed fact
=> prior candidate rejected
=> recompute under the new SemanticFlightKey
```

```text
terminal presentation/serialization change only
=> semantic typed candidate reused
=> only terminal materialization recomputed
```

## 7.4 Mandatory zero-work assertions

```text
Vue/Svelte compile with zero native projection demand
=> zero CompileTypeInfo construction
=> zero semantic projection call
```

```text
Svelte runtime compile under current capability
=> zero Vue projection demand
```

```text
same ParseKey in one live ParseOwnerDomainId
=> one parse invocation
```

```text
separate ordinary direct one-shot calls
=> no process-global parse cache/lock required
```

```text
IDE + build for same bytes and parse contract in one shared owner domain
=> one frontend parse
=> different validation/lowering allowed
```

```text
pressure-evicted parse requested concurrently
=> at most one same-key reparse flight in that owner domain
```

```text
IDE companion requested
=> required SourceProjectionMap constructed exactly once
```

```text
runtime external source maps disabled and not required by product
=> zero RuntimeSourceMapData generation
=> zero external map encoding
```

```text
provenance/serialization/rendered diagnostics disabled
=> zero corresponding terminal materialization and retained artifacts
```

```text
CSS exact identity + N consumers
=> one live syntax parse
```

```text
unrequested optional native enrichment
=> zero semantic work, retained values, formatted events, and attributable allocations
```

# 8. Performance decision methodology

## 8.1 Gate immutability

`A6` freezes populated cells in `performance-gates.toml` against the exact post-A3, post-instrumentation implementation baseline before `B1` and every later non-safety foundational cutover. A gate may be recalibrated only when:

- machine/toolchain/corpus/competitor changed materially;
- raw before/after calibration is retained;
- no candidate result was inspected before choosing the new threshold;
- an independent performance reviewer accepts the change;
- the affected block is re-baselined or restarted.

A candidate cannot choose its pass criterion after measurement.

## 8.2 Benchmark cell identity

Each cell fixes:

- source/corpus fingerprint and size distribution;
- framework, product contract, semantic profile, mappings, diagnostics, provenance, serialization;
- direct/prepared/managed state and warm definition;
- threading/execution profile;
- build mode/toolchain/features/allocator;
- boundary surface: Rust, NAPI, WASM, CLI, LSP;
- exact measured statistic and sample policy;
- work/copy/allocation validity assertions;
- absolute and relative gate.

Results from different cells are never combined without a declared aggregation rule.

## 8.3 Sampling and statistics

Default rules are frozen by the `A6` Implementation Lock Record and `performance-gates.toml`:

- at least 30 interleaved measured samples for short cells after declared warmup;
- at least 10 independent long-cell runs when practical;
- bootstrap 95% confidence interval over the declared statistic;
- no-regression upper slowdown bound no greater than `max(3%, 2 × measured noise floor)` unless the locked cell is tighter;
- p95/p99 only with sufficient observations;
- predefined outlier policy; no discretionary deletion after seeing direction;
- machine drift checks using a stable control benchmark;
- report indistinguishable results as indistinguishable.

For process-level peak RSS, isolate processes and record allocator/platform behavior. For CPU, report process CPU and wall time. For parallel cells, report efficiency and work counts.

## 8.4 Required benchmark families

### Direct compiler

- tiny, medium, large, adversarial, and many-file unique corpora;
- one-thread loop and max configured width;
- Vue/Svelte and each claimed runtime/IDE/public product;
- no projection, local projection, imported projection;
- maps/diagnostics/provenance/serialization off/on as separate cells;
- direct one-shot and prepared first/repeat;
- source-only and project load-wave/retry;
- Rust core, NAPI, and WASM separately.

### Managed compiler

- cold content/session and validated warm;
- one-character edits in script/template/style;
- unit move without byte change;
- dependency/config/project edit;
- create/delete/rename/reopen;
- queue/background load and interactive priority;
- pressure eviction and reparse;
- stale/cancelled work.

### TypeInfo and flow

- script/index/kernel baseline;
- no-flow position;
- each activated domain family;
- closure/loop/completion/call/context;
- local/imported Vue projection;
- cold/warm/recomputed/partial;
- deep/budgeted/cancelled;
- derived projection versus direct graph traversal where promotion is proposed.

### CSS

- parse/index/format/Vue/Svelte plans;
- exact shared identity with multiple consumers;
- transformed new identity;
- maps/provenance off/on;
- large/recovery/adversarial dialect fixtures.

### LSP/provider

- edit-to-companion, completion, hover, diagnostics, rename/navigation where claimed;
- provider acknowledgement and stale rejection;
- provider restart/route transition/project reload;
- background indexing under interactive load;
- queue saturation/cancellation;
- native enrichment off/on/delayed/failing.

### Boundaries

- native Rust result;
- diagnostics/rendering;
- map generation versus encoding;
- JSON/protobuf/binary;
- NAPI/WASM host copies and heap delta;
- cancellation/supersession before and during conversion.

## 8.5 Competitor/Pareto proof

A competitor row is valid only when it records:

- exact source revision/version and build flags;
- corpus and target equivalence;
- output validity and supported semantics;
- source maps/diagnostics/imported-type behavior;
- threading and cache state;
- boundary included/excluded;
- raw result and uncertainty.

Verter must first pass absolute SLO and self no-regression. It is blocking-dominated when one valid competitor is materially faster **and** lower peak RSS under the locked tolerance while doing equivalent work.

The `A6` lock sets exact aggregate and strategic-cell tolerances before candidate implementation. The target is to meet or beat the fastest valid equivalent-work Rust implementation on the primary direct-suite aggregate and to avoid material Pareto domination in strategic cells. A candidate miss is blocking and cannot be waived after results are known. If indispensable extra work or a comparison mismatch proves the locked product/equivalence premise false, the project must amend the product/architecture and Implementation Lock Record under the blind recalibration rule, invalidate affected candidate evidence, and restart. Repeated work, weaker semantics, invalid comparison, or unbounded retention are never acceptable premises.

# 9. Allocation, copy, arena, stack, and boundary proof

Every strategic cell reports:

- allocation count and requested bytes;
- live and peak logical bytes by owner;
- source/input bytes copied and number of source-sized buffers;
- output/map/serialization temporary bytes;
- arena capacity versus live payload and oversized pool discards;
- `Arc` clones/atomic traffic/lock contention/channel messages where attributable;
- worker count × reserved stack and measured high-water where practical;
- NAPI/WASM/host heap delta and transfer copies.

Required negative assertions:

- primary direct Rust source is borrowed;
- no AST/source clone exists merely to satisfy `Send`, `Sync`, or `'static`;
- no public output pins an OXC/request arena;
- independently evictable entries do not share a lifetime-pinning arena;
- terminal representations are not built for cancelled/superseded/unrequested products;
- one explicit safe boundary copy is not mislabeled as failure when zero-copy would require unsafe lifetime coupling.

# 10. Bounded-memory and soak contract

## 10.1 Owner metrics

Every retained owner exposes:

- logical live, pinned, and evictable bytes;
- entry/count by compatibility/input generation;
- admission/refusal/hit/miss/validation/eviction;
- pin count/reason, oldest pin age, generation age, last-use age;
- superseded-but-pinned bytes;
- parse/reparse, graph/projection, semantic cohort, interner, queue, flight, provider, tombstone, and audit counts;
- configured soft/hard bound and trim result.

## 10.2 Workloads

Soaks include:

- repeated edits across tiny and large files;
- create/delete/rename/move/reopen and same-content new incarnation;
- project/workspace-folder add/remove/reload;
- dependency/config/library changes;
- provider restart and mode transition;
- TypeInfo/flow query storms;
- cancellation/supersession and abandoned waiters;
- pressure and admission refusal;
- formatter/CSS/compiler/LSP mixed activity;
- idle periods and explicit quiescence/trim protocol.

`L1` minimum durations/work counts are fixed by the `A6` Implementation Lock Record and the accepted `L1` charter before the soak candidate runs.

## 10.3 Acceptance

After warm-up and quiescence:

- logical bytes are within owner budgets plus attributable live pins;
- no statistically meaningful positive long-run slope exists in live logical bytes/counts;
- superseded generations become reclaimable after live readers end;
- all remaining pins have current source/request/provider reasons;
- RSS remains inside the platform/allocator plateau envelope;
- queue/flight/tombstone/interner/audit counts do not grow monotonically;
- clean-equivalence samples remain green;
- no restart is needed for cleanup.

# 11. QueryRuntime, flight, cancellation, and executor proof

Test at minimum:

- warm hit inline with no executor task;
- cold same-key producer with many followers;
- first waiter cancels while followers continue;
- all waiters cancel and producer cooperatively stops;
- follower deadline/budget differs from producer aggregate;
- additional budget while `Running` extends the bounded producer without changing output;
- higher-budget request after budget finalization uses a successor flight;
- ordinary budget never selects an approximation or prunes a required obligation;
- priority elevation and safe lowering;
- producer panic/internal failure/budget/stale/cancel outcome resolves every waiter once and admits nothing;
- no self-wait/cycle deadlock;
- same semantic arguments under incompatible `ResultContractId` do not share a query identity or flight;
- same `QueryIdentity` on different `InputBasisId` values does not join in flight by default;
- a bounded cached candidate produced on an older basis may be found by `QueryIdentity` but is used only after positive/negative fact validation;
- content artifact may join across snapshots when identity is immutable;
- follower validates completed value against its own admissible view;
- shutdown empties flight table and owner queues;
- owner-affine command does not move AST/arena;
- tiny dependent work stays inline;
- chunk/fork threshold, fan-out, queue bound, and interactive capacity under background saturation;
- native threaded and WASM/local state machines produce equivalent outcomes.

Model-based/state-machine tests should cover every legal transition and reject double-finalization, waiter loss, publication after failure, and producer lifetime tied to first requester.

# 12. Incremental, snapshot, and publication proof

## 12.1 Input commits

Assert:

- readers observe complete root before or after commit, never mixed subroots;
- concurrent commits do not lose updates;
- parser/provider/semantic work is absent from writer critical section;
- document ranges apply only to matching incarnation/version;
- version gaps enter explicit unsynchronized state;
- removed inputs immediately leave authority.

## 12.2 Incremental-clean equivalence

For every supported edit class:

```text
incremental final products(final committed inputs)
== clean final products(final committed inputs)
```

Compare code, required mappings, diagnostics, public DTOs, dependencies, exactness, stable IDs, and canonical serialization.

Test script/template/style-only edits, unit move, dependency change, project/config change, external template/style, fallback, pressure eviction, and repeated edit sequences.

## 12.3 Publication

Adversarially interleave:

- rapid edits during compile/provider work;
- stale native-TypeScript/tsserver/extension response;
- provider epoch transition;
- companion ready before map and map ready before companion;
- mapping supersession;
- dependency input change;
- close/reopen same content with new incarnation;
- provider off and native enrichment on/off/delayed/failing;
- queue saturation and cancellation.

No stale, torn, incompletely mapped, or mixed-provider result may publish.

# 13. CSS and formatter proof

For every claimed dialect/operation:

- Native/External/Unsupported capability is explicit;
- exact parse identity and live owner domain recorded;
- formatter output parses under the same frontend and is idempotent;
- comments/trivia/recovery behavior preserved according to contract;
- range formatting changes only a structurally safe range;
- index/navigation facts match syntax/recovery completeness;
- Vue `v-bind`, modules, scoping/keyframes, and Svelte style consumers reuse syntax where bytes/profile match;
- changed transform/preprocessor output receives a new identity;
- semantic transforms refuse when recovery cannot prove structure;
- source-map/provenance zero-work assertions pass;
- direct and managed results are equivalent.

# 14. Failure, trust, and adversarial proof

Test boundaries with malformed, hostile, deep, huge, cyclic, and inconsistent input:

- parser recovery and deterministic parse failure;
- stack-depth/explicit-stack limits;
- semantic recursion/work budgets;
- giant unions/intersections/generic recursion/flow loops;
- huge LoadSet and dependency cycles;
- invalid UTF-8 at byte-oriented boundaries where applicable;
- path traversal, symlink/case/realpath ambiguity, package exports cycles;
- malformed/untrusted persistent or precomputed payloads;
- oversized graph/protocol/diagnostic/map payloads;
- panic in parser/compiler/semantic/provider/FFI adapter;
- provider crash/hang/protocol violation;
- cancellation at all long-loop and boundary checkpoints;
- shutdown during work;
- corrupt cache entry and digest mismatch;
- session handle from wrong generation/cohort.

Requirements:

- no undefined behavior or unsafe lifetime escape;
- no cache/persistence admission after panic/cancel/stale/budget/internal failure;
- no process-global poison that permanently breaks unrelated operations;
- no sensitive ambient filesystem/network/process access from direct/compiler/semantic core;
- typed bounded failure with deterministic code/basis;
- malformed external data is size/integrity/compatibility checked before allocation/semantic use.

# 15. Compatibility and persistence proof

For every retained compatibility domain:

- owner, scope, epoch, schema/algorithm, producer, consumers, and migration policy recorded;
- monotonic epoch behavior proven;
- no duplicate authority required to remain numerically equal;
- internal no-boundary counters deleted;
- old persisted/public payload behavior matches accepted migration/rejection policy;
- semantic/output/presentation/serialization identities are not conflated;
- precomputed facts validate root/batch/profile/kernel/input/dependency/exactness/integrity/size basis;
- compatibility mismatch fails closed before use;
- canonical serialization byte tests pass;
- persistence is disabled for values lacking hermetic complete positive/negative facts.

# 16. Complexity, concepts, and deletion report

Every block reports before/after:

- production owner/service types;
- traits and dynamic dispatch points;
- public/crate-visible entry points;
- caches/maps/interners;
- locks/atomics/concurrent maps/channels;
- queues/pools/background tasks;
- revision/epoch/token/handle types;
- semantic/syntax/control representations;
- source-sized buffers and materialization passes;
- reachable production paths and runtime selectors;
- lines added/deleted and dependencies added/removed;
- tests/guards/docs/comments added/deleted;
- cold/warm latency, CPU, allocations, copies, peak/retained memory.

A net concept increase must correspond to an explicit accepted capability/invariant that the prior model could not represent. Moving complexity behind new names is a failure.

# 17. Orchestration, program-state, stack, and landing proof

Every accepted block proves its delivery process as well as its code:

- Revision 11 package validation passed from the exact extracted package;
- `program-state.toml` contains every DAG block exactly once and validates before start, review, acceptance recommendation, and acceptance;
- only blocks with accepted predecessors become active, except contingent `READY`/`IN_PROGRESS`/`REVIEW` upper layers whose unaccepted predecessors are lower layers in the same validated immutable stack snapshot; no such upper layer reaches acceptance recommendation before predecessor landing/restack, and before `A6` no post-Gate-0 block is active;
- the context packet, charter, program-state, base SHA/tree, candidate SHA/tree, and evidence digests agree;
- one writable worktree/branch/worker owns the mutation surface; clean-tree proof includes generated and untracked files;
- shared generated files, lockfiles, protocols, and dependency-firewall files had one writer lease;
- the orchestrator did not count its own implementation/synthesis as independent review or maintainer acceptance;
- the stack window is bounded, maps every layer to a block/charter, and contains no hidden unaccepted cross-stack dependency;
- every mergeable layer is independently releasable and passes required checks on its cumulative tree;
- private atomic layers remain draft/non-mergeable and never reach trunk independently;
- `D1` is recorded only as a private checkpoint and `D2` is the atomic public landing;
- every lower-layer change records old/new base/tree, patch/range-diff, conflict/manual edits, regenerated outputs, CI reruns, and review reattestation;
- no approval transfers automatically across a restack or candidate change;
- a `LANDABLE` stack lands only its lowest eligible layer and then issues a successor snapshot for remaining dependants; an `ATOMIC_REVIEW` stack lands only its final candidate;
- the actual accepted commit/tree is bound to the reviewed candidate through exact canonical candidate-delta equality on recorded bases, matching generated-output digests, and required post-landing checks; full-tree equality is not assumed after a base advance;
- program state records actual accepted commit/tree, the landing-equivalence digest, and invalidates/restacks remaining dependent work.

Adversarial delivery tests include an out-of-order block start, stale program state, duplicate block entry, lower-layer restack after upper approval, manual rebase conflict, hidden generated diff, two workers targeting one branch, private-layer merge attempt, merge-queue rebase, failed post-merge candidate-delta equivalence, and missing maintainer authority. Each must fail closed.

# 18. Final acceptance matrix

`L4` requires all rows below on one exact candidate SHA/tree, exact base tree, and evidence digest.

| Area | Required proof |
|---|---|
| Authority | Revision 11 digest, accepted ADRs, no contradiction or unresolved public/identity/lifetime gate |
| Candidate | exact provenance, clean tree, non-vacuous canonical commands |
| Dependency | forbidden edges/cycles absent; direct core independent of managed engine |
| Syntax | one shared error-tolerant frontend per language domain; scoped parse owner/reparse proof |
| Compiler | compositional products, borrowed direct core, prepared/resumable transaction, exact mappings |
| TypeInfo | one profile-parameterized kernel/resolver path; sealed compile facade; load waves bounded |
| Flow | one graph authority, one production solver, demand domains, structural returns, closure/completion correctness |
| Completeness | partial/unsupported/stale/cancelled/budgeted/panicked work cannot admit complete |
| Public API | operation DTOs primary, optional bounded graph export, stable IDs separate from handles, no general TypeExpr |
| Query/runtime | final InputStore basis, value facts, FlightCell, bounded executor/owner affinity |
| Incremental | immutable stable-unit reuse and clean final equivalence |
| CSS | one syntax authority per exact identity, deterministic formatter/index, explicit preprocessing |
| Frameworks | typed Vue/Svelte boundaries, no final Any bag, synthetic alternate-shape fixture |
| Providers/LSP | project-scoped non-racing route, atomic companion/map, stale rejection |
| FFI/checker | deterministic safe NAPI/WASM conversion; narrow verter_tsc checker boundary |
| Work | amplification and all zero-work assertions green |
| Performance | absolute SLO, self no-regression, equivalent-work Pareto/competitor gates green |
| Memory | L1 plateau and pin attribution; no restart cleanup |
| Failure | adversarial, panic, cancellation, untrusted decode, deep-input containment |
| Complexity | negative-net architecture or accepted capability rationale; old paths and campaign machinery gone |
| Delivery | validated program state, bounded stack windows, worktree isolation, restack reattestation, atomic private layers, reviewed-to-accepted candidate-delta equivalence |
| Review | required exact-SHA/tree conformance, architecture, and adversarial/performance approvals plus maintainer acceptance |

A row marked “not applicable” requires a contract citation and evidence that the product/capability is unsupported rather than silently skipped.


---

# Consolidated source: `templates/implementation-lock-record.md`

# Verter Revision 11 Implementation Lock Record

**Status:** Draft | Accepted | Superseded  
**Record ID:**  
**Entry checkout SHA:**  
**Implementation baseline SHA:**  
**Implementation baseline tree OID:**  
**Record digest:**  
**Authority package digest:**  
**Program DAG digest:**  
**Accepted program-state digest:**  
**Accepted by/date:**

# 1. Repository and authority

- entry checkout SHA / clean state:
- ordered Gate 0 commits since entry checkout:
- exact implementation baseline SHA and tree / clean state:
- evidence refreshed after each affected Gate 0 SHA change:
- open PR/branch disposition:
- Rust/Node/TypeScript/framework/provider toolchains:
- Revision 11 document digests:
- designated maintainer and orchestrator:
- evidence root and custody policy:

# 2. Non-vacuous command manifest

| Command ID | Exact command/features/target | Discovered | Executed | Skip/ignore policy | Sentinel proof | Raw log |
|---|---|---:|---:|---|---|---|

# 3. Capability and maturity matrix

| Framework/surface | Operation/product | Complete/Partial/Unsupported | Profile | Oracle/evidence | Public contract |
|---|---|---|---|---|---|

# 4. Identity and profile lock

- canonical digest schema/domain epoch:
- source/unit/syntax/parse/placement identities:
- stable ID collision/equality policy:
- TypeScript option-classification table locations:
- output/presentation/serialization/execution profile schemas:
- unknown option behavior:

# 5. Compatibility and protocol lock

| Domain | Owner | Current namespace/epoch | Consumers | Persistence/public | Evolve or replace | Migration/rejection |
|---|---|---|---|---|---|---|

- semantic graph protocol disposition:
- `TypeExpr` consumer inventory disposition:
- parser/artifact/cache/schema duplicate authority disposition:

# 6. Dependency and owner baseline

- crate/module dependency graph artifact:
- forbidden edges/tests:
- owner/service/cache/lock/queue/path concept inventory:
- current IDE/build parser/frontends:
- current direct/managed/FFI routes:

# 7. Work, performance, and memory lock

- `A4` counter schema/version:
- baseline raw result location:
- accepted `performance-gates.toml` digest:
- benchmark runner/machine class:
- competitor versions/builds/corpus equivalence:
- owner memory budgets and allocator slack:
- soak workloads/durations/seeds:

# 8. Semantic safety state

- U6 G1–G11 harness location/result:
- wrong-complete rows retracted:
- typed gap/admission behavior:
- remaining unsupported rows:

# 9. Orchestration, worktree, CI, review, and stack lock

- designated maintainer and acceptance channel:
- orchestrator identity/harness and permissions:
- maximum active worker contexts:
- context-packet storage/digest policy:
- program-state path, writer, validation command, and transition policy:
- worktree/branch naming and one-writer lease policy:
- generated-file/lockfile/protocol central-writer policy:
- GitHub branch protection, required checks, merge queue, signed-commit/rebase facts:
- stack implementation (`github-native`, `gh-stack`, ordinary dependent PRs, or none):
- default/max stack window and larger-window approval rule:
- restack/range-diff/CI/review invalidation rule:
- atomic private-layer and D1/D2 landing rule:
- post-merge candidate-delta/generated-output equivalence verification:

| Initial block | Context packet digest | Worktree/branch | Stack/layer | Worker | Review mandates |
|---|---|---|---|---|---|

# 10. First unlocked charters

| Block | Charter digest | Implementation baseline SHA | Review class | Scoper/challenger | Predecessors verified |
|---|---|---|---|---|---|

Required immediately eligible charter set: `B1`, and optionally `J1` when CSS work is selected. Successor charters may be pre-scoped. Contingent stacked draft/review work is legal only under the validated stack contract; no successor may become acceptance-recommended until every predecessor is formally satisfied and the candidate is restacked/revalidated. No later block is unlocked merely by being listed here.

# 11. Unresolved items

Every item here must be a private implementation choice that cannot change semantics, identity, lifetime, cache validity, mapping interpretation, compatibility, dependency direction, or pass/fail gates.

| Item | Why non-blocking | Owner | Resolution point |
|---|---|---|---|

# 12. Acceptance checklist

- [ ] exact entry checkout, exact implementation baseline, tree OID, record digest, and authority digest recorded
- [ ] all canonical commands non-vacuous
- [ ] capability/protocol/consumer inventory complete
- [ ] identity/profile/compatibility decisions accepted
- [ ] performance gate file contains no placeholders/zero-required fields
- [ ] raw baseline and noise measurements retained
- [ ] semantic safety retraction complete
- [ ] maintainer/orchestrator identities and program-state/evidence custody accepted
- [ ] worktree/branch/CI/merge/stack/restack policy accepted
- [ ] first foundational charters/context packets/stack placement accepted
- [ ] no unresolved public/semantic/identity/lifetime/cache/compatibility/gate issue
- [ ] exact SHA/tree architecture and adversarial evidence accepted
- [ ] no agent may self-accept, weaken gates, or merge private atomic layers independently


The lock record is an immutable evidence artifact addressed by `Record digest`. It may be stored outside the implementation commit to avoid self-reference. If later committed into the repository, that documentation commit does not replace the recorded implementation baseline unless a new A6 lock is accepted.


---

# Consolidated source: `templates/landing-equivalence.template.toml`

```toml
# Verter Revision 11 landing-equivalence proof template.
# Resolve every REQUIRED_* value, set status = "PROVEN", validate this file,
# then record this file's SHA-256 in the post-landing program-state update.
# The proof intentionally binds only the pre-landing program-state digest;
# binding the post-landing state would create a hash cycle.

schema = 1
revision = 11
status = "TEMPLATE"
authority_package_digest = "REQUIRED_PACKAGE_SHA256"
implementation_lock_digest = "REQUIRED_A6_LOCK_SHA256"
block_id = "REQUIRED_BLOCK_ID"
target_branch = "REQUIRED_TARGET_BRANCH"
merge_method = "merge_queue|merge_commit|squash|fast_forward"
reviewed_base_sha = "REQUIRED_FULL_SHA"
reviewed_base_tree = "REQUIRED_TREE_OID"
reviewed_candidate_sha = "REQUIRED_FULL_SHA"
reviewed_candidate_tree = "REQUIRED_TREE_OID"
accepted_base_sha = "REQUIRED_FULL_SHA"
accepted_base_tree = "REQUIRED_TREE_OID"
accepted_sha = "REQUIRED_FULL_SHA"
accepted_tree = "REQUIRED_TREE_OID"
candidate_patch_digest = "REQUIRED_CANONICAL_GIT_DIFF_SHA256"
accepted_patch_digest = "REQUIRED_CANONICAL_GIT_DIFF_SHA256"
candidate_generated_digest = "NOT_APPLICABLE_OR_REQUIRED_GENERATED_OUTPUT_SHA256"
accepted_generated_digest = "NOT_APPLICABLE_OR_REQUIRED_GENERATED_OUTPUT_SHA256"
review_snapshot_digest = "REQUIRED_REVIEW_SNAPSHOT_SHA256"
candidate_evidence_digest = "REQUIRED_CANDIDATE_EVIDENCE_SHA256"
maintainer_acceptance_digest = "REQUIRED_MAINTAINER_ACCEPTANCE_SHA256"
program_state_before_digest = "REQUIRED_PRE_LANDING_PROGRAM_STATE_SHA256"
stack_snapshot_digest = "NOT_APPLICABLE_OR_REQUIRED_STACK_SNAPSHOT_SHA256"
manual_conflict_resolution_after_review = false
generated_tree_clean = true
notes = ""

# Canonical patch digest is SHA-256 of:
# git -c core.quotePath=true diff --binary --full-index --no-renames
#     --no-color --no-ext-diff --no-textconv <base_sha> <head_sha> -- .
# evaluated in the repository with LC_ALL=C. Candidate and accepted patch
# digests, plus generated-output digests, must match exactly. Base and full-tree
# identities may differ after a legitimate base advance; any changed delta or
# post-review manual conflict requires a new frozen candidate and review. The
# accepted_base_sha must be the accepted commit's first parent, so one program
# block lands as one merge/squash/queue/one-commit-fast-forward delta.

# Each required post-landing command must be represented once. `exit_code` is
# normally zero; a nonzero expected assertion is allowed only when the command
# and charter explicitly define that result as success.
[[post_landing_check]]
command_id = "REQUIRED_STABLE_COMMAND_ID"
command = "REQUIRED_EXACT_COMMAND"
exit_code = 0
non_vacuous_count = "REQUIRED_POSITIVE_INTEGER_OR_NOT_APPLICABLE"
evidence_digest = "REQUIRED_RAW_OUTPUT_SHA256"
```


---

# Consolidated source: `templates/performance-gates.template.toml`

```toml
# Verter Revision 11 performance-gate template.
# A6 copies this file to the implementation repository as performance-gates.toml.
# The locked file must set status = "LOCKED", contain no REQUIRED_* values, and
# pass tools/validate_performance_gates.py. Metrics not listed for a cell are
# measured when useful but are not pass/fail gates for that cell.

schema = 1
revision = 11
status = "TEMPLATE"
authority_digest = "REQUIRED_AUTHORITY_DIGEST"
baseline_sha = "REQUIRED_BASELINE_SHA"
created_at_utc = "REQUIRED_TIMESTAMP"

[runner]
class = "REQUIRED_RUNNER_CLASS"
os = "REQUIRED_OS"
cpu = "REQUIRED_CPU"
logical_cpus = "REQUIRED_INTEGER"
memory_bytes = "REQUIRED_INTEGER"
rust_toolchain = "REQUIRED_RUST_TOOLCHAIN"
node_runtime = "REQUIRED_NODE_RUNTIME"
power_policy = "REQUIRED_POWER_POLICY"
control_benchmark = "REQUIRED_CONTROL_BENCHMARK"
max_control_drift_percent = "REQUIRED_NUMBER"

[statistics]
short_min_samples = 30
long_min_runs = 10
confidence = 0.95
bootstrap_resamples = 10000
no_regression_floor_percent = 3.0
noise_multiplier = 2.0
outlier_policy = "REQUIRED_PREDECLARED_POLICY"
interleave_policy = "REQUIRED_INTERLEAVE_POLICY"

[primary_suite]
id = "REQUIRED_PRIMARY_SUITE_ID"
cell_ids = ["REQUIRED_CELL_ID"]
aggregate = "geomean_ratio"
competitor_ids = ["REQUIRED_COMPETITOR_ID"]
max_verter_to_fastest_ratio = "REQUIRED_NUMBER"
post_result_exception_allowed = false
premise_change_requires_new_lock = true

[[cell]]
id = "REQUIRED_CELL_ID"
owner = "REQUIRED_OWNER"
operation = "REQUIRED_OPERATION"
corpus_fingerprint = "REQUIRED_CORPUS_DIGEST"
normalized_product_request_digest = "REQUIRED_PRODUCT_REQUEST_DIGEST"
result_contract = "REQUIRED_RESULT_CONTRACT"
semantic_profile = "REQUIRED_SEMANTIC_PROFILE_OR_NONE"
execution_profile = "REQUIRED_EXECUTION_PROFILE"
cache_state = "REQUIRED_COLD_WARM_PREPARED_STATE"
threads = "REQUIRED_INTEGER"
boundary = "rust|napi|wasm|lsp|cli"
required = true

[cell.validity]
required_product_kinds = ["REQUIRED_PRODUCT_KIND"]
required_output_profiles = ["REQUIRED_OUTPUT_PROFILE"]
required_presentation_profiles = []
required_serialization_profiles = []
required_mapping_kinds = []
required_diagnostics_policy = "REQUIRED_POLICY"
required_exactness = "REQUIRED_EXACTNESS"
output_oracle = "REQUIRED_ORACLE"
zero_counter_assertions = []

# Add only metrics that are actual pass/fail gates for this cell.
# `limit` is a number in the locked file. A literal zero is valid when zero is
# the intended gate (for example, zero source bytes copied).
[[cell.metric]]
name = "wall_ns"
statistic = "median"
comparison = "absolute_max"
limit = "REQUIRED_NUMBER"

[[cell.metric]]
name = "wall_ns"
statistic = "median"
comparison = "no_regression_percent_max"
limit = "REQUIRED_NUMBER"

[[cell.metric]]
name = "peak_rss_bytes"
statistic = "max"
comparison = "absolute_max"
limit = "REQUIRED_NUMBER"

# Work/copy/allocation gates use the same metric form.
[[cell.metric]]
name = "parse_amplification"
statistic = "max"
comparison = "absolute_max"
limit = 1.0

[[cell.metric]]
name = "source_bytes_copied"
statistic = "max"
comparison = "absolute_max"
limit = "REQUIRED_NUMBER"

[cell.competitor]
rule = "none|pareto|suite_geomean|both"
competitor_ids = []
max_wall_slowdown_percent = "REQUIRED_NUMBER_OR_NOT_APPLICABLE"
max_peak_rss_increase_percent = "REQUIRED_NUMBER_OR_NOT_APPLICABLE"
post_result_exception_allowed = false

[cell.memory]
owner_budget_bytes = "REQUIRED_INTEGER_OR_NOT_APPLICABLE"
allocator_slack_bytes = "REQUIRED_INTEGER_OR_NOT_APPLICABLE"
quiescence_protocol = "REQUIRED_PROTOCOL_OR_NOT_APPLICABLE"
max_positive_slope_bytes_per_hour = "REQUIRED_NUMBER_OR_NOT_APPLICABLE"
```


---

# Consolidated source: `templates/program-state.template.toml`

```toml
# Verter Revision 11 program-state template.
# A0 copies this into the repository evidence area, replaces REQUIRED_* values,
# updates state only through the orchestrator, and validates it after every change.

schema = 1
revision = 11
status = "TEMPLATE"
authority_package_digest = "REQUIRED_PACKAGE_DIGEST"
release_report_digest = "REQUIRED_RELEASE_REPORT_DIGEST"
program_dag_digest = "REQUIRED_DAG_DIGEST"
entry_checkout_sha = ""
entry_checkout_tree = ""
implementation_baseline_sha = ""
implementation_baseline_tree = ""
implementation_lock_digest = ""
performance_gates_digest = ""
architecture_premise_ledger_digest = ""
current_block = "A0"

[repository]
remote = "REQUIRED_REMOTE"
branch = "REQUIRED_BRANCH"
head_sha = "REQUIRED_FULL_SHA"
head_tree = "REQUIRED_TREE_OID"
dirty = false
untracked_count = 0

[orchestration]
maintainer = "REQUIRED_MAINTAINER"
orchestrator = "REQUIRED_ORCHESTRATOR"
requested_model_id = "claude-opus-5"
actual_model_id = "REQUIRED_ACTUAL_MODEL_ID"
orchestrator_runtime = "REQUIRED_RUNTIME"
orchestrator_runtime_version = "REQUIRED_RUNTIME_VERSION"
provider = "REQUIRED_PROVIDER"
fallback_or_substitution = false
model_exception_accepted = false
max_active_workers = 3
max_open_stack_layers = 4
stack_tool = "UNDECIDED_UNTIL_A6"
stack_mode_policy = "UNDECIDED_UNTIL_A6"
evidence_root = "REQUIRED_REPOSITORY_RELATIVE_PATH"

# Status: LOCKED | READY | IN_PROGRESS | REVIEW | ACCEPTANCE_RECOMMENDED | ACCEPTED |
#         BLOCKED | RESCOPE_REQUIRED | ABORTED | SUPERSEDED | PRIVATE_CHECKPOINT
# Reviews: NOT_REQUIRED | PENDING | PASS | BLOCKING | NOT_PROVEN | INVALIDATED
# candidate_sha/tree = exact reviewed cumulative candidate.
# accepted_sha/tree = actual landed commit and full repository tree; they may differ
# from the reviewed candidate after a base advance. landing_equivalence_digest binds
# both base/candidate/accepted identities and exact candidate-delta/post-landing proof.

[[block]]
id = "A0"
status = "READY"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "A1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "A2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "A3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "A4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "A5"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "A6"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "B1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "B2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "B3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "B4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "B5"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "B6"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "C1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "C2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "C3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "C4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D5"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D6"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D7"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "D8"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "E1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "E2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "E3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "E4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "F1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "F2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "G1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "G2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "G3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "G4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "G5"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "H1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "H2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "H3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "J1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "J2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "J3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "J4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "K1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "K2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "K3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "L1"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "L2"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "L3"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""

[[block]]
id = "L4"
status = "LOCKED"
charter_digest = ""
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = ""
```


---

# Consolidated source: `templates/architecture-premise-ledger.template.md`

# Revision 11 Architecture Premise Ledger

**Status:** Draft | Accepted | Superseded  
**A6 Lock ID:**  
**Implementation baseline SHA:**  
**Ledger digest:**

# Decision classes

- `CONSTITUTIONAL`
- `REPOSITORY_FACT`
- `TACTIC`
- `DEFERRED_RESEARCH`

# Premises

| ID | Class | Statement | Evidence/source | Confidence/uncertainty | Falsification trigger/test | Allowed alternatives | Affected blocks/gates | Action | Owner |
|---|---|---|---|---|---|---|---|---|---|
| PREM-001 |  |  |  |  |  |  |  |  |  |

# Acceptance

- [ ] every architecture-significant repository assumption is represented
- [ ] every performance-sensitive tactic has a falsification test
- [ ] constitutional invariants cite architecture/ADR authority
- [ ] deferred choices cannot enter an active charter without amendment
- [ ] affected blocks and invalidation behavior are explicit
- [ ] independent architecture challenger reviewed the ledger


---

# Consolidated source: `templates/stack-window.template.toml`

```toml
schema = 1
revision = 11
status = "TEMPLATE"

# The immutable StackSnapshotId is the SHA-256 of this fully resolved,
# validated file. LANDABLE is one connected dependency path whose layers may
# merge independently. ATOMIC_REVIEW has private checkpoints and one final
# mergeable acceptance layer.
mode = "LANDABLE"
stack_id = "REQUIRED_STACK_ID"
acceptance_block_id = ""
authority_package_digest = "REQUIRED_PACKAGE_SHA256"
implementation_lock_digest = "REQUIRED_A6_LOCK_SHA256"
program_state_basis_digest = "REQUIRED_PRE_STACK_PROGRAM_STATE_SHA256"
previous_stack_snapshot_digest = "NOT_APPLICABLE"
root_branch = "REQUIRED_ROOT_BRANCH"
root_base_sha = "REQUIRED_FULL_SHA"
root_base_tree = "REQUIRED_TREE_OID"
stack_tool = "REQUIRED_STACK_TOOL"
stack_tool_version = "REQUIRED_STACK_TOOL_VERSION"
landing_mode = "bottom-up"
max_open_layers = 4
owner = "REQUIRED_ORCHESTRATOR_OR_STACK_OWNER"
evidence_root = "REQUIRED_REPOSITORY_RELATIVE_PATH"
shared_writer_surfaces = []
integration_commands = []
notes = ""

[[layer]]
index = 1
layer_id = "B1"
block_id = "B1"
charter_digest = "REQUIRED_B1_CHARTER_DIGEST"
kind = "mergeable"
branch = "REQUIRED_B1_BRANCH"
base_branch = "REQUIRED_ROOT_BRANCH"
worktree = "REQUIRED_B1_WORKTREE_ID_OR_PATH"
worker = "REQUIRED_B1_WORKER"
pr_number = 0
pr_url = ""
base_sha = "REQUIRED_FULL_SHA"
base_tree = "REQUIRED_TREE_OID"
head_sha = ""
head_tree = ""
patch_digest = ""
generated_digest = ""
evidence_digest = ""
ci_state = "PENDING"
review_state = "PENDING"
mergeable = true
notes = ""

[[layer]]
index = 2
layer_id = "B2"
block_id = "B2"
charter_digest = "REQUIRED_B2_CHARTER_DIGEST"
kind = "mergeable"
branch = "REQUIRED_B2_BRANCH"
base_branch = "REQUIRED_B1_BRANCH"
worktree = "REQUIRED_B2_WORKTREE_ID_OR_PATH"
worker = "REQUIRED_B2_WORKER"
pr_number = 0
pr_url = ""
base_sha = "REQUIRED_B1_HEAD_SHA"
base_tree = "REQUIRED_B1_HEAD_TREE_OID"
head_sha = ""
head_tree = ""
patch_digest = ""
generated_digest = ""
evidence_digest = ""
ci_state = "PENDING"
review_state = "PENDING"
mergeable = true
notes = ""
```


---

# Consolidated source: `templates/block-charter.md`

# Verter Revision 11 Block Charter

**Block:**  
**Review class:** Local | Subsystem | Foundational  
**Frozen implementation baseline SHA/tree:**  
**Authority package / A6 lock ID:**  
**Program-state digest:**  
**Direct predecessors and accepted SHA/tree/evidence:**  
**Context-packet digest:**  
**Stack window/layer/kind:**  
**Writable worktree/branch/worker:**  
**Target base and candidate SHA/tree:**

# 1. Owner and invariant

- current owner/authority:
- invariant being changed:
- product operations/result contracts:
- semantic/output/presentation/serialization/execution profiles:

# 2. Current closure

- readers/writers/callers:
- source/project/identity basis:
- lifetime/thread/platform boundaries:
- caches/flights/tasks/queues/pools:
- public/cross-crate/wire consumers:
- persistence/compatibility domains:

# 3. Scope

- consumers in scope:
- explicit exclusions:
- final ownership/dependency/lifetime direction:
- private construction/testing boundary before cutover:
- rejected alternatives and evidence:

# 4. Deletion and cutover

- APIs/types/modules:
- implementations/call sites:
- caches/keys/tasks/metrics/flags:
- dependencies/re-exports/schema fields:
- fixtures/guards/scanners/docs/comments:
- exact clean-cutover sequence:
- rollback before cutover / recovery after failed candidate:

# 5. Proof

- correctness/conformance/failure:
- determinism/direct-prepared-managed equivalence:
- incremental/stale-publication:
- work/copy/allocation counters:
- applicable locked performance cells and thresholds:
- memory/retention/soak:
- native/WASM/NAPI/FFI:
- compatibility/migration:
- negative admission/unsupported behavior:

# 6. Delivery and review

- branch protection / required CI:
- stack/restack and range-diff policy:
- mergeability / atomic landing group:
- generated-file and shared-writer leases:
- post-merge target-tree verification:

## 6.1 Review

- architecture challenger result:
- conformance reviewer mandate:
- architecture reviewer mandate:
- adversarial/performance reviewer mandate:
- scope cone:
- block-specific causal blockers:

# 7. Abort/rescope conditions

List every assumption whose failure requires work to stop. Include identity omissions, undeclared consumers, dependency cycles, dual-path need, unsafe lifetime, missing gate, or invalid benchmark equivalence.

# 8. Completion report

- candidate SHA:
- raw evidence locations/digests:
- before/after concept inventory:
- deletion proof:
- review state by exact SHA/tree and evidence digest:
- latest restack old/new base, range-diff, and reattestation:
- non-blocking discoveries/disposition:


---

# Consolidated source: `templates/context-packet.md`

# Verter Revision 11 Worker Context Packet

**Packet digest:**  
**Created from program-state digest:**  
**Role:** Scoper | Implementor | Conformance reviewer | Architecture reviewer | Adversarial/performance reviewer | Investigator  
**Block / charter:**  
**Stack window / StackSnapshotId / layer_id / acceptance block:**  
**Writable worktree / branch:**  
**Maintainer:**  
**Orchestrator:**

# 1. Exact identities

- authority package digest:
- A6 Implementation Lock digest or `PRE-A6`:
- entry checkout SHA/tree:
- implementation baseline SHA/tree or `UNSET`:
- block base SHA/tree:
- current candidate SHA/tree or `UNSET`:
- charter digest:
- relevant predecessor accepted SHAs/trees/evidence digests:

# 2. Assigned objective

One paragraph stating exactly what must become true.

# 3. Current source facts

- current authorities/readers/writers:
- exact files/symbols/contracts already inspected:
- current behavior/capability status:
- known open PR/branch conflicts and disposition:

Do not substitute the implementor/orchestrator summary for direct source inspection when the role requires independent evidence.

# 4. Allowed write set

- files/modules/generated outputs allowed:
- dependency/lockfile/protocol changes allowed:
- branch/history operations allowed:

Everything else is read-only unless the orchestrator accepts a rescope.

# 5. Forbidden changes

- architecture/ADR/gate weakening:
- scope widening or unrelated cleanup:
- compatibility shim, shadow path, runtime switch, alternate authority:
- ambient I/O, secret/permission changes, or unowned worktree mutation:
- self-approval or review-result fabrication:

# 6. Required end state and deletions

- surviving owner/path/API:
- old declarations/implementations/caches/tasks/metrics/flags/docs to delete:
- public/protocol/compatibility consequences:
- exact one-path/atomicity invariant:

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|

Include correctness, determinism, work/copy/allocation, performance, memory, platform, failure, dependency, generated-file, and clean-tree proof applicable to the charter.

# 8. Review scope and output

- mandatory changed surface:
- required dependency/owner closure:
- causal blocker rule:
- output format:

# 9. Stop/rescope conditions

List exact facts that require stopping rather than improvising.

# 10. Handoff result

Return the block record required by `contracts/agent-orchestration.md`, with raw evidence paths/digests and no unsupported success claim.


---

# Consolidated source: `templates/pr-description.md`

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


---

# Consolidated source: `templates/review-report.md`

# Verter Revision 11 Review Report

**Block:**  
**Mandate:** Conformance | Architecture | Adversarial/Performance  
**Baseline SHA:**  
**Candidate SHA:**  
**Charter digest:**  
**Authority/A6 lock ID:**

# Result

Choose exactly one:

- PASS
- BLOCKING FINDINGS
- NOT PROVEN
- NON-BLOCKING DISCOVERIES

# Mandatory evidence checked

- changed/deleted/replacement surface:
- direct caller/owner/lifetime/dependency closure:
- promised deletion set:
- applicable test/benchmark/memory/failure outputs:
- exact-SHA and non-vacuous execution:

# Blocking finding template

```text
Finding:
Severity:
Candidate cause:
Authority/charter requirement violated:
Affected behavior/invariant:
Evidence/reproduction:
Minimum correction condition:
```

# Non-blocking discovery template

```text
Class: DISC-CORRECTNESS | DISC-ARCH | DISC-PERF | DISC-DOC | DISC-INVESTIGATE
Finding:
Why outside current causal boundary:
Suggested disposition:
```


---

# Consolidated source: `charters/A0.md`

# A0 — Adopt Revision 11 and freeze the exact checkout

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational.  
**Predecessors:** none.  
**Entry checkout SHA:** `UNSET` until A0.

## Objective

Produce an exact entry-checkout lock, validate the Revision 11 package and program ledger, identify the maintainer/orchestrator and delivery permissions, and disposition every architecture-affecting open change. The implementation baseline is accepted later by A6 after Gate 0 source changes.

## In scope

- only the deliverables named for `A0` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Completed entry lock; package/DAG digests; clean SHA/tree/worktree/submodule proof; open PR/branch/queue disposition; GitHub/CI/stack/signing permission inventory; designated maintainer/orchestrator; initialized and validated program state; A0 context/evidence packet.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A0` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.


---

# Consolidated source: `charters/A1.md`

# A1 — Prove non-vacuous commands and capability truth

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational.  
**Predecessors:** A0.  
**Gate 0 lineage SHA:** `UNSET`; record the exact candidate for this evidence block.

## Objective

Establish that canonical commands execute intended work and that product capability claims are explicit.

## In scope

- only the deliverables named for `A1` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Raw command records with counts/digests; completed affected capability rows.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A1` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.


---

# Consolidated source: `charters/A2.md`

# A2 — Strengthen the U6 public cold/warm differential harness

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational.  
**Predecessors:** A0, A1.  
**Gate 0 lineage SHA:** `UNSET`; record the exact candidate for this evidence block.

## Objective

Make known semantic and admission defects mechanically discriminating.

## In scope

- only the deliverables named for `A2` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Exact expectations; negative controls; crossed matrices; oracle/profile stamps.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A2` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.


---

# Consolidated source: `charters/A3.md`

# A3 — Retract known wrong-complete results

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational safety.  
**Predecessors:** A2.  
**Gate 0 lineage SHA:** `UNSET`; record the exact candidate for this evidence block.

## Objective

Replace known fabricated complete results with typed non-admissible outcomes without selecting the final flow owner.

## In scope

- only the deliverables named for `A3` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Cold/warm public tests; no semantic-any fallback; no warm admission.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A3` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.


---

# Consolidated source: `charters/A4.md`

# A4 — Install measurement-only attribution and capture the baseline

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational.  
**Predecessors:** A3.  
**Gate 0 lineage SHA:** `UNSET`; record the exact candidate for this evidence block.

## Objective

On the exact post-A3 lineage, make work, copies, allocations, tasks, retention, and determinism observable without changing semantics.

## In scope

- only the deliverables named for `A4` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Counter schema; disabled-overhead proof; baseline dataset and raw profiles.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A4` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA.


---

# Consolidated source: `charters/A5.md`

# A5 — Complete owner, identity, profile, compatibility, protocol, and consumer inventories

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational.  
**Predecessors:** A4.  
**Gate 0 lineage SHA:** `UNSET`; record the exact candidate for this evidence block.

## Objective

Against the exact post-safety, instrumented lineage, remove all hidden implementation decisions from later charters.

## In scope

- only the deliverables named for `A5` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Resolved current-owner rows; option classification; domains; consumer/protocol map; dependency test plan; evidence custody; program-state workflow; worktree/branch/CI/merge rules; bounded stack-window and review-context policy.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A5` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.


---

# Consolidated source: `charters/A6.md`

# A6 — Accept the Implementation Lock Record

**Status:** PREPARED; complete from the implementation checkout.  
**Class:** Foundational.  
**Predecessors:** A5.  
**Implementation baseline SHA:** `UNSET` until A6 accepts the post-Gate-0 candidate.

## Objective

Accept one exact post-Gate-0 implementation baseline, program inputs, and numeric gates before foundational candidates exist.

## In scope

- only the deliverables named for `A6` in `program.md`;
- evidence and source changes strictly necessary to produce those deliverables;
- deletion of obsolete evidence-only scaffolding created by this block before acceptance.

## Out of scope

- later production ownership or API cutovers;
- compatibility shims, parallel architecture paths, or speculative services;
- numeric threshold selection after candidate direction is observed.

## Required evidence

Maintainer-accepted immutable lock record; exact entry and implementation SHA/tree; package/DAG/program-state digests; locked performance-gates.toml; resolved orchestration/permission/worktree/CI/merge/stack policy; first block charters/context packets/stack placement; exact independent review state.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from the charter assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A6` is accepted only when its evidence is attached to one unchanged candidate SHA/tree and evidence digest.


---

# Consolidated source: `charters/B1.template.md`

# B1 — Neutral Contracts, Typed Identities, Profile Schemas, and Dependency Firewall

**Status:** TEMPLATE; A6 must bind source paths, current owners, gates, and reviewers.  
**Class:** Foundational.  
**Predecessor:** `A6`.

## Objective

Land dependency-neutral types and canonical encodings needed by later blocks without creating service owners or migrating semantic behavior.

## Required final state

- distinct identity/profile/mapping/result-contract types from `architecture.md`;
- canonical encoding golden vectors shared by native/WASM where applicable;
- `StableEntityId` and `SessionHandle` non-interchangeable;
- `QueryIdentity` distinct from `SemanticFlightKey` and `InputBasisId`;
- forbidden crate/module edges and cycles fail builds;
- no global revision/request/deadline/budget smuggled into reusable identity;
- no current semantic/cache/parser owner duplicated merely to host new types.

## Source-specific closure to fill at A6

- current crates/modules that own IDs/options/mapping/version wrappers;
- every public/wire consumer;
- compatibility domains and migration/deletion set;
- exact dependency graph before/after;
- benchmark cells for canonicalization/hash/key construction overhead;
- native/WASM test targets.

## Cutover

Introduce the final typed contracts, migrate the bounded consumer closure, and delete displaced aliases/wrappers/counters in the same candidate. Do not add conversion layers intended to survive.

## Abort/rescope

Stop for an undiscovered public/wire consumer, incompatible persisted domain, dependency cycle, missing canonical equality material, or a field whose profile/identity class is ambiguous.


---

# Consolidated source: `charters/J1.template.md`

# J1 — Reconcile CSS Syntax, Formatter, Scanner, and Transform Owners

**Status:** TEMPLATE; optional first parallel block after A6.  
**Class:** Subsystem, promoted to Foundational if it changes shared syntax ownership or public compatibility.  
**Predecessors:** `A4`, `A6`.

## Objective

Establish the exact current CSS authority and consumer graph before changing parse, plan, transform, formatter, mapping, or preprocessor behavior.

## Required evidence

- exact `StyleSyntaxIr` producer and all consumers;
- specialized fast paths and whether they share the same syntax authority;
- dialect/recovery/formatter/index/navigation capabilities;
- Vue/Svelte transform and `CodeTransform` mapping routes;
- external preprocessor inputs/maps/dependencies/fingerprints;
- parse/pass/materialization/copy/allocation baseline by exact style identity;
- current public/wire compatibility obligations.

## Required disposition

Each current path is `Preserve`, `Converge`, `Replace`, `Delete`, or `Defer`. A specialized path is not deleted solely for conceptual uniformity; it survives only when it cannot invent syntax meaning and wins equivalent-work evidence.

## Abort/rescope

Stop if source evidence shows multiple semantic syntax authorities, an unaccounted dialect/product contract, or a mapping/preprocessor owner outside the charter closure.


---

# Consolidated source: `implementation-readiness-review-v10.md`

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


---

# Consolidated source: `implementation-readiness-review-v9.md`

# Revision 9 Implementation-Readiness Review

**Verdict:** Revision 9 was architecturally strong but **not ready for unrestricted implementation**. It was safe to begin baseline measurement, characterization, and urgent fail-closed corrections. It was not safe to delegate all foundational cutovers because several cross-owner contracts remained open.

# 1. What Revision 9 got right

Revision 9 correctly established the durable direction:

- a real borrowed direct compiler distinct from the managed engine;
- minimum prerequisite closure and exact live-artifact reuse;
- one semantic authority with deterministic derived projections;
- a concrete sealed compile-semantic facade;
- TypeScript semantic profiles;
- operation-specific DTOs and optional graph export;
- demand-selected flow domains;
- value-side validation and return-only results;
- owner-affine local state and bounded retention;
- terminal rendering, source maps, provenance, serialization, and FFI;
- no permanent dual production paths;
- reproducible performance and long-running memory proof.

Those decisions are preserved in Revision 11.

# 2. Blocking readiness findings

## R9-B1 — Ratification status was contradictory

The manifest and execution documents treated ADRs as architecture authority while the ADRs themselves were marked `Proposed`. An implementor could not know whether a conflicting implementation detail required an ADR amendment.

**Revision 11 correction:** every durable ADR is `Accepted`; the manifest defines exact precedence and amendment rules.

## R9-B2 — The dependency graph was not executable

Revision 9 described a DAG but did not provide a complete machine-readable predecessor relation. Important orderings—input authority before managed query convergence, final flow before TypeExpr cutover, and numeric gates before hot-path implementation—were not unambiguously enforced.

**Revision 11 correction:** `program-dag.toml` defines every block and predecessor; `program.md` explains each edge and abort condition.

## R9-B3 — Flow migration could preserve two production authorities

Revision 9’s sequence built graph/domain/semantic blocks and deleted displaced flow paths only at the end. Accepted intermediate merges could therefore retain the old evaluator as the production authority while a second nearly complete solver existed, or expose both for comparison.

**Revision 11 correction:** `D1` builds only a private hermetic foundation. `D2` is one indivisible public cutover: all public flow operations move to the new solver and the old evaluator, caches, tasks, flags, and guards are deleted. Later blocks expand only the sole solver; unsupported behavior remains typed and non-admissible.

## R9-B4 — Parse identity could encode consumer duplication

A parse `role`/`flavor` dimension could allow IDE and runtime consumers to create distinct parses even when construction semantics were identical. The phrase “one live parse identity” also did not state whether the scope was process-global, per direct invocation, per prepared value, or per managed worker.

**Revision 11 correction:** `ParseKey` contains only syntax-construction dimensions. `ParseOwnerDomainId` separately names direct invocation/batch, `PreparedCarrier`, or managed owner/shard. The invariant is exactly one live `(owner domain, parse key)` result; direct calls do not acquire a hidden process-global cache.

## R9-B5 — Compiler products were not fully compositional

A single artifact enum could not cleanly express simultaneous runtime, IDE, public API, declarations, diagnostics, mapping, and serialization requests without hidden widening or repeated planning.

**Revision 11 correction:** compiler products are canonical typed per-product requests. Each owns only the output and terminal profiles that affect it; required mappings are part of the product, optional presentation/serialization is product-local, and equal subrequests share reusable subplans. Unsupported, duplicate, or irrelevant combinations fail before expensive work.

## R9-B6 — Project-aware compile was not a closed transaction

Revision 9 mentioned `NeedInputs(LoadSet)` but did not bind plans and facts strongly enough to the prepared root, request, profile, projection schema, and input basis. Retry/no-progress/resource limits and stale fact rejection were under-specified.

**Revision 11 correction:** `prepare -> plan -> project -> emit` is an explicit staged protocol. `CompilePlanToken` and facts carry anti-replay bases. Input loading is batched, coherent, bounded, and restarts from a new snapshot rather than splicing observations into an existing attempt.

## R9-B7 — Profile and policy dimensions were conflated

Semantic interpretation, code generation, presentation, serialization, and execution limits have different compatibility and cache consequences. A general policy identity risked both over-keying and reuse of a weaker result as complete.

**Revision 11 correction:** `TypeScriptSemanticProfileId`, `OutputProfileId`, `PresentationProfileId`, `SerializationProfileId`, `ResultContractId`, and waiter-local `ExecutionPolicy` are distinct. Ordinary deadlines, cancellation, priority, and budgets do not enter reusable result identity; exhaustion is partial/failure.

## R9-B8 — Stable IDs and session handles were conflated

Deterministic cross-regime identity and cohort-local continuation handles cannot share one promise. Raw graph/arena IDs are not stable and keeping them stable can pin storage.

**Revision 11 correction:** `StableEntityId` has a canonical deterministic basis; `SessionHandle` is opaque, generation/cohort-bound, and excluded from cross-session equality.

## R9-B9 — Mapping contracts were ambiguous

IDE/provider companions require exact projection mappings as part of their atomic product. Runtime source maps are optional terminal materialization. Treating both as “source maps” makes a zero-work assertion either false or unsafe.

**Revision 11 correction:** `PlacementMap`, required `SourceProjectionMap`, optional `RuntimeSourceMapData`, and terminal `EncodedSourceMap` are separate types and lifecycle contracts.

## R9-B10 — Shared-flight behavior was not a complete state machine

“Waiter-owned” was directionally correct but did not fully close producer ownership, registration, cancellation, priority/budget aggregation, finalization, panic, shutdown, admission, and cross-snapshot joining.

**Revision 11 correction:** the `FlightCell` owns production. Semantic query flights are exact-`InputBasisId` scoped by default; immutable content flights may cross snapshots. Every transition finalizes once, resolves every waiter, and admits only through the owner’s sealed path.

## R9-B11 — Reusable query identity and exact-basis flight identity were not cleanly separated

An exact snapshot/input basis is necessary for safe in-flight joining, but putting the whole basis into reusable cache lookup makes a still-valid candidate undiscoverable after an unrelated edit. Omitting it from a running semantic flight, however, permits joining work whose eventual read set is not yet known.

**Revision 11 correction:** snapshot-independent `QueryIdentity<Q>` discovers bounded candidates, which are then positive/negative-fact validated. `SemanticFlightKey<Q>` adds the exact `InputBasisId` for default in-flight production.

## R9-B12 — Completeness proof construction remained too abstract

A private proof token is insufficient unless the set of required semantic obligations is closed and the finalizer can prove every obligation was discharged under the same graph, demand, profile, and input basis.

**Revision 11 correction:** each operation has a closed static domain registry, an obligation ledger, typed discharge states, deterministic fixed-point completion, and one private finalizer. Unknown obligations cannot be silently ignored or converted to semantic `any`.

## R9-B13 — Binding dependency direction was not locked

Without a binding crate/module direction, the compiler, semantic facade, session, and framework DTOs could create cycles that force public traits, `Any`, `Arc`, locks, and alternate semantic callbacks.

**Revision 11 correction:** identity/contracts and syntax point inward; semantic kernel is dependency-neutral; compiler depends on the sealed semantic facade; managed/session and adapters depend inward only. Build tests reject forbidden edges and cycles.

## R9-B14 — Performance gates could move after implementation began

Revision 9 had good methodology but no mandatory pre-candidate record containing exact numeric cells, machine/corpus identities, and competitor rules. A candidate could influence its own acceptance threshold.

**Revision 11 correction:** `A6` freezes one machine-readable `performance-gates.toml` and Implementation Lock Record before foundational cutovers. Gate relaxation after seeing candidate results is prohibited; benchmark defects require baseline and candidate reruns.


# 3. Final Revision 11 preflight corrections

Before publication, Revision 11 itself received two additional closure fixes:

- Gate 0 is one ordered lineage. `A0` captures the entry checkout; command/harness work and `A3` safety retraction precede `A4` measurement and `A5` final inventories; `A6` accepts one exact post-Gate-0 implementation baseline. Earlier evidence is refreshed when affected by a SHA change.
- compact source units and mapping contracts now land before the borrowed direct compiler, and the source-sized blanking path is deleted in that bounded cutover. The new public direct route therefore never earns acceptance while depending on a knowingly transitional source model.

# 4. Readiness decision

Revision 11 is ready to adopt and execute at Gate 0. It intentionally does **not** authorize broad architecture changes immediately. That is a feature, not incompleteness: the exact repository state, current-owner dispositions, executable commands, compatibility obligations, and numeric performance gates can only be truthfully fixed from the implementation checkout.

After `A6`, a block becomes implementable only when its predecessor set, charter, current-tree closure, tests, numeric gates, deletion set, and independent reviews are complete. This prevents “following the architecture” while inventing unresolved behavior in code.

# 5. Review limitation

This review examined the architecture documents and frozen source evidence but did not execute Verter’s full Rust/TypeScript/NAPI/WASM suites, differential corpus, benchmarks, provider matrix, or long-running memory soak. Revision 11 turns those into explicit implementation gates rather than treating them as already proven.


---

# Consolidated source: `baseline/9af553dd.md`

# Frozen Baseline Evidence — `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0`

**Captured:** 9 August 2026  
**Commit:** `feat(typeinfo): support type narrowing (#94)`  
**Preceded by:** #100 CSS parser convergence and #101 cache/read-set work.

This document records source-shape observations used to design Revision 11. It is evidence for one SHA, not durable architecture. A later architecture-affecting baseline requires a new document and reconciliation.

# 1. Review limitations

The Revision 11 review reviewed the uploaded Revision 8 plan, current GitHub source at the frozen SHA, current repository README, relevant U6 architecture/gap documents, and official TypeScript 7.0 release guidance.

The full repository test suite, benchmark suite, memory soak, provider conformance matrix, and differential corpus were **not executed as part of this document review**. Runtime/performance assertions remain unproven until Gate 0 captures exact command output.

# 2. Repository/product observations

## 2.1 Historical source identity

Revision 8 explicitly froze `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` as its evidence baseline. This document records observations for that exact source identity. It does **not** assert that a current local checkout or current public `main` still points to it. `A0` must freeze and reconcile the implementation checkout independently.

## 2.2 Product surfaces

The repository exposes:

- Rust compiler/semantic/session/LSP/MCP crates;
- NAPI native bindings;
- WASM bindings;
- Vue tooling and compiler surfaces;
- external TypeScript provider integration;
- component metadata and graph/DTO surfaces;
- experimental pinned Svelte native compilation.

Consequences for architecture:

- native-threaded assumptions cannot define semantics for all surfaces;
- FFI and WASM ownership/copy behavior need independent proof;
- Svelte has architectural citizenship but current maturity is not equal to mature Vue claims;
- public/wire compatibility domains have real consumers and cannot be treated as purely internal counters.

# 3. Flow source observations

## 3.1 `FunctionFlowGraph` intends to be canonical

Source:

```text
crates/verter_semantic/src/analysis/flow/flow_graph.rs
```

Observed contract:

- graph is built once per function content version from `FunctionBodySkeleton` alone;
- graph build cannot re-walk the AST, observe query demand, lower a type, or produce a semantic fact;
- typed value/effect/control edge classes define demand reachability;
- source comments explicitly reserve future narrowing, closure, loop, and try/finally classes on the same graph and forbid a second flow structure.

This is a strong foundation and should remain the semantic structural authority.

## 3.2 A second syntax-shaped evaluator exists

Source:

```text
crates/verter_session/src/flow_slice_content.rs
```

Observed shape:

- the module lowers selected content into an owned typed IR with a block/if control-flow tree;
- it owns sequential reachability, terminal return/throw behavior, block nesting, switch/try/labeled handling, and unsupported markers;
- `SliceRegion`, `SliceStatement`, and `SliceExpr` form a second control/content representation;
- semantic behavior varies by parent/expression position in current approximations.

This directly supports Revision 11's decision to delete the second semantic evaluator rather than evolve it.

## 3.3 The measured U6 gap document confirms wrong-and-warm defects

Source:

```text
docs/arch/u6-flow-return-gaps-and-target.md
```

Observed evidence:

- eleven checker divergences were measured through the public cold/warm API;
- all were admitted/replayed as complete;
- several fabricated `any`;
- root causes include subject emptiness used as control verdict, missing nominal unique-symbol identity, position-specific effects, capture-as-value-snapshot, boolean completion, and unmodelled content converted to `any`;
- most required structural facts already exist, while nested capture writes are the principal producer gap;
- the document itself prescribes structural returns, completion algebra, binding-identity state, capture summaries, shared relation authority, and proof-carrying completeness.

Revision 11 preserves those semantic corrections but replaces a potentially monolithic lattice with demand-selected product domains and explicitly permits derived non-semantic graph projections.

## 3.4 Current plan/hash/lower path replans

Source:

```text
crates/verter_session/src/cache_runtime/flow_slice_node.rs
```

Observed contract:

- a hash node runs the demand planner and hashes the selected subgraph;
- a lowered node re-runs the planner and describes the replan as cheap and deterministic;
- over-budget plans are return-only/non-admitted.

The non-admission policy is strong. The replan remains a performance hypothesis and must be measured against carrying the first plan.

## 3.5 Binding identities are already partially available

Source:

```text
crates/verter_semantic/src/analysis/function_program.rs
crates/verter_semantic/src/analysis/flow/flow_ir.rs
```

Observed shape:

- function programs/skeletons carry stable binding identities and structural inventories;
- capture resolution can use defining frame plus binding slot;
- some flow carriers still include name data and optional slot resolution.

This makes a binding-identity semantic cutover feasible without inventing a new source-wide identity authority.

# 4. Cache/query source observations

## 4.1 Same-key singleflight is already serious infrastructure

Source:

```text
crates/verter_session/src/cache_runtime/singleflight.rs
```

Observed strengths:

- exactly one producer per key;
- followers join;
- panic safety and post-compute view validation are considered;
- the implementation documents what it does not own.

Observed revision need:

- shared-flight ownership under leader cancellation/follower continuation and priority inheritance needs an explicit architecture contract;
- current generic bounds and blocking synchronization should not become mandatory semantics for local/WASM execution;
- production comments contain migration/campaign guidance that should not survive final architecture.

## 4.2 Scheduler/host surface is large

The frozen scheduler/session implementation contains extensive task taxonomy, shared maps/locks/pools, generation/lifecycle behavior, and source/analysis/artifact coordination. Revision 11 does not assume all of it is wrong. It requires Gate 0 ownership/work evidence and then converges only responsibilities whose complete replacement exists.

The target remains:

- query owners define semantic dependencies and validity;
- executor queues contain prerequisite-ready CPU work;
- input authority owns committed state;
- provider actors own external protocol state;
- no second scheduler semantic DAG remains.

# 5. Public TypeInfo/wire observations

Source:

```text
crates/verter_protocol/proto/verter/v1/typeinfo.proto
```

Observed shape:

- a protobuf-authoritative TypeInfo graph wire surface exists;
- Rust and TypeScript DTOs are generated;
- `SemanticTypeGraph` requires `schema_version`;
- stable numbered fields include nodes, symbols, signatures, origin edges, roots, exactness, diagnostics, ID maps, string table, and relation proof table;
- comments describe additive schema evolution and existing schema history.

Consequences:

- graph export is a real advanced product surface, not merely an internal structure;
- its compatibility domain needs deliberate evolution or explicit replacement;
- blanket in-place reset to epoch zero is unsafe as a general policy;
- simple TypeInfo operations should not be forced to emit this graph;
- internal semantic storage should not be constrained to mirror the protobuf layout.

# 6. CSS observations

The frozen baseline includes the post-#100 custom CSS-family syntax authority (`StyleSyntaxIr`) and no longer uses Lightning CSS as the target production authority. Revision 11 accepts that decision. Track G focuses on exact-identity reuse, shared syntax facts/plans, explicit new identities after transformed bytes, terminal materialization, and dialect capability proof.

# 7. TypeScript provider observations

Official TypeScript 7.0 release guidance states:

- TypeScript 7.0 is stable and native;
- it does not ship a programmatic API;
- a different API is expected in a later release;
- TypeScript 6 compatibility remains available side-by-side;
- embedded-language workflows such as Vue and Svelte likely need TypeScript 6 routes until suitable support exists.

Consequences:

- native TypeScript and tsserver/TypeScript 6 routes are both legitimate capability-dependent provider choices;
- “stable versus preview” is not the provider architecture;
- native TypeInfo compatibility must be stamped with a semantic profile rather than one global TypeScript assumption;
- `auto` selection must remain evidence-based and project-scoped.

# 8. Revision 8 plan contradictions/risks observed

## 8.1 Sealed facade versus public trait object

Revision 8 first defines a concrete sealed `CompileTypeInfo`, then later sketches `Option<&mut dyn CompileTypeInfo>`. Revision 11 resolves this in favor of the concrete sealed facade.

## 8.2 One graph/lattice wording

Revision 8 correctly rejects a second semantic evaluator but can be read as rejecting all derived topology/execution indexes or mandating an always-maximal lattice. Revision 11 distinguishes semantic authority from deterministic projections and uses demand-selected product domains.

## 8.3 Compatibility reset

Revision 8 deletes redundant counters correctly but resets retained published domains to zero. Revision 11 uses monotonic epochs within a domain and new domains for clean breaking replacement.

## 8.4 Mandatory graph protocol

Revision 8 places the canonical semantic graph protocol at the center of public TypeInfo. The frozen protocol is substantial and already versioned. Revision 11 makes operation DTOs primary and graph export explicit/advanced.

## 8.5 Program ordering

Revision 8 schedules QueryRuntime/executor convergence before the direct compiler. Revision 11 proves direct operation/artifact/identity boundaries first, then builds managed reuse around them.

## 8.6 Universal competitor threshold

Revision 8 uses a universal sustained 10% gap blocker. Revision 11 retains aggressive competitiveness but requires cell identity, absolute SLO, no-regression, uncertainty, and Pareto evidence.

# 9. Revision 9 implementation-readiness gaps reconciled in Revision 11

The Revision 9 split package was architecturally strong but not a closed implementation specification. Source-independent document review found:

- accepted authority referred to ADRs still marked Proposed;
- an incomplete predecessor graph and contradictory cutover ordering;
- a flow sequence that could require two production evaluators;
- parse role/flavor and process scope insufficiently separated;
- non-compositional compile requests;
- under-specified project input retry and plan/facts anti-replay;
- conflated semantic/presentation/serialization/execution identity;
- stable IDs conflated with session handles;
- required IDE projection maps conflated with optional runtime maps;
- shared-flight direction without a complete state machine;
- missing binding dependency direction;
- performance gates not frozen before foundational work.

Revision 11 addresses these in accepted ADRs, the Revision 11 contracts, and an explicit predecessor DAG.

# 10. Required Gate 0 measurements not yet proven

Before implementation claims are accepted, collect:

- pure direct versus host-backed/managed compilation decomposition;
- exact work amplification counts;
- source/FFI copies and allocations;
- parse/reparse ownership;
- flow plan/hash/replan/lower/evaluate counts;
- zero-work paths;
- deterministic output across schedules/profiles;
- queue/task/flight behavior;
- positive/negative read-set completeness;
- memory/pin/interner/cohort/tombstone plateau;
- native/WASM semantic and boundary equivalence;
- provider capability matrix by project/version/profile;
- compatibility-domain consumer inventory;
- discriminating U6 public cold/warm corpus.

# 11. Baseline conclusion

The frozen tree contains strong foundations worth preserving:

- structural flow graph/skeleton/index work;
- exact content identities and same-key flights;
- value-side fact/read-set validation;
- CSS syntax convergence;
- typed failures/non-admission in several cache paths;
- existing binding identities;
- closed macro DTO direction;
- native and WASM product surfaces that make ownership discipline valuable.

It also contains transition architecture that the implementation must not ratify:

- a second syntax-shaped flow evaluator;
- hash/replan/lower repetition without measurement;
- broad managed/session/scheduler ownership;
- migration-bound comments and generic concurrency bounds;
- general graph/wire surface pressure on simple APIs;
- framework erasure/blanking/options seams that are not final.

This baseline supports Revision 11's direction and its requirement to begin only with Gate 0. It does not itself prove the final architecture's performance or correctness outcomes.
