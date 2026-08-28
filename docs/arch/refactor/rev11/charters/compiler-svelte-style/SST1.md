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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L1547
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-style/SST1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SST1 — Svelte selector query plan and candidate-index architecture

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte selector query plan and candidate-index architecture. The current owner is **Svelte style matching and source-stage glue**. The final and sole owner is **Svelte-owned adaptive matcher over canonical CSS/template facts**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_css_syntax/src`.
- Named API/data boundaries: `SvelteStylePlan`, `CandidateIndex`, `StyleMatchFact`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP2:** exact current receipt ID and digest for “Compact Svelte compiler structure and canonical template topology”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SST0:** exact current receipt ID and digest for “Svelte framework style semantics and source-stage integration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

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

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_css_syntax`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1547`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1547-305FDF89D948

- Kind: `context`
- Source: `compiler-proposal.md:1547-1547`
- Applicability: `SST1`
- Exact text SHA-256: `305fdf89d948df7a64e08892356bc204e142e7a73270ea6c8d0471b8682e7e0f`

~~~~markdown
## `SST1.md` — Svelte selector query plan and candidate-index architecture
~~~~

### SRC-COMP-L1549-EC99F3CC08AC

- Kind: `context`
- Source: `compiler-proposal.md:1549-1549`
- Applicability: `SST1`
- Exact text SHA-256: `ec99f3cc08ac03e4fdc0603edc878cba54ea058814c3dacdd48a56c2c99877a9`

~~~~markdown
**Intent:** compile selectors and template topology into a sound, data-oriented query workload without changing semantic answers.
~~~~

### SRC-COMP-L1551-9BCE1C49B3FD

- Kind: `context`
- Source: `compiler-proposal.md:1551-1551`
- Applicability: `SST1`
- Exact text SHA-256: `9bce1c49b3fd5f6618ec08d14496e2b36ff682fa5605d78899afc9187261ccf8`

~~~~markdown
**Problem:** scanning every element for every selector and cloning path structures can dominate large components, while always building an index can regress small components.
~~~~

### SRC-COMP-L1553-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1553-1553`
- Applicability: `SST1`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1555-005734067394

- Kind: `requirement`
- Source: `compiler-proposal.md:1555-1555`
- Applicability: `SST1`
- Exact text SHA-256: `005734067394a8227f50c90c2759695d026dd58941891364374f8c7810f6d382`

~~~~markdown
- exact matcher semantics remain framework-owned and authoritative;
~~~~

### SRC-COMP-L1556-4B175DF768E2

- Kind: `requirement`
- Source: `compiler-proposal.md:1556-1556`
- Applicability: `SST1`
- Exact text SHA-256: `4b175df768e261cdcdce788da7f537d2bb19c51a719d78061b2647500b7b283e`

~~~~markdown
- compile J selector structure into compact steps/subprogram ranges only when useful;
~~~~

### SRC-COMP-L1557-D57175E90D80

- Kind: `forbidden`
- Source: `compiler-proposal.md:1557-1557`
- Applicability: `SST1`
- Exact text SHA-256: `d57175e90d80547abf5649b7a643947bb64a1d187dca4222e3b49a61f87b3196`

~~~~markdown
- use `SCP2` canonical topology, never runtime IR;
~~~~

### SRC-COMP-L1558-010BA78440AD

- Kind: `context`
- Source: `compiler-proposal.md:1558-1558`
- Applicability: `SST1`
- Exact text SHA-256: `010ba78440ad71ed30dd2db1ff3db0dde09d768b66377cefca65fb797b1cb76e`

~~~~markdown
- define deterministic cost inputs:
~~~~

### SRC-COMP-L1560-7CB324D1AF52

- Kind: `context`
- Source: `compiler-proposal.md:1560-1566`
- Applicability: `SST1`
- Exact text SHA-256: `7cb324d1af523e5645c4c3490462f6f1783a206bdcd0c7af0dde8e3828b2db03`

~~~~markdown
```text
  template node count
  selector count and step count
  positive-anchor availability
  dynamic/wildcard ratio
  posting cardinalities
  ```
~~~~

### SRC-COMP-L1568-10B6D9088503

- Kind: `context`
- Source: `compiler-proposal.md:1568-1568`
- Applicability: `SST1`
- Exact text SHA-256: `10b6d90885033e5c25c5279f9cb6f479ed208cb3f0dc034a1b1c2ef05eae0605`

~~~~markdown
- support `DirectMatcher` and `IndexedMatcher`;
~~~~

