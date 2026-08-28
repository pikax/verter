<!-- unified-charter-v2
id=VCP7
name=Vue Default compiler product terminal
phase=compiler
train=compiler.vue-compiler
product=vue_compiler
kind=terminal
semantic_role=convergence
class=compiler
predecessors=VCP6,CPER2,BR0
conditional_predecessors=
owner=compiler.vue-compiler:Vue-owned Default compiler cells over shared compiler substrate
conflict_domains=compiler_execution,vue_product
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
source_refs=source:compiler-proposal.md:L1395
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-compiler/VCP7.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VCP7 — Vue Default compiler product terminal

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue Default compiler product terminal. The current owner is **Vue runtime emitter and assembly paths**. The final and sole owner is **Vue-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`.
- Named API/data boundaries: `VueSemanticSnapshot`, `VueCompilePlan`, `VueTarget`, `VueArtifactSet`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **VCP6:** exact current receipt ID and digest for “Vue module assembly, artifacts, host integration, and atomic cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPER2:** exact current receipt ID and digest for “Shared compiler physical-execution and zero-work terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **BR0:** exact current receipt ID and digest for “Post-L4 successor product promotion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** decide whether Vue V2 is a correct, production-quality, independently promotable default compiler.
- **Problem:** a successful cutover still needs cumulative correctness, performance, memory, failure, and deletion proof on one exact tree.
- **Solution and architecture decisions:** read-only terminal over all Vue targets and style integration.
- **Suggested predecessors:** VCP6, CPER2.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VCP7-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VCP7-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VCP7-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VCP7-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_vue_conformance/tests`, `packages/vue-conformance-oracle`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Vue emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
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

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1395`

## Reconciled source-plan contract

**Intent:** decide whether Vue V2 is a correct, production-quality, independently promotable default compiler.

**Problem:** a successful cutover still needs cumulative correctness, performance, memory, failure, and deletion proof on one exact tree.

**Solution and architecture decisions:** read-only terminal over all Vue targets and style integration.

**Suggested predecessors:** `VCP6`, `CPER2`.

**Required evidence:**

- exact `VCP0` contract matrix;
- runtime/hydration/diagnostic/map/CSS/module-artifact validation;
- strict malformed-source refusal with tooling recovery unaffected;
- direct/prepared/managed and incremental/fresh equivalence;
- single and multi-target work-ledger compliance;
- cold/warm/batch/RSS/cancellation gates;
- zero old Vue compiler/session assembly consumers;
- truthful `Default = Supported`, `Optimized = FutureSeparateTrain` capability rows.

**Acceptance:** all locked cells pass on one candidate and old Vue compiler authorities are deleted.

**Forbidden:** implementation fixes in the terminal, waiving a correctness cell for speed, or enabling `Optimized`.

**Deletion/abort:** findings return to the exact Vue owner; terminal deletes nothing beyond verifying `VCP6`’s deletion.

---

# 9. Svelte Default compiler train

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1395-7A86F740AC5F

- Kind: `context`
- Source: `compiler-proposal.md:1395-1395`
- Applicability: `VCP7`
- Exact text SHA-256: `7a86f740ac5fa8f30fe1f0191a7d3589579171768e08feda04a2c21d33259452`

~~~~markdown
## `VCP7.md` — Vue Default compiler product terminal
~~~~

### SRC-COMP-L1397-6867FF7C1D6F

- Kind: `context`
- Source: `compiler-proposal.md:1397-1397`
- Applicability: `VCP7`
- Exact text SHA-256: `6867ff7c1d6f2646840f21b00de725f98d92efcf1a589d65cb1d0b809bc3b866`

~~~~markdown
**Intent:** decide whether Vue V2 is a correct, production-quality, independently promotable default compiler.
~~~~

### SRC-COMP-L1399-09E8777E5B84

- Kind: `deletion`
- Source: `compiler-proposal.md:1399-1399`
- Applicability: `VCP7`
- Exact text SHA-256: `09e8777e5b84aa5fca7c68b60221b9d4f94a8182bf9a815f1ca607837b68c781`

~~~~markdown
**Problem:** a successful cutover still needs cumulative correctness, performance, memory, failure, and deletion proof on one exact tree.
~~~~

### SRC-COMP-L1401-4D79DF1A1A69

- Kind: `requirement`
- Source: `compiler-proposal.md:1401-1401`
- Applicability: `VCP7`
- Exact text SHA-256: `4d79df1a1a69c8359bc14f0eb3bbb38ec4d9f3d042767ace5948c5f50f2e5015`

~~~~markdown
**Solution and architecture decisions:** read-only terminal over all Vue targets and style integration.
~~~~

### SRC-COMP-L1403-F4EA14C8202D

- Kind: `context`
- Source: `compiler-proposal.md:1403-1403`
- Applicability: `VCP7`
- Exact text SHA-256: `f4ea14c8202d3a7a527f5a7fe4ceacc3c0c97de7985b50b791652b91b7ed803c`

~~~~markdown
**Suggested predecessors:** `VCP6`, `CPER2`.
~~~~

### SRC-COMP-L1405-A2C8E1662524

- Kind: `acceptance`
- Source: `compiler-proposal.md:1405-1405`
- Applicability: `VCP7`
- Exact text SHA-256: `a2c8e1662524a5e9dd67dc462dc193f99c488cfaafea06b74c2b600c004186b0`

~~~~markdown
**Required evidence:**
~~~~

### SRC-COMP-L1407-CB163F00DAD7

- Kind: `requirement`
- Source: `compiler-proposal.md:1407-1407`
- Applicability: `VCP7`
- Exact text SHA-256: `cb163f00dad7d3eb53d80d9897f96868303225a60061ced78665b488e279f469`

~~~~markdown
- exact `VCP0` contract matrix;
~~~~

### SRC-COMP-L1408-9A1C9C73999F

- Kind: `context`
- Source: `compiler-proposal.md:1408-1408`
- Applicability: `VCP7`
- Exact text SHA-256: `9a1c9c73999fe6e1813fa7bd1d83daaffaf23fdd61d0aac4c8e513b316b07b66`

~~~~markdown
- runtime/hydration/diagnostic/map/CSS/module-artifact validation;
~~~~

### SRC-COMP-L1409-9B8EA1BC20A0

- Kind: `context`
- Source: `compiler-proposal.md:1409-1409`
- Applicability: `VCP7`
- Exact text SHA-256: `9b8ea1bc20a0b7c000c7ba8a38c64b4b496901946337a778ed79b62096d392af`

~~~~markdown
- strict malformed-source refusal with tooling recovery unaffected;
~~~~

### SRC-COMP-L1410-998DC23D34E4

- Kind: `context`
- Source: `compiler-proposal.md:1410-1410`
- Applicability: `VCP7`
- Exact text SHA-256: `998dc23d34e478019fa15271330686e62910e37922f1524454910d3d9bf21bd6`

~~~~markdown
- direct/prepared/managed and incremental/fresh equivalence;
~~~~

### SRC-COMP-L1411-55FEFB118598

- Kind: `context`
- Source: `compiler-proposal.md:1411-1411`
- Applicability: `VCP7`
- Exact text SHA-256: `55fefb118598b318d4ac76ea4d874ba26adba10ec5bd7079ebe0233e71e5f7c3`

~~~~markdown
- single and multi-target work-ledger compliance;
~~~~

### SRC-COMP-L1412-DA23988DECA3

- Kind: `context`
- Source: `compiler-proposal.md:1412-1412`
- Applicability: `VCP7`
- Exact text SHA-256: `da23988deca35617f2df9a92a8a12874b30bee32ca0b7cb2da39bfc6821caba5`

~~~~markdown
- cold/warm/batch/RSS/cancellation gates;
~~~~

### SRC-COMP-L1413-E9ACC278538A

- Kind: `context`
- Source: `compiler-proposal.md:1413-1413`
- Applicability: `VCP7`
- Exact text SHA-256: `e9acc278538a06a79855f44a2a16f846dfa1de4d6ba451a3a71a77436c3cd978`

~~~~markdown
- zero old Vue compiler/session assembly consumers;
~~~~

### SRC-COMP-L1414-923871471FD6

- Kind: `context`
- Source: `compiler-proposal.md:1414-1414`
- Applicability: `VCP7`
- Exact text SHA-256: `923871471fd67dc76c663bb7aa55c6a3a5c8ff6d1cf6410ff18484a46fcfd775`

~~~~markdown
- truthful `Default = Supported`, `Optimized = FutureSeparateTrain` capability rows.
~~~~

### SRC-COMP-L1416-8C98CBAD28FA

- Kind: `deletion`
- Source: `compiler-proposal.md:1416-1416`
- Applicability: `VCP7`
- Exact text SHA-256: `8c98cbad28fa50f2cda6777b6c42873eb6ceff4b1430941fc157b517ae4f9878`

~~~~markdown
**Acceptance:** all locked cells pass on one candidate and old Vue compiler authorities are deleted.
~~~~

### SRC-COMP-L1418-4C43D57E7EF4

- Kind: `forbidden`
- Source: `compiler-proposal.md:1418-1418`
- Applicability: `VCP7`
- Exact text SHA-256: `4c43d57e7ef42884da77f2947e2f2c6c2c6fb0ec8e7a48a933db4280d26e063e`

~~~~markdown
**Forbidden:** implementation fixes in the terminal, waiving a correctness cell for speed, or enabling `Optimized`.
~~~~

### SRC-COMP-L1420-66C5067768BF

- Kind: `deletion`
- Source: `compiler-proposal.md:1420-1420`
- Applicability: `VCP7`
- Exact text SHA-256: `66c5067768bf9d2015109b143f37df90e9714324e8a0882e7b6f988006b85355`

~~~~markdown
**Deletion/abort:** findings return to the exact Vue owner; terminal deletes nothing beyond verifying `VCP6`’s deletion.
~~~~

### SRC-COMP-L1422-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1422-1422`
- Applicability: `VCP7`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
