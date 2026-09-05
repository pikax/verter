<!-- unified-charter-v2
id=CMP2
name=Data-oriented compiler structure, regions, topology, and lifetime model
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CMP1
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=compiler_execution
resource_class=rust-mixed
review_profile=concurrency-3
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
charter=charters/compiler-compiler-core/CMP2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CMP2 — Data-oriented compiler structure, regions, topology, and lifetime model

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Data-oriented compiler structure, regions, topology, and lifetime model. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CMP1:** implemented ledger row for “Demand-refined semantic consumption and admissions”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** establish compact framework-neutral mechanics while preserving framework-native compiler meaning.
- **Problem:** object graphs with per-node String, Vec, HashMap, copied text, and source-offset identities increase allocation/RSS and make repeated structural discovery likely.
- **Solution and architecture decisions:**
- dense snapshot-local typed IDs are direct arena indices;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CMP2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CMP2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CMP2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CMP2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
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

Apply `concurrency-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `concurrency-lifetime`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** establish compact framework-neutral mechanics while preserving framework-native compiler meaning.

**Problem:** object graphs with per-node `String`, `Vec`, `HashMap`, copied text, and source-offset identities increase allocation/RSS and make repeated structural discovery likely.

**Solution and architecture decisions:**

- dense snapshot-local typed IDs are direct arena indices;
- authored start/end offsets live in side tables and never define compiler identity;
- region-owned control flow normalizes branch/body ownership once;
- compact topology tables provide parent/child/sibling/preorder/region relations where a framework demands them;
- hot classifications use packed/dense tables; rare facts use sparse tables;
- child, attribute, operation, dependency and relation collections use flat arenas plus ranges;
- raw authored slices remain source-backed; only requested decoded/interned/normalized values allocate;
- lifetime classes are explicit (`Frontend`, `Semantic`, `CompilerScratch`, `TargetScratch`, `Emission`) and may be combined only through measurement;
- canonical compiler structures are logical contracts; direct one-shot execution may later stream/fuse portions after materialized parity is proven.

**Suggested predecessor:** `CMP1`.

**Normative source decomposition:** typed ID/arena primitives, span/offset indexes, region storage, topology storage, interning/range migration, lifetime/size verification.

**Acceptance:** `nodes[id.index()]` is O(1) with compact dense storage; source-position lookup remains exact through a separate index; no source-length-sized sparse node arena is required; node-size and bytes/node gates pass; no hot node owns variable-size collections directly unless a measured exception is ratified.

**Forbidden:** `NodeId = authored byte offset`, cross-revision offset identity, one arena by ideology, universal semantic node kinds, or copied source strings for ownership.

**Deletion/abort:** migrate one framework structure at a time; abort any “shared” node/region primitive that requires framework branches.

---

