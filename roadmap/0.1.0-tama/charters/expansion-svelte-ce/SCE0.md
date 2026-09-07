<!-- unified-charter-v2
id=SCE0
name=Svelte Custom Element producer and consumer retrofit
phase=expansion
train=expansion.svelte-ce
product=svelte_ce
kind=terminal
semantic_role=delivery
class=successor
predecessors=HWC3,CPF1,SKL3
owner=expansion.svelte-ce:Svelte release-profile CustomElement producer and consumer adapter
conflict_domains=svelte_custom_element_producer_and_consumer_retrofit,svelte_product
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=product
external_requirements=
charter=charters/expansion-svelte-ce/SCE0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SCE0 — Svelte Custom Element producer and consumer retrofit

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Svelte Custom Element producer and consumer retrofit. The current owner is **the current source owners enumerated in the SCE0 migration manifest**. The final and sole owner is **Svelte release-profile CustomElement producer and consumer adapter**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src`, `crates/verter_language/src`.
- Named API/data boundaries: `Svelte Custom Element producer and consumer retrofit`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **HWC3:** implemented ledger row for “Web Component standards model, registry analysis, and CEM”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **CPF1:** implemented ledger row for “Carrier frontend registration and Vue/Svelte cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **SKL3:** implemented ledger row for “Maintainer-ratified atomic workflow activation”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Normative intent:** give the exact accepted Svelte release first-class CE production and consumption facts.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **the exact superseded SCE0 owner routes listed in the deletion manifest**, **the named SCE0 compatibility/fallback call sites in the zero-consumer search receipt** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **SCE0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **SCE0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **SCE0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **SCE0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **the exact superseded SCE0 owner routes listed in the deletion manifest**.
- Delete or structurally reject: **the named SCE0 compatibility/fallback call sites in the zero-consumer search receipt**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** give the exact accepted Svelte release first-class CE production and consumption facts.
**Predecessors:** `HWC3`, `CPF1`, `SKL3`.
**Subblocks:** (1) capture `<svelte:options customElement>` and admitted static compiler-option evidence; (2) model CE class/public surface and observed/reflecting attributes per locked release; (3) associate explicit registrations and consumer bindings; (4) contribute Svelte-owned evidence to HWC3, which solely projects standards facts and CEM output conforming to `CEF0`, then test TypeInfo/ComponentInfo/CEM results; (5) provide diagnostic/action/IDE/source-map cells; (6) test ordinary/CE variants, dynamic options, and scoped registries.
**Acceptance:** producer mode changes prepared-artifact identity when it changes projection; ordinary and CE variants never collide; unknown/dynamic options are typed incomplete; cross-framework consumers resolve standards facts.
**Forbidden:** compiler-output inference as source authority, a family-wide Svelte version switch, Vue semantics, vertical-owned CEM serialization, or a private formatter. CE mode does not change formatter semantics; its syntax is covered by ordinary Svelte fixtures in `FMTS0`.
**Deletion/abort:** delete only named Svelte profile rows/adapters after zero-consumer proof; shared schema/registry deletion belongs to `CEC0`; incompatible release behavior opens a separate release profile.

