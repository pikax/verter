<!-- unified-charter-v2
id=VCP2
name=Compact Vue compiler structure and canonical template topology
phase=compiler
train=compiler.vue-compiler
product=vue_compiler
kind=implementation
semantic_role=delivery
class=compiler
predecessors=VCP1
conditional_predecessors=
owner=compiler.vue-compiler:Vue-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,vue_product
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
source_refs=source:compiler-proposal.md:L1201
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP2 — Compact Vue compiler structure and canonical template topology

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compact Vue compiler structure and canonical template topology. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP1:** exact current receipt ID and digest for “Canonical Vue semantic authority convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** replace repeated AST relationship discovery with a compact Vue-owned structural lowering suitable for all targets.
- **Problem:** directives/siblings/slots/control flow can be rediscovered by multiple targets, and object-heavy nodes impede cache locality.
- **Solution and architecture decisions:**
- dense VueCompileNodeId, VueTemplateNodeId, VueRegionId, and ranges;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VCP2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VCP2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VCP2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VCP2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_vue_conformance/tests`, `packages/vue-conformance-oracle`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Vue emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
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

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1201`

## Reconciled source-plan contract

**Intent:** replace repeated AST relationship discovery with a compact Vue-owned structural lowering suitable for all targets.

**Problem:** directives/siblings/slots/control flow can be rediscovered by multiple targets, and object-heavy nodes impede cache locality.

**Solution and architecture decisions:**

- dense `VueCompileNodeId`, `VueTemplateNodeId`, `VueRegionId`, and ranges;
- region-owned `if`, `for`, slot and component-child structures;
- canonical parent/child/sibling/preorder topology where demanded;
- source spans/anchors, semantic references and target decisions in side tables;
- flat attribute/child/directive arenas and interned names;
- logical materialization contract with future streaming permission;
- no target-specific patch/effect/server state in structural nodes.

**Suggested predecessor:** `VCP1`.

**Normative source decomposition:** ID/arena migration, control-flow regions, slot/component regions, topology, side-table/data-layout conversion, dumps/verifiers.

**Acceptance:** all targets can consume one structural authority; node access is O(1) by dense ID; source offsets remain separate; node-size/allocation budgets pass; malformed source never enters admitted lowering.

**Forbidden:** source-offset node IDs, target flags in structural nodes, per-node `Vec`/`String` defaults, or universal UI operations.

**Deletion/abort:** migrate behavior-preservingly and delete old shared walkers only when their final target moves.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1201-7E571EC6AC68

- Kind: `context`
- Source: `compiler-proposal.md:1201-1201`
- Applicability: `VCP2`
- Exact text SHA-256: `7e571ec6ac6838b86c54b826cd8e00812190ffc7a34c53d5307c1565bf5314a4`

~~~~markdown
## `VCP2.md` — Compact Vue compiler structure and canonical template topology
~~~~

### SRC-COMP-L1203-758BF0088BF8

- Kind: `context`
- Source: `compiler-proposal.md:1203-1203`
- Applicability: `VCP2`
- Exact text SHA-256: `758bf0088bf8f449405d30031e175833b7a5b2d9f69cd00787728de8bdb9b5b1`

~~~~markdown
**Intent:** replace repeated AST relationship discovery with a compact Vue-owned structural lowering suitable for all targets.
~~~~

### SRC-COMP-L1205-5F5BEC9C8F9B

- Kind: `context`
- Source: `compiler-proposal.md:1205-1205`
- Applicability: `VCP2`
- Exact text SHA-256: `5f5bec9c8f9b043d4ceb877517fd3ac7d115b3bdbdccdbcb93efff654c92fc3d`

~~~~markdown
**Problem:** directives/siblings/slots/control flow can be rediscovered by multiple targets, and object-heavy nodes impede cache locality.
~~~~

### SRC-COMP-L1207-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1207-1207`
- Applicability: `VCP2`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1209-2FFEF144EEB9

- Kind: `context`
- Source: `compiler-proposal.md:1209-1209`
- Applicability: `VCP2`
- Exact text SHA-256: `2ffef144eeb967744aa2b2312cf3ce62494a5d216eb176cb5b189a64e2d8550d`

~~~~markdown
- dense `VueCompileNodeId`, `VueTemplateNodeId`, `VueRegionId`, and ranges;
~~~~

### SRC-COMP-L1210-9562C91F8A0E

- Kind: `context`
- Source: `compiler-proposal.md:1210-1210`
- Applicability: `VCP2`
- Exact text SHA-256: `9562c91f8a0e686c64ddc1669ab513d35087e3e15a085b638e3d6225d23d4a2b`

