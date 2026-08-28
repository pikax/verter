<!-- unified-charter-v2
id=OPT0
name=Compiler optimization engine rescope and maintainer ratification
phase=compiler
train=compiler.compiler-optimization
product=compiler_optimization
kind=rescope
semantic_role=delivery
class=compiler
predecessors=CMP6,CPER3
conditional_predecessors=
owner=compiler.compiler-optimization:maintainer-ratified separately measured optimization engine scope
conflict_domains=compiler_execution
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
dispatchable=false
optional=false
release_gating=non_release
source_refs=source:compiler-proposal.md:L1809
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-optimization/OPT0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=RESCOPE_REQUIRED
-->

# OPT0 — Compiler optimization engine rescope and maintainer ratification

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compiler optimization engine rescope and maintainer ratification. The current owner is **deferred optimization proposal**. The final and sole owner is **maintainer-ratified separately measured optimization engine scope**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`.
- Named API/data boundaries: `OptimizationPlan`, `OptimizationProof`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CMP6:** exact current receipt ID and digest for “Cross-framework compiler-engine falsification”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPER3:** exact current receipt ID and digest for “Cross-framework compiler soak and equivalent-work study”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** reserve the future optimization-engine decision point while explicitly preventing premature implementation.
- **Problem:** project-wide provenance, declaration/implementation inspection, proof/evidence storage, cost models and fallback policy may improve generated output, but designing a generalized engine now would be speculative and could delay correct default compi
- **Suggested predecessors:** CMP6, CPER3.
- **Required input for future rescope:** a maintainer-provided or maintainer-approved dedicated plan that addresses at least:

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **OPT0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **OPT0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **OPT0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **OPT0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_bench`.

## Deletions and forbidden designs

