<!-- unified-charter-v2
id=REL0
name=Milestone release-planning overlay and DAG scheduling
predecessors=GH2
conditional_predecessors=
phase=governance
train=governance.release-control
product=release_control
kind=implementation
semantic_role=delivery
class=successor
owner=governance.release-control:maintainer-owned milestone overlay applied only after DAG readiness
conflict_domains=release_orchestration,github_projection_state
resource_class=ts-heavy
gate_profile=canonical
review_profile=security-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
source_refs=source:github-control-plane-program.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/governance-release-control/REL0.md
size=M
max_production_loc=600
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# REL0 — Milestone release-planning overlay and DAG scheduling

Authority state is derived at dispatch. Release planning requires the current accepted ORC0 activation receipt and current GH2 projection receipt.

## Independently acceptable outcome

Implement milestones and Project views as maintainer-owned scheduling overlays applied only after the immutable DAG READY frontier. Pre-scope result: milestone interpretation, READY-only prioritization, and one long-lived Project 3 view model form one planning boundary; release readiness remains REL1.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `MilestoneOverlay`, `ReadySchedulingPlan`, `ReleaseTarget`, Project 3 view projection.

## Exact predecessor contracts

- **GH2:** exact current receipt ID and digest for “DAG → GitHub issue projection and reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Treat milestone as intended/earliest release, owned exclusively by maintainers; AI never moves it without explicit instruction.
- Compute READY nodes from `programctl`, then order only that set by milestone, critical path, conflict, and resource policy.
- Never use milestone to bypass a predecessor, activation, authorization, receipt, or lease.
- Use Project 3 as one long-lived project with execution, READY, triage, review/gate, train, milestone, and roadmap views.
- Keep release target on the issue rather than duplicating milestone assignment on the block PR.
- Mutable milestone movement cannot erase a finding carry-forward obligation or change P0/P1 status.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **REL0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **REL0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **REL0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **REL0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject milestone-as-predecessor, AI milestone movement, per-release Project creation, duplicate PR milestone assignment, and Project Status as authority.
- Do not determine release readiness, create release PRs, or execute workflows in this block.

## Budgets and mandatory rescope

- Target ceiling: 600 production LOC, 6 production files, 2 related packages.
- Rescope if planning mutates maintainer fields or creates a second DAG/frontier engine.
- Correctness budget: zero readiness bypass, unauthorized milestone write, duplicate planning authority, or finding loss.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation/frontier, missing Project identity, attempted milestone write without instruction, or any plan containing a non-READY node.
- Abort rather than infer a release waiver from mutable GitHub state.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Mutable GitHub milestone or issue state cannot dispose an immutable carry-forward obligation.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-REL0-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:488-545`
- Applicability: `REL0`
- Exact text SHA-256: `6d904be1f7c803d49fab7f7d37b30f542e506056091498565732ee3021920867`

~~~~markdown
REL0 — Milestone release-planning overlay and DAG scheduling
Predecessor: GH2

Milestones are maintainer-owned release planning.

The AI must never move an issue between milestones unless explicitly instructed by the maintainer.

Interpret an issue's milestone as its intended/earliest target release.

Examples:

```
0.1.0-beta.4
0.1.0-beta.5
0.1.0-beta.6
0.1.0-rc.1
0.1.0
```

Do not attempt to assign one issue to both beta and final. Later releases transitively contain earlier shipped work.

Use Project 3 as one long-lived project with views rather than creating a new project per release.

Useful views should include:

* DAG execution;
* READY frontier;
* non-DAG triage;
* review/gate;
* by train;
* by milestone/release;
* roadmap.

Milestones influence scheduling ONLY among already-READY DAG nodes.

Correct order:

```
programctl frontier
     ↓
READY nodes
     ↓
milestone priority
     ↓
critical-path/conflict/resource scheduling
```

Incorrect:

```
milestone says beta.4
     ↓
bypass DAG predecessor
```

Never do that.

Do not assign every block PR to the milestone if its issue is already assigned; that would double-count milestone progress. Keep the target release primarily on the work issue.
~~~~
