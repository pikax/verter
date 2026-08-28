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