- Delete or structurally reject: **optimization hidden in Default**.
- Delete or structurally reject: **benchmark-specific code paths**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1809`

## Reconciled source-plan contract

**Status:** `RESCOPE_REQUIRED`; no implementation authority; no `OPT1+` block may be created from this proposal.

**Intent:** reserve the future optimization-engine decision point while explicitly preventing premature implementation.

**Problem:** project-wide provenance, declaration/implementation inspection, proof/evidence storage, cost models and fallback policy may improve generated output, but designing a generalized engine now would be speculative and could delay correct default compilers.

**Suggested predecessors:** `CMP6`, `CPER3`.

**Required input for future rescope:** a maintainer-provided or maintainer-approved dedicated plan that addresses at least:

- precise optimization goals and measurable benefit;
- Verter-native analysis only (`verter_analysis`, `type_info`, resolver);
- internal analysis-depth strategy behind public `Optimized`;
- `OptimizationRequestBasis` versus `OptimizationObservationSet`;
- exact read-set validation, invalidation, cancellation and budgets;
- evidence/provenance representation and whether a generalized proof system is justified;
- deterministic fallback to `Default`;
- artifact identity and reproducibility;
- security, filesystem/package boundaries and RSS;
- per-framework target admission;
- independent benchmarks proving compile-cost versus runtime/code-size benefit.

**Acceptance:** only a newly ratified plan and DAG amendment can close `OPT0` and create successors.

**Forbidden:** code, “temporary” project traversal, enabling `Optimized`, generic certificate/proof engines, or using ambient LSP facts.

**Deletion/abort:** none; remain `RESCOPE_REQUIRED` until maintainer action.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1809-F93D1FD64CF5

- Kind: `context`
- Source: `compiler-proposal.md:1809-1809`
- Applicability: `OPT0`
- Exact text SHA-256: `f93d1fd64cf50e3f27ed8c9299c7e61cf3023428b3fc0f61cb5ed5bef6fa6b51`

~~~~markdown
## `OPT0.md` — Compiler optimization engine rescope and maintainer ratification
~~~~

### SRC-COMP-L1811-39309A33AACD

- Kind: `requirement`
- Source: `compiler-proposal.md:1811-1811`
- Applicability: `OPT0`
- Exact text SHA-256: `39309a33aacdfed9773c9f8f55c6ce07a3f9c4d9c0cf2dcb13987c342d072cf0`

~~~~markdown
**Status:** `RESCOPE_REQUIRED`; no implementation authority; no `OPT1+` block may be created from this proposal.
~~~~

### SRC-COMP-L1813-1D4133D972CC

- Kind: `context`
- Source: `compiler-proposal.md:1813-1813`
- Applicability: `OPT0`
- Exact text SHA-256: `1d4133d972cc6f81007a31166aed514f61438294440755e66b9dfaab508a63c2`

~~~~markdown
**Intent:** reserve the future optimization-engine decision point while explicitly preventing premature implementation.
~~~~

### SRC-COMP-L1815-5D0D58141228

- Kind: `context`
- Source: `compiler-proposal.md:1815-1815`
- Applicability: `OPT0`
- Exact text SHA-256: `5d0d5814122845419a69ac4809a90a3f74d33c8d45d05ade8b64e57a84aabf24`

~~~~markdown
**Problem:** project-wide provenance, declaration/implementation inspection, proof/evidence storage, cost models and fallback policy may improve generated output, but designing a generalized engine now would be speculative and could delay correct default compilers.
~~~~

### SRC-COMP-L1817-C79BAF7E8EAE

- Kind: `context`
- Source: `compiler-proposal.md:1817-1817`
- Applicability: `OPT0`
- Exact text SHA-256: `c79baf7e8eaedf0f8f1086c676e8203ce4edaeeb9725384b305e3fd0bac2e1b2`

~~~~markdown
**Suggested predecessors:** `CMP6`, `CPER3`.
~~~~

### SRC-COMP-L1819-2643F16A3C0E

- Kind: `requirement`
- Source: `compiler-proposal.md:1819-1819`
- Applicability: `OPT0`
- Exact text SHA-256: `2643f16a3c0ee51445a5111e75e8382cd2a69fad88f113693eea86ead4bfbc97`

~~~~markdown
**Required input for future rescope:** a maintainer-provided or maintainer-approved dedicated plan that addresses at least:
~~~~

### SRC-COMP-L1821-B9E110B2BD43

- Kind: `context`
- Source: `compiler-proposal.md:1821-1821`
- Applicability: `OPT0`
- Exact text SHA-256: `b9e110b2bd4396a13875ea8ef2370bb9e38d62dcd64dc8ced6deb75c101ae5b9`

~~~~markdown
- precise optimization goals and measurable benefit;
~~~~

### SRC-COMP-L1822-A8126630386F

- Kind: `requirement`
- Source: `compiler-proposal.md:1822-1822`
- Applicability: `OPT0`
- Exact text SHA-256: `a8126630386fcf65fe602db9b79c62eb1bc51a6bb85e5e05610969ce74bc7a07`

~~~~markdown
- Verter-native analysis only (`verter_analysis`, `type_info`, resolver);
~~~~

### SRC-COMP-L1823-7B94C64730A0

- Kind: `context`
- Source: `compiler-proposal.md:1823-1823`
- Applicability: `OPT0`
- Exact text SHA-256: `7b94c64730a0969aa67c66ad24123281ebcd41a7bb05c61b90daaf0f58a4d44a`

~~~~markdown
- internal analysis-depth strategy behind public `Optimized`;
~~~~

### SRC-COMP-L1824-75B65B46D0CD

- Kind: `context`
- Source: `compiler-proposal.md:1824-1824`
- Applicability: `OPT0`
- Exact text SHA-256: `75b65b46d0cd517e9971fe1256f34d9d8a705eebf22cdce167c11a105a932e81`

~~~~markdown
- `OptimizationRequestBasis` versus `OptimizationObservationSet`;
~~~~

### SRC-COMP-L1825-D3557005EEBC

- Kind: `requirement`
- Source: `compiler-proposal.md:1825-1825`
- Applicability: `OPT0`
- Exact text SHA-256: `d3557005eebc54cc1b30d695f3912737eec313f7ad42b69670ced647b8a1b34b`

~~~~markdown
- exact read-set validation, invalidation, cancellation and budgets;
~~~~

### SRC-COMP-L1826-519BB725643F

- Kind: `context`
- Source: `compiler-proposal.md:1826-1826`
- Applicability: `OPT0`
- Exact text SHA-256: `519bb725643fe6a12f21f0bfaff576193021f7e9927a8d691fb8f5b354521fdb`

~~~~markdown
- evidence/provenance representation and whether a generalized proof system is justified;
~~~~

### SRC-COMP-L1827-65915DEAB937

- Kind: `context`
- Source: `compiler-proposal.md:1827-1827`
- Applicability: `OPT0`
- Exact text SHA-256: `65915deab9378180d21c3c2ef029a9ab6c64318fa0981fc132ac4c94b67b758b`

~~~~markdown
- deterministic fallback to `Default`;
~~~~

### SRC-COMP-L1828-99826D178F45

- Kind: `context`
- Source: `compiler-proposal.md:1828-1828`
- Applicability: `OPT0`
- Exact text SHA-256: `99826d178f456a32585b0a232a6be5f85e747d4dd6cb163ac986694ddab437ae`

~~~~markdown
- artifact identity and reproducibility;
~~~~

### SRC-COMP-L1829-32FFF6C39956

- Kind: `context`
- Source: `compiler-proposal.md:1829-1829`
- Applicability: `OPT0`
- Exact text SHA-256: `32fff6c39956549c8fd3aec5af7c5116319705839ef05b298966d5fdc99d98a4`

~~~~markdown
- security, filesystem/package boundaries and RSS;
~~~~

### SRC-COMP-L1830-70D0F4F1B99D

- Kind: `context`
- Source: `compiler-proposal.md:1830-1830`
- Applicability: `OPT0`
- Exact text SHA-256: `70d0f4f1b99d21cae813606edf18a6b8faf75910408d5871a1a3bc718f82ebb4`

~~~~markdown
- per-framework target admission;
~~~~

### SRC-COMP-L1831-C318FD090326

- Kind: `context`
- Source: `compiler-proposal.md:1831-1831`
- Applicability: `OPT0`
- Exact text SHA-256: `c318fd090326546f4d308e058fbd8a898c20114808e2315440cbe9a2b0c2e002`

~~~~markdown
- independent benchmarks proving compile-cost versus runtime/code-size benefit.
~~~~

### SRC-COMP-L1833-B152B24F184D

- Kind: `acceptance`
- Source: `compiler-proposal.md:1833-1833`
- Applicability: `OPT0`
- Exact text SHA-256: `b152b24f184df2e03eaaf807ad77d18eb22610e11f6d6456bbbe2874ecce8013`

~~~~markdown
**Acceptance:** only a newly ratified plan and DAG amendment can close `OPT0` and create successors.
~~~~

### SRC-COMP-L1835-E2458F3EFD28

- Kind: `forbidden`
- Source: `compiler-proposal.md:1835-1835`
- Applicability: `OPT0`
- Exact text SHA-256: `e2458f3efd286ea402d4d35a5f98fa03a93d53d98b7def029dcd3397a010d213`

~~~~markdown
**Forbidden:** code, “temporary” project traversal, enabling `Optimized`, generic certificate/proof engines, or using ambient LSP facts.
~~~~

### SRC-COMP-L1837-16FC22FCDD87

- Kind: `deletion`
- Source: `compiler-proposal.md:1837-1837`
- Applicability: `OPT0`
- Exact text SHA-256: `16fc22fcdd872990b94e9c3502af796a3190dd16feeff7df23a009bee376b226`

~~~~markdown
**Deletion/abort:** none; remain `RESCOPE_REQUIRED` until maintainer action.
~~~~

### SRC-COMP-L1839-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1839-1839`
- Applicability: `OPT0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
