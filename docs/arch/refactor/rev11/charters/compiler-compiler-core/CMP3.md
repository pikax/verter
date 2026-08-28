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
conditional_predecessors=
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
source_refs=source:compiler-proposal.md:L1017
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP3 — Framework-native target planning and static physical execution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Framework-native target planning and static physical execution. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CMP2:** exact current receipt ID and digest for “Data-oriented compiler structure, regions, topology, and lifetime model”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1017`

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

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1017-1C672CE8C07B

- Kind: `context`
- Source: `compiler-proposal.md:1017-1017`
- Applicability: `CMP3`
- Exact text SHA-256: `1c672ce8c07b56b47d0dbd5d9e64d5454b997c6ecbebd6fa797cb3443a3abb19`

~~~~markdown
## `CMP3.md` — Framework-native target planning and static physical execution
~~~~

### SRC-COMP-L1019-FA31B5D5CFE5

- Kind: `requirement`
- Source: `compiler-proposal.md:1019-1019`
- Applicability: `CMP3`
- Exact text SHA-256: `fa31b5d5cfe50836f0c179b605d14017733a1f77112bfb0399a5fc207a3ff6de`

~~~~markdown
**Intent:** compile only the relationships required by each requested target without universal lowering or dynamic pass dispatch.
~~~~

### SRC-COMP-L1021-2B7148213510

- Kind: `context`
- Source: `compiler-proposal.md:1021-1021`
- Applicability: `CMP3`
- Exact text SHA-256: `2b7148213510249e08cf83d2e601bc213e8f0249db8fc8c799758ada1995f74e`

~~~~markdown
**Problem:** whole target-tree copies, mandatory reactivity IRs, runtime pass registries, and per-node strategy calls waste work and leak framework semantics.
~~~~

### SRC-COMP-L1023-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1023-1023`
- Applicability: `CMP3`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1025-FFD98B4BB3E5

- Kind: `context`
- Source: `compiler-proposal.md:1025-1025`
- Applicability: `CMP3`
- Exact text SHA-256: `ffd98b4bb3e59ae4a2e4c29c2312a4794a6d076b1b292ab5ae2995dc9c185bf8`

~~~~markdown
- each framework owns a private compiler structure and target executors;
~~~~

### SRC-COMP-L1026-14BF2C6233BC

- Kind: `context`
- Source: `compiler-proposal.md:1026-1026`
- Applicability: `CMP3`
- Exact text SHA-256: `14bf2c6233bc9f0d8c920e6be187480440aae6c8755a7806c9dece5488afb0bc`

~~~~markdown
- framework selection and target selection occur once outside hot loops;
~~~~

### SRC-COMP-L1027-F67FDC55C56F

- Kind: `context`
- Source: `compiler-proposal.md:1027-1027`
- Applicability: `CMP3`
- Exact text SHA-256: `f67fdc55c56f2de49975424e0fe4d253284103a709698ea995c835e609bfa4ec`

~~~~markdown
- logical operations classify as local synthesized, regional, barrier graph, target planning, emission, or terminal materialization;
~~~~

### SRC-COMP-L1028-8CBCB8D4E77D

- Kind: `context`
- Source: `compiler-proposal.md:1028-1028`
- Applicability: `CMP3`
- Exact text SHA-256: `8cbcb8d4e77d0b1c884146ca7f5641560990988aaf83d04247b23328f889269c`

~~~~markdown
- local facts fuse into existing typed visits;
~~~~

### SRC-COMP-L1029-A70E4B406360

- Kind: `context`
- Source: `compiler-proposal.md:1029-1029`
- Applicability: `CMP3`
- Exact text SHA-256: `a70e4b406360f2b1e3d63d2cd1b752e264be0fe7fbdd99fcfea792d2538cf88c`

~~~~markdown
- barrier algorithms operate on compact tables/graphs, not the syntax tree;
~~~~

### SRC-COMP-L1030-FC8964ACAB56

- Kind: `context`
- Source: `compiler-proposal.md:1030-1030`
- Applicability: `CMP3`
- Exact text SHA-256: `fc8964acab56f0c06560d6a82025cb49e13d33b951e5e4e58e030e787d57b4db`

~~~~markdown
- VDOM-like targets use sparse patch/hoist/cache overlays;
~~~~

### SRC-COMP-L1031-A492E96E7136

- Kind: `context`
- Source: `compiler-proposal.md:1031-1031`
- Applicability: `CMP3`
- Exact text SHA-256: `a492e96e713661f7b761b5cb61e93d94527eb0249284106ac288d7ce79583d1a`

