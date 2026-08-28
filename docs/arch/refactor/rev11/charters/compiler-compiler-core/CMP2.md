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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L987
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP2 — Data-oriented compiler structure, regions, topology, and lifetime model

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Data-oriented compiler structure, regions, topology, and lifetime model. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CMP1:** exact current receipt ID and digest for “Demand-refined semantic consumption and admissions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `concurrency-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `concurrency-lifetime`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `concurrency-lifetime`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L987`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L987-4C86D312BC41

- Kind: `context`
- Source: `compiler-proposal.md:987-987`
- Applicability: `CMP2`
- Exact text SHA-256: `4c86d312bc419898cc365c3d8a1733430f87677d3647cba0801b1dc16dca16e1`

~~~~markdown
## `CMP2.md` — Data-oriented compiler structure, regions, topology, and lifetime model
~~~~

### SRC-COMP-L989-898B17B84A6F

- Kind: `context`
- Source: `compiler-proposal.md:989-989`
- Applicability: `CMP2`
- Exact text SHA-256: `898b17b84a6fa6b0da98534837a3233c1c28173c6d27fd9b4840af7fc9ab8c8a`

~~~~markdown
**Intent:** establish compact framework-neutral mechanics while preserving framework-native compiler meaning.
~~~~

### SRC-COMP-L991-872394D7CBE7

- Kind: `context`
- Source: `compiler-proposal.md:991-991`
- Applicability: `CMP2`
- Exact text SHA-256: `872394d7cbe7da42f544798df23496b095a7f512c02be3574669a9aeab63db3b`

~~~~markdown
**Problem:** object graphs with per-node `String`, `Vec`, `HashMap`, copied text, and source-offset identities increase allocation/RSS and make repeated structural discovery likely.
~~~~

### SRC-COMP-L993-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:993-993`
- Applicability: `CMP2`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L995-67A725048F67

- Kind: `context`
- Source: `compiler-proposal.md:995-995`
- Applicability: `CMP2`
- Exact text SHA-256: `67a725048f67a906a55fbe518ef39c12b3708a8e7e07ea7224d7455dec37b450`

~~~~markdown
- dense snapshot-local typed IDs are direct arena indices;
~~~~

### SRC-COMP-L996-36AF05C90D42

- Kind: `forbidden`
- Source: `compiler-proposal.md:996-996`
- Applicability: `CMP2`
- Exact text SHA-256: `36af05c90d42602ae63f30db428af58796342a4682aab76a1400189ad64a0d90`

~~~~markdown
- authored start/end offsets live in side tables and never define compiler identity;
~~~~

### SRC-COMP-L997-BA76AB843CC1

- Kind: `context`
- Source: `compiler-proposal.md:997-997`
- Applicability: `CMP2`
- Exact text SHA-256: `ba76ab843cc18124832023e030a7c60dff238f0ad1884bcdcd0fa45d446ce81b`

~~~~markdown
- region-owned control flow normalizes branch/body ownership once;
~~~~

### SRC-COMP-L998-9A54DC9328FD

- Kind: `context`
- Source: `compiler-proposal.md:998-998`
- Applicability: `CMP2`
- Exact text SHA-256: `9a54dc9328fdaff2dfb24f6dd773bfa57ac3d66d8da30ef1b6b51d4d81483c62`

~~~~markdown
- compact topology tables provide parent/child/sibling/preorder/region relations where a framework demands them;
~~~~

### SRC-COMP-L999-263EF8A24B59

- Kind: `context`
- Source: `compiler-proposal.md:999-999`
- Applicability: `CMP2`
- Exact text SHA-256: `263ef8a24b59d26346e549a545d2f162736db9161d57398ea217ee7547af6e99`

~~~~markdown
- hot classifications use packed/dense tables; rare facts use sparse tables;
~~~~

### SRC-COMP-L1000-9D129C962A88

- Kind: `context`
- Source: `compiler-proposal.md:1000-1000`
- Applicability: `CMP2`
- Exact text SHA-256: `9d129c962a88098467c4a59c66fcd44a9368bbadb3d77cf8d5a833914a5d7f52`

