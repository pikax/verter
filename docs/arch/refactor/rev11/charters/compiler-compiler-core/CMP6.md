<!-- unified-charter-v2
id=CMP6
name=Cross-framework compiler-engine falsification
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=convergence
semantic_role=convergence
class=compiler
predecessors=VCP7,SCP7
conditional_predecessors=
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=compiler_execution,performance_evidence
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
release_gating=non_release
source_refs=source:compiler-proposal.md:L1754
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP6.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP6 — Cross-framework compiler-engine falsification

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Cross-framework compiler-engine falsification. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **VCP7:** exact current receipt ID and digest for “Vue Default compiler product terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SCP7:** exact current receipt ID and digest for “Svelte Default compiler product terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** attack every supposedly shared compiler abstraction after both default compilers land.
- **Problem:** common machinery can silently contain Vue- or Svelte-shaped semantics that were not visible before both implementations existed.
- **Solution and architecture decisions:**
- compare authority, data-layout, demand, target, artifact, map and host integration usage;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CMP6-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CMP6-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CMP6-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CMP6-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_vue_conformance/tests`, `crates/verter_svelte_conformance/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **dynamic dispatch inside node loops**.
- Delete or structurally reject: **whole-tree materialization fallback**.
- Delete or structurally reject: **unqualified artifact assembly**.
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

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1754`

## Reconciled source-plan contract

**Intent:** attack every supposedly shared compiler abstraction after both default compilers land.

**Problem:** common machinery can silently contain Vue- or Svelte-shaped semantics that were not visible before both implementations existed.

**Solution and architecture decisions:**

- compare authority, data-layout, demand, target, artifact, map and host integration usage;
- move framework-shaped concepts back to their owner;
- retain only semantics-neutral mechanics;
- do not promote shared compiler semantic operations after only two frameworks;
- require a third genuinely different compiler before a semantic operation can become common under the rule of three;
- publish follow-up architecture defects without revoking independently accepted products unless the defect invalidates their correctness basis.

**Suggested predecessors:** `VCP7`, `SCP7`.

**Normative source decomposition:** dependency graph audit, type/enum field audit, hot-path dispatch audit, data-layout comparison, deletion/move-back patches in owner blocks, read-only convergence review.

**Acceptance:** the shared engine contains no target/framework semantics and both compilers remain accepted after any move-back cleanup.

**Forbidden:** creating a universal IR, forcing symmetric representations, or coupling product releases.

**Deletion/abort:** delete false common abstractions through bounded owner amendments; this node is non-release.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1754-7A15BD7CE32F

- Kind: `context`
- Source: `compiler-proposal.md:1754-1754`
- Applicability: `CMP6`
- Exact text SHA-256: `7a15bd7ce32fb6c2f04e73250d3df34d78af8f8a9c908a2d12eb8da78b731edf`

~~~~markdown
## `CMP6.md` — Cross-framework compiler-engine falsification
~~~~

### SRC-COMP-L1756-DB716F88D1CA

- Kind: `context`
- Source: `compiler-proposal.md:1756-1756`
- Applicability: `CMP6`
- Exact text SHA-256: `db716f88d1ca2cd5c534de3874d143994211792dbb0186ea3970896d3bd5ad26`

~~~~markdown
**Intent:** attack every supposedly shared compiler abstraction after both default compilers land.
~~~~

### SRC-COMP-L1758-9EA98B78D5D9

- Kind: `context`
- Source: `compiler-proposal.md:1758-1758`
- Applicability: `CMP6`
- Exact text SHA-256: `9ea98b78d5d977b870b83b3a6d6443c297cfffc7a05db07a4654ff48190fecf7`

~~~~markdown
**Problem:** common machinery can silently contain Vue- or Svelte-shaped semantics that were not visible before both implementations existed.
~~~~

### SRC-COMP-L1760-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1760-1760`
- Applicability: `CMP6`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1762-52622253D647

- Kind: `context`
- Source: `compiler-proposal.md:1762-1762`
- Applicability: `CMP6`
- Exact text SHA-256: `52622253d6470a5962c04cd67b1485442007ce35cbb0df9ca8c4df96ece083c0`

~~~~markdown
- compare authority, data-layout, demand, target, artifact, map and host integration usage;
~~~~

### SRC-COMP-L1763-CE66AEC21268