~~~~markdown
- region-owned `if`, `for`, slot and component-child structures;
~~~~

### SRC-COMP-L1211-8432E829F817

- Kind: `context`
- Source: `compiler-proposal.md:1211-1211`
- Applicability: `VCP2`
- Exact text SHA-256: `8432e829f81714be2a25fe2363fd02bdd061a89cd8b15628cfa4dc35c075e9fb`

~~~~markdown
- canonical parent/child/sibling/preorder topology where demanded;
~~~~

### SRC-COMP-L1212-BD85D03E84BE

- Kind: `context`
- Source: `compiler-proposal.md:1212-1212`
- Applicability: `VCP2`
- Exact text SHA-256: `bd85d03e84be55614ae976a7bec4fbdb1ecda0daf25a324e11f96d4f8aa96c56`

~~~~markdown
- source spans/anchors, semantic references and target decisions in side tables;
~~~~

### SRC-COMP-L1213-1A778A9F14AD

- Kind: `context`
- Source: `compiler-proposal.md:1213-1213`
- Applicability: `VCP2`
- Exact text SHA-256: `1a778a9f14ad192ca9dc64ad621318c4e79fd8150a2fc642f6b2dd2783dd31fc`

~~~~markdown
- flat attribute/child/directive arenas and interned names;
~~~~

### SRC-COMP-L1214-D2E239D7527A

- Kind: `context`
- Source: `compiler-proposal.md:1214-1214`
- Applicability: `VCP2`
- Exact text SHA-256: `d2e239d7527ac46b7b30d4e8831e6fd9dbd954161045f19a33eb0235f4a5fd12`

~~~~markdown
- logical materialization contract with future streaming permission;
~~~~

### SRC-COMP-L1215-2B308EFCEE2C

- Kind: `context`
- Source: `compiler-proposal.md:1215-1215`
- Applicability: `VCP2`
- Exact text SHA-256: `2b308efcee2c37cf86a99b9307282abbc7db09515e3a25adf653645f7d72d908`

~~~~markdown
- no target-specific patch/effect/server state in structural nodes.
~~~~

### SRC-COMP-L1217-21BEDA004DA0

- Kind: `context`
- Source: `compiler-proposal.md:1217-1217`
- Applicability: `VCP2`
- Exact text SHA-256: `21beda004da0eceadc30037990e9e697609cce20fa9ab17c7ac98c89da679849`

~~~~markdown
**Suggested predecessor:** `VCP1`.
~~~~

### SRC-COMP-L1219-5364A0A95826

- Kind: `context`
- Source: `compiler-proposal.md:1219-1219`
- Applicability: `VCP2`
- Exact text SHA-256: `5364a0a95826c3986513fdf103f9b7348e6c749d339f8b1d36ac7240a85e8506`

~~~~markdown
**Suggested subblocks:** ID/arena migration, control-flow regions, slot/component regions, topology, side-table/data-layout conversion, dumps/verifiers.
~~~~

### SRC-COMP-L1221-5A4EE4C0AF33

- Kind: `forbidden`
- Source: `compiler-proposal.md:1221-1221`
- Applicability: `VCP2`
- Exact text SHA-256: `5a4ee4c0af3315bc780296ddac34028cf667bd5e013580729bed11b6cd71f3d2`

~~~~markdown
**Acceptance:** all targets can consume one structural authority; node access is O(1) by dense ID; source offsets remain separate; node-size/allocation budgets pass; malformed source never enters admitted lowering.
~~~~

### SRC-COMP-L1223-C56EA55A1322

- Kind: `forbidden`
- Source: `compiler-proposal.md:1223-1223`
- Applicability: `VCP2`
- Exact text SHA-256: `c56ea55a1322bb7a8417820db3338eae0f0f6060f27fdd0b00e9c8f388c6c620`

~~~~markdown
**Forbidden:** source-offset node IDs, target flags in structural nodes, per-node `Vec`/`String` defaults, or universal UI operations.
~~~~

### SRC-COMP-L1225-DE64B8D120CC

- Kind: `deletion`
- Source: `compiler-proposal.md:1225-1225`
- Applicability: `VCP2`
- Exact text SHA-256: `de64b8d120cc20c62e4314051e9bb3a825d4d21900f642d29b1c03102705924f`

~~~~markdown
**Deletion/abort:** migrate behavior-preservingly and delete old shared walkers only when their final target moves.
~~~~

### SRC-COMP-L1227-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1227-1227`
- Applicability: `VCP2`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