~~~~markdown
- fine-grained client targets request compact dependency/effect/operation graphs;
~~~~

### SRC-COMP-L1032-1BDBA08D304A

- Kind: `context`
- Source: `compiler-proposal.md:1032-1032`
- Applicability: `CMP3`
- Exact text SHA-256: `1bdba08d304a602e45db18465fa13a94e535de142e5209c9f36eb6d990bfbf85`

~~~~markdown
- server targets request no client effect graph;
~~~~

### SRC-COMP-L1033-59EE5128007B

- Kind: `requirement`
- Source: `compiler-proposal.md:1033-1033`
- Applicability: `CMP3`
- Exact text SHA-256: `59ee5128007bab11aecf8f67e75fc5e03e2e6cce2b1f60e92eccebc46a6139c8`

~~~~markdown
- target structure is materialized only when it avoids rediscovery, enables reuse, or is required by a barrier;
~~~~

### SRC-COMP-L1034-25CDB419D6D2

- Kind: `context`
- Source: `compiler-proposal.md:1034-1034`
- Applicability: `CMP3`
- Exact text SHA-256: `25cdb419d6d225198ec95472fc6c2dec62ef6f3852b71898fd40dc62405b985a`

~~~~markdown
- compatible multi-target requests share parse, semantic and structural prerequisites and branch at the minimum target-specific point;
~~~~

### SRC-COMP-L1035-3BC51974D98D

- Kind: `context`
- Source: `compiler-proposal.md:1035-1035`
- Applicability: `CMP3`
- Exact text SHA-256: `3bc51974d98db64a3d2bfd4424273f200b61cd5e716d0b85875d42f2adf1e87f`

~~~~markdown
- shared semantic abstractions follow a rule of three; two similarly named framework constructs are insufficient.
~~~~

### SRC-COMP-L1037-DC6858138762

- Kind: `context`
- Source: `compiler-proposal.md:1037-1037`
- Applicability: `CMP3`
- Exact text SHA-256: `dc6858138762fe11084c5311c16b15958eaf836d01668f86a4c197d346b26500`

~~~~markdown
**Suggested predecessor:** `CMP2`.
~~~~

### SRC-COMP-L1039-9161B6D4715E

- Kind: `deletion`
- Source: `compiler-proposal.md:1039-1039`
- Applicability: `CMP3`
- Exact text SHA-256: `9161b6d4715eba2c3cb99bd7c567bb7500d5d7ff40c750f2e1d18232cdf7fbcb`

~~~~markdown
**Suggested subblocks:** execution classes, static target executor pattern, sparse overlay primitives, dependency/effect graph primitives, multi-target branch planner, dynamic-dispatch deletion guards.
~~~~

### SRC-COMP-L1041-8F61BEF001B8

- Kind: `acceptance`
- Source: `compiler-proposal.md:1041-1041`
- Applicability: `CMP3`
- Exact text SHA-256: `8f61bef001b8c4826d2dc87217f6757f29680ce6b9b6a2308b36154d14e23b3b`

~~~~markdown
**Acceptance:** no accepted hot loop uses per-node dynamic target dispatch; server-only targets produce zero effect-plan ledger entries; target overlays contain only target-specific state; multi-target requests prove shared prerequisites; a synthetic second framework can use the mechanics without importing the first framework’s semantics.
~~~~

### SRC-COMP-L1043-39FCC97FB525

- Kind: `forbidden`
- Source: `compiler-proposal.md:1043-1043`
- Applicability: `CMP3`
- Exact text SHA-256: `39fcc97fb52598cfe3f7f432b4ad3d4b896d1f2b7796b00d2963ccee9eaf51ed`

~~~~markdown
**Forbidden:** universal UI IR, mandatory reactive AST, runtime plugin pass graph, full target tree for symmetry, or speculative build-two-and-discard-one production optimization.
~~~~

### SRC-COMP-L1045-79FCB3B4C057

- Kind: `deletion`
- Source: `compiler-proposal.md:1045-1045`
- Applicability: `CMP3`
- Exact text SHA-256: `79fcb3b4c057f6b249bb26dbd3bf3757e76c4115e48ca957e752285818d09707`

~~~~markdown
**Deletion/abort:** delete old strategy/walker dispatch only after target parity; move any framework-shaped shared abstraction back to its owner.
~~~~

### SRC-COMP-L1047-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1047-1047`
- Applicability: `CMP3`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