- Kind: `context`
- Source: `compiler-proposal.md:1763-1763`
- Applicability: `CMP6`
- Exact text SHA-256: `ce66aec2126840c3d0f129a4828a3b8039fd625aaa6ed7df331b235d6fcdaf0a`

~~~~markdown
- move framework-shaped concepts back to their owner;
~~~~

### SRC-COMP-L1764-11146048A74E

- Kind: `requirement`
- Source: `compiler-proposal.md:1764-1764`
- Applicability: `CMP6`
- Exact text SHA-256: `11146048a74e008067bdb3c5afa15f90a82dbf6842c9c474eebea297103100c0`

~~~~markdown
- retain only semantics-neutral mechanics;
~~~~

### SRC-COMP-L1765-C36C76036168

- Kind: `requirement`
- Source: `compiler-proposal.md:1765-1765`
- Applicability: `CMP6`
- Exact text SHA-256: `c36c76036168069002c951942f46a3e1d2bc7a9c6cf6af21f869eca31127c481`

~~~~markdown
- do not promote shared compiler semantic operations after only two frameworks;
~~~~

### SRC-COMP-L1766-CF9BE1A1A87E

- Kind: `context`
- Source: `compiler-proposal.md:1766-1766`
- Applicability: `CMP6`
- Exact text SHA-256: `cf9be1a1a87ef4ddd276f383a8124756fcaeaffaf215d594a151bf8131f4171c`

~~~~markdown
- require a third genuinely different compiler before a semantic operation can become common under the rule of three;
~~~~

### SRC-COMP-L1767-7471ACE53F00

- Kind: `context`
- Source: `compiler-proposal.md:1767-1767`
- Applicability: `CMP6`
- Exact text SHA-256: `7471ace53f00aadd1880b0b1e5a1948c73a425a43ead0bfb39494524fd6cf1d5`

~~~~markdown
- publish follow-up architecture defects without revoking independently accepted products unless the defect invalidates their correctness basis.
~~~~

### SRC-COMP-L1769-158CAE019A6E

- Kind: `context`
- Source: `compiler-proposal.md:1769-1769`
- Applicability: `CMP6`
- Exact text SHA-256: `158cae019a6e0aeae206398211c47dc2309486847ebf015600af00666f1f3ab0`

~~~~markdown
**Suggested predecessors:** `VCP7`, `SCP7`.
~~~~

### SRC-COMP-L1771-FE360A40CB6D

- Kind: `deletion`
- Source: `compiler-proposal.md:1771-1771`
- Applicability: `CMP6`
- Exact text SHA-256: `fe360a40cb6d0759236196b30e5504150f2ee325bf7a02a45101989979235fdc`

~~~~markdown
**Suggested subblocks:** dependency graph audit, type/enum field audit, hot-path dispatch audit, data-layout comparison, deletion/move-back patches in owner blocks, read-only convergence review.
~~~~

### SRC-COMP-L1773-6F3A30AF6874

- Kind: `acceptance`
- Source: `compiler-proposal.md:1773-1773`
- Applicability: `CMP6`
- Exact text SHA-256: `6f3a30af6874e11af53401ef4930ee0fc3f8b1276fd3045f3bd09f6b5d5c3852`

~~~~markdown
**Acceptance:** the shared engine contains no target/framework semantics and both compilers remain accepted after any move-back cleanup.
~~~~

### SRC-COMP-L1775-A02CDC7E4389

- Kind: `forbidden`
- Source: `compiler-proposal.md:1775-1775`
- Applicability: `CMP6`
- Exact text SHA-256: `a02cdc7e4389347542348b86068c284cc13272907ef59cc88eadde7eb91f9564`

~~~~markdown
**Forbidden:** creating a universal IR, forcing symmetric representations, or coupling product releases.
~~~~

### SRC-COMP-L1777-2970E2BEA046

- Kind: `deletion`
- Source: `compiler-proposal.md:1777-1777`
- Applicability: `CMP6`
- Exact text SHA-256: `2970e2bea0461babef8d2715f9ec325fd5f0735c23141b9283eacca0d2123494`

~~~~markdown
**Deletion/abort:** delete false common abstractions through bounded owner amendments; this node is non-release.
~~~~

### SRC-COMP-L1779-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1779-1779`
- Applicability: `CMP6`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