~~~~markdown
- child, attribute, operation, dependency and relation collections use flat arenas plus ranges;
~~~~

### SRC-COMP-L1001-1B6067B6ADD4

- Kind: `requirement`
- Source: `compiler-proposal.md:1001-1001`
- Applicability: `CMP2`
- Exact text SHA-256: `1b6067b6add49fbdebaf8e8b13d182bcacaf9b5d00abeea312cf2e3bd982c69d`

~~~~markdown
- raw authored slices remain source-backed; only requested decoded/interned/normalized values allocate;
~~~~

### SRC-COMP-L1002-B74331FB40EC

- Kind: `requirement`
- Source: `compiler-proposal.md:1002-1002`
- Applicability: `CMP2`
- Exact text SHA-256: `b74331fb40ec91da435a7c6a322386758d3a6dd7efe319cf634ba8fa5cf10d6b`

~~~~markdown
- lifetime classes are explicit (`Frontend`, `Semantic`, `CompilerScratch`, `TargetScratch`, `Emission`) and may be combined only through measurement;
~~~~

### SRC-COMP-L1003-AA1B12CC6F0E

- Kind: `context`
- Source: `compiler-proposal.md:1003-1003`
- Applicability: `CMP2`
- Exact text SHA-256: `aa1b12cc6f0ee729069656948cccdd1d5688e3e78b6103fb003cdd61a79d1231`

~~~~markdown
- canonical compiler structures are logical contracts; direct one-shot execution may later stream/fuse portions after materialized parity is proven.
~~~~

### SRC-COMP-L1005-B328913439D5

- Kind: `context`
- Source: `compiler-proposal.md:1005-1005`
- Applicability: `CMP2`
- Exact text SHA-256: `b328913439d5f95bd18dca6cf16fabc8838d93b87db4bf93b40e23a6e5ab2f0d`

~~~~markdown
**Suggested predecessor:** `CMP1`.
~~~~

### SRC-COMP-L1007-4FD2E85B260D

- Kind: `context`
- Source: `compiler-proposal.md:1007-1007`
- Applicability: `CMP2`
- Exact text SHA-256: `4fd2e85b260dfddccd394978f7159771397af9338475fd3a6d47a2f37e61558a`

~~~~markdown
**Suggested subblocks:** typed ID/arena primitives, span/offset indexes, region storage, topology storage, interning/range migration, lifetime/size verification.
~~~~

### SRC-COMP-L1009-D1C878B8D2B8

- Kind: `acceptance`
- Source: `compiler-proposal.md:1009-1009`
- Applicability: `CMP2`
- Exact text SHA-256: `d1c878b8d2b86639adcd8bc48bf1ba8002468e871dbc36cce4fce5153f5eeb95`

~~~~markdown
**Acceptance:** `nodes[id.index()]` is O(1) with compact dense storage; source-position lookup remains exact through a separate index; no source-length-sized sparse node arena is required; node-size and bytes/node gates pass; no hot node owns variable-size collections directly unless a measured exception is ratified.
~~~~

### SRC-COMP-L1011-26777BD1AC40

- Kind: `forbidden`
- Source: `compiler-proposal.md:1011-1011`
- Applicability: `CMP2`
- Exact text SHA-256: `26777bd1ac40eef9d87493eb7e0097e83092eea47edbb0d748f2259fc3c4a5a5`

~~~~markdown
**Forbidden:** `NodeId = authored byte offset`, cross-revision offset identity, one arena by ideology, universal semantic node kinds, or copied source strings for ownership.
~~~~

### SRC-COMP-L1013-5537C0840A44

- Kind: `deletion`
- Source: `compiler-proposal.md:1013-1013`
- Applicability: `CMP2`
- Exact text SHA-256: `5537c0840a4447f0cfa0e48ea4b1f6cec67f014f0d2959c39d0183e51589d138`

~~~~markdown
**Deletion/abort:** migrate one framework structure at a time; abort any “shared” node/region primitive that requires framework branches.
~~~~

### SRC-COMP-L1015-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1015-1015`
- Applicability: `CMP2`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
