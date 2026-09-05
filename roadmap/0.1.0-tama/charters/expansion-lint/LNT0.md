<!-- unified-charter-v2
id=LNT0
name=Native lint product and compatibility lock
phase=expansion
train=expansion.lint
product=lint
kind=lock
semantic_role=delivery
class=successor
predecessors=LRA0,CFG0
owner=expansion.lint:demand-driven native lint service with explicit external fallback
conflict_domains=diagnostic_action_service
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-lint/LNT0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LNT0 — Native lint product and compatibility lock

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Native lint product and compatibility lock. The current owner is **distributed diagnostics/fix rules**. The final and sole owner is **demand-driven native lint service with explicit external fallback**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics/src`, `crates/verter_actions/src`, `crates/verter_session/src`.
- Named API/data boundaries: `RuleId`, `LintRequest`, `DiagnosticFact`, `FixTransaction`, `SuppressionProvenance`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **LRA0:** implemented ledger row for “Profile-scoped diagnostics, lint, fixes, and actions”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **CFG0:** implemented ledger row for “Declarative Verter and captured ecosystem configuration”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Normative intent:** freeze the native/equivalent/external rule universe and product claims without inventing another lint engine.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Inventory evidence:** enumerate every in-scope outcome, consumer and displaced production route before validating ownership. Assign each outcome and consumer exactly one implementation owner, and each displaced production route exactly one later production-capable deletion/rejection owner, bound to its concrete DAG node, successor path and receiving acceptance criterion. This contract node proves inventory completeness and unambiguous ownership; later implementation nodes prove production deletion.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LNT0-AC1 — ownership contract:** enumerate every in-scope outcome, consumer and displaced production route. Each outcome/consumer has exactly one implementation owner; each displaced production route has exactly one later production-capable deletion/rejection owner. Bind owners to existing DAG node IDs, valid successor paths and receiving acceptance IDs under contracts/successor-charter-quality.md. The contract-owned schema/validator must reject missing inventory members, unknown/pathless owners and conflicting assignments before this node completes; production deletion remains acceptance of the later owner.
- **LNT0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LNT0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **LNT0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Inventory and assign the later deletion/rejection owner for: **implicit ESLint/Stylelint authority**. This node changes no production route.
- Inventory and assign the later deletion/rejection owner for: **unsafe overlapping fix application**. This node changes no production route.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_diagnostics -p verter_actions -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** freeze the native/equivalent/external rule universe and product claims without inventing another lint engine.
**Predecessors:** `LRA0`, `CFG0`.
**Subblocks:** (1) inventory current Verter rules and fixes; (2) pin ESLint, TypeScript-ESLint, eslint-plugin-vue, Svelte, Stylelint, and relevant framework rule versions; (3) classify NativeEquivalent/VerterOnly/ExternalOnly/Unsupported cells; (4) lock diagnostic/fix compatibility; (5) lock corpus/performance/zero-work gates; (6) ratify config and external-runner policy.
**Acceptance:** no blanket “ESLint compatible” claim; every rule ID has exact applicability, owner, fact demand, oracle, and fix safety.
**Forbidden:** running arbitrary plugins in core, claiming compatibility from similar names, or choosing easy rules after implementation.
**Deletion/abort:** no code; rescope incompatible semantic rules explicitly.