### SRC-COMP-L1569-D873CF418C42

- Kind: `context`
- Source: `compiler-proposal.md:1569-1569`
- Applicability: `SST1`
- Exact text SHA-256: `d873cf418c42bf0346959b47eddc295b446c50401d45f4b308a8a7dbc3f27d93`

~~~~markdown
- indexed postings for sound positive tag/id/class/attribute keys;
~~~~

### SRC-COMP-L1570-3360FDFCF7FC

- Kind: `context`
- Source: `compiler-proposal.md:1570-1570`
- Applicability: `SST1`
- Exact text SHA-256: `3360fdfcf7fcdbda270b628b4e1d40543349d71d72557b08ac34a86127b18865`

~~~~markdown
- choose the rarest sound mandatory positive anchor using actual posting cardinality;
~~~~

### SRC-COMP-L1571-1355E7404600

- Kind: `forbidden`
- Source: `compiler-proposal.md:1571-1571`
- Applicability: `SST1`
- Exact text SHA-256: `1355e7404600ddf99c88be76ea1867df9d5a874e7f41f5792ba1cd0696ae660e`

~~~~markdown
- negated predicates and unsafe pseudo branches never seed candidates;
~~~~

### SRC-COMP-L1572-BF207F03E1E2

- Kind: `context`
- Source: `compiler-proposal.md:1572-1572`
- Applicability: `SST1`
- Exact text SHA-256: `bf207f03e1e2a727de48e44eb577144bb80c1c90286e12e8618e0e05d16b2638`

~~~~markdown
- dynamic/spread/maybe buckets are explicitly unioned into candidate sets;
~~~~

### SRC-COMP-L1573-461413062D89

- Kind: `requirement`
- Source: `compiler-proposal.md:1573-1573`
- Applicability: `SST1`
- Exact text SHA-256: `461413062d89edce3a9e6e08576dc5387e2327b38e25b87810eca5c0747bb953`

~~~~markdown
- query planning is demand-only and may be skipped for tiny workloads.
~~~~

### SRC-COMP-L1575-21AF708254D0

- Kind: `context`
- Source: `compiler-proposal.md:1575-1575`
- Applicability: `SST1`
- Exact text SHA-256: `21af708254d0a1f259ada50909b9ec89cc3e243d8921f71164c5c6560cfc5ac8`

~~~~markdown
**Suggested predecessors:** `SCP2`, `SST0`.
~~~~

### SRC-COMP-L1577-68876CC5ED5C

- Kind: `context`
- Source: `compiler-proposal.md:1577-1577`
- Applicability: `SST1`
- Exact text SHA-256: `68876cc5ed5cd8f95d632d15766551b26c53633b53beea66f13bd1e4eada999b`

~~~~markdown
**Suggested subblocks:** selector-step representation, direct matcher baseline, feature postings, candidate rules/dynamic buckets, deterministic cost model, differential/performance tests.
~~~~

### SRC-COMP-L1579-D5BAA15BC1D5

- Kind: `acceptance`
- Source: `compiler-proposal.md:1579-1579`
- Applicability: `SST1`
- Exact text SHA-256: `d5baa15bc1d578c2df3dbb2243202eb86a7ce726c808f94d9a504085d701951a`

~~~~markdown
**Acceptance:** candidate selection has no false negatives; direct and indexed paths feed the same exact verifier; small workloads avoid index construction; all candidate/index work is ledger-visible.
~~~~

### SRC-COMP-L1581-303CBE451AE8

- Kind: `forbidden`
- Source: `compiler-proposal.md:1581-1581`
- Applicability: `SST1`
- Exact text SHA-256: `303cbe451ae87e916e36f0ea002775e52cd8ba77fde744dae57d8cbabb039304`

~~~~markdown
**Forbidden:** probabilistic rejection, negated anchors, always-on indexing, universal selector semantics, or pruning from candidate selection alone.
~~~~

### SRC-COMP-L1583-262C21E0AEDF

- Kind: `deletion`
- Source: `compiler-proposal.md:1583-1583`
- Applicability: `SST1`
- Exact text SHA-256: `262c21e0aedf9f9f5a6dd36ff56f7c8d60525563ea3098bd54f3a00bc190a071`

~~~~markdown
**Deletion/abort:** preserve the exact matcher while replacing only physical execution; abort indexing if equivalent-work benefit is not demonstrated.
~~~~

### SRC-COMP-L1585-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1585-1585`
- Applicability: `SST1`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
