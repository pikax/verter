<!-- unified-charter-v2
id=SST1
name=Svelte selector query plan and candidate-index architecture
phase=compiler
train=compiler.svelte-style
product=svelte_style
kind=implementation
semantic_role=delivery
class=compiler
predecessors=SCP2,SST0
owner=compiler.svelte-style:Svelte-owned adaptive matcher over canonical CSS/template facts
conflict_domains=style_semantics,svelte_product
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-svelte-style/SST1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SST1 — Svelte selector query plan and candidate-index architecture

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Svelte selector query plan and candidate-index architecture. The current owner is **Svelte style matching and source-stage glue**. The final and sole owner is **Svelte-owned adaptive matcher over canonical CSS/template facts**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_css_syntax/src`.
- Named API/data boundaries: `SvelteStylePlan`, `CandidateIndex`, `StyleMatchFact`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP2:** implemented ledger row for “Compact Svelte compiler structure and canonical template topology”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **SST0:** implemented ledger row for “Svelte framework style semantics and source-stage integration”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** compile selectors and template topology into a sound, data-oriented query workload without changing semantic answers.
- **Problem:** scanning every element for every selector and cloning path structures can dominate large components, while always building an index can regress small components.
- **Solution and architecture decisions:**
- exact matcher semantics remain framework-owned and authoritative;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **SST1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **SST1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **SST1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **SST1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_css_syntax/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **Svelte-local CSS parser**.
- Delete or structurally reject: **unbounded selector scan**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_css_syntax`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** compile selectors and template topology into a sound, data-oriented query workload without changing semantic answers.

**Problem:** scanning every element for every selector and cloning path structures can dominate large components, while always building an index can regress small components.

**Solution and architecture decisions:**

- exact matcher semantics remain framework-owned and authoritative;
- compile J selector structure into compact steps/subprogram ranges only when useful;
- use `SCP2` canonical topology, never runtime IR;
- define deterministic cost inputs:

  ```text
  template node count
  selector count and step count
  positive-anchor availability
  dynamic/wildcard ratio
  posting cardinalities
  ```

- support `DirectMatcher` and `IndexedMatcher`;
- indexed postings for sound positive tag/id/class/attribute keys;
- choose the rarest sound mandatory positive anchor using actual posting cardinality;
- negated predicates and unsafe pseudo branches never seed candidates;
- dynamic/spread/maybe buckets are explicitly unioned into candidate sets;
- query planning is demand-only and may be skipped for tiny workloads.

**Suggested predecessors:** `SCP2`, `SST0`.

**Normative source decomposition:** selector-step representation, direct matcher baseline, feature postings, candidate rules/dynamic buckets, deterministic cost model, differential/performance tests.

**Acceptance:** candidate selection has no false negatives; direct and indexed paths feed the same exact verifier; small workloads avoid index construction; all candidate/index work is ledger-visible.

**Forbidden:** probabilistic rejection, negated anchors, always-on indexing, universal selector semantics, or pruning from candidate selection alone.

**Deletion/abort:** preserve the exact matcher while replacing only physical execution; abort indexing if equivalent-work benefit is not demonstrated.

---

