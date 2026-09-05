<!-- unified-charter-v2
id=CMP3
name=Framework-native target planning and static physical execution
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CMP2
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=performance_evidence
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-core/CMP3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CMP3 — Framework-native target planning and static physical execution

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Framework-native target planning and static physical execution. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CMP2:** implemented ledger row for “Data-oriented compiler structure, regions, topology, and lifetime model”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** compile only the relationships required by each requested target without universal lowering or dynamic pass dispatch.
- **Problem:** whole target-tree copies, mandatory reactivity IRs, runtime pass registries, and per-node strategy calls waste work and leak framework semantics.
- **Solution and architecture decisions:**
- each framework owns a private compiler structure and target executors;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CMP3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CMP3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CMP3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CMP3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_vue_conformance/tests`, `crates/verter_svelte_conformance/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **dynamic dispatch inside node loops**.
- Delete or structurally reject: **whole-tree materialization fallback**.
- Delete or structurally reject: **unqualified artifact assembly**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** compile only the relationships required by each requested target without universal lowering or dynamic pass dispatch.

**Problem:** whole target-tree copies, mandatory reactivity IRs, runtime pass registries, and per-node strategy calls waste work and leak framework semantics.

**Solution and architecture decisions:**

- each framework owns a private compiler structure and target executors;
- framework selection and target selection occur once outside hot loops;
- logical operations classify as local synthesized, regional, barrier graph, target planning, emission, or terminal materialization;
- local facts fuse into existing typed visits;
- barrier algorithms operate on compact tables/graphs, not the syntax tree;
- VDOM-like targets use sparse patch/hoist/cache overlays;
- fine-grained client targets request compact dependency/effect/operation graphs;
- server targets request no client effect graph;
- target structure is materialized only when it avoids rediscovery, enables reuse, or is required by a barrier;
- compatible multi-target requests share parse, semantic and structural prerequisites and branch at the minimum target-specific point;
- shared semantic abstractions follow a rule of three; two similarly named framework constructs are insufficient.

**Suggested predecessor:** `CMP2`.

**Normative source decomposition:** execution classes, static target executor pattern, sparse overlay primitives, dependency/effect graph primitives, multi-target branch planner, dynamic-dispatch deletion guards.

**Acceptance:** no accepted hot loop uses per-node dynamic target dispatch; server-only targets produce zero effect-plan ledger entries; target overlays contain only target-specific state; multi-target requests prove shared prerequisites; a synthetic second framework can use the mechanics without importing the first framework’s semantics.

**Forbidden:** universal UI IR, mandatory reactive AST, runtime plugin pass graph, full target tree for symmetry, or speculative build-two-and-discard-one production optimization.

**Deletion/abort:** delete old strategy/walker dispatch only after target parity; move any framework-shaped shared abstraction back to its owner.

---

